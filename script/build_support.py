"""Shared portable-build checks; never access user configuration or credentials."""
from __future__ import annotations

import hashlib
import json
import os
import stat
import struct
import subprocess
import tarfile
import urllib.request
import uuid
import zipfile
from pathlib import Path

from build_hybrid_windows import BuildError, require, relative_member, write_json

ROOT = Path(__file__).resolve().parents[1]
PINS = ROOT / "resources/build-inputs.lock.json"


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fetch(pin: dict, destination: Path) -> Path:
    """Only use a pinned HTTPS URL. Publish a complete, verified new download."""
    require(pin["url"].startswith("https://"), "Build input must use HTTPS")
    if destination.exists():
        require(destination.is_file() and destination.stat().st_size == pin["bytes"] and sha256(destination) == pin["sha256"], "Cached build input differs from its pin")
        return destination
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(destination.name + "." + uuid.uuid4().hex + ".part")
    # No proxy credentials, custom index or user-controlled request headers.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(pin["url"], timeout=90) as response, temporary.open("xb") as stream:
        count = 0
        while chunk := response.read(1024 * 1024):
            count += len(chunk)
            require(count <= pin["bytes"], "Download exceeds its pinned size")
            stream.write(chunk)
    require(count == pin["bytes"] and sha256(temporary) == pin["sha256"], "Build input checksum differs")
    require(not destination.exists(), "Build input appeared during download")
    temporary.rename(destination)
    return destination


def extract_archive(archive: Path, destination: Path, *, standalone: bool = False) -> None:
    require(not destination.exists(), "Extraction destination already exists")
    destination.mkdir(parents=True)
    if zipfile.is_zipfile(archive):
        with zipfile.ZipFile(archive) as source:
            for member in source.infolist():
                relative_member(member.filename.rstrip("/"))
                if member.is_dir():
                    continue
                require(not member.flag_bits & 1 and not stat.S_ISLNK(member.external_attr >> 16), "Encrypted or linked ZIP member")
            source.extractall(destination)
    else:
        with tarfile.open(archive) as source:
            selected = []
            for member in source.getmembers():
                name = member.name.removeprefix("./").rstrip("/")
                if not name:
                    continue
                relative_member(name)
                require(member.isfile() or member.isdir() or member.issym(), "Unsupported archive entry")
                # Python's data filter also verifies internal symlink targets.
                tarfile.data_filter(member, str(destination))
                if standalone and (name.startswith("python/lib/python3.13/site-packages/") or name in {"python/bin/pip", "python/bin/pip3", "python/bin/pip3.13"}):
                    continue
                selected.append(member)
            source.extractall(destination, members=selected, filter="data")
    checked_files(destination)


def checked_files(root: Path) -> list[Path]:
    """Allow only regular files and in-tree relative links, including runtime aliases."""
    root = root.absolute()
    result = []
    for directory, dirs, names in os.walk(root, followlinks=False):
        for name in sorted(dirs + names):
            path = Path(directory) / name
            info = path.lstat()
            if path.is_symlink():
                target = os.readlink(path)
                require(not Path(target).is_absolute() and path.resolve(strict=True).is_relative_to(root.resolve()), "Bundle link escapes its root")
                result.append(path)
            else:
                require(not getattr(info, "st_file_attributes", 0) & 0x400, "Unreviewed reparse point")
                require(path.is_file() or path.is_dir(), "Unsupported bundle member")
                if path.is_file():
                    result.append(path)
    return sorted(result)


def manifest(root: Path, destination: Path) -> list[dict]:
    records = []
    for path in checked_files(root):
        if path == destination:
            continue
        name = path.name.lower()
        require(name not in {".env", "config.v3.toml", "credentials.json", "secrets.json"}, "User configuration in build output")
        require("gradio" not in path.relative_to(root).parts and "pdf2zh_next" not in path.relative_to(root).parts, "Legacy application in build output")
        record = {"path": path.relative_to(root).as_posix()}
        if path.is_symlink():
            record.update(kind="symlink", target=os.readlink(path))
        else:
            record.update(kind="file", bytes=path.stat().st_size, sha256=sha256(path), executable=bool(path.stat().st_mode & 0o111))
        records.append(record)
    write_json(destination, records)
    return records


def verify_manifest(root: Path, manifest_path: Path) -> None:
    expected = json.loads(manifest_path.read_bytes())
    names = {row["path"] for row in expected}
    actual = {path.relative_to(root).as_posix() for path in checked_files(root) if path != manifest_path}
    require(names == actual and len(names) == len(expected), "Bundle file set differs from manifest")
    for row in expected:
        path = root / str(relative_member(row["path"]))
        if row.get("kind") == "symlink":
            require(path.is_symlink() and os.readlink(path) == row["target"], "Bundle link differs")
        else:
            require(not path.is_symlink() and path.stat().st_size == row["bytes"] and sha256(path) == row["sha256"], "Bundle file checksum differs")


def zip_bundle(root: Path, destination: Path) -> None:
    require(not destination.exists(), "Artifact archive already exists")
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for path in checked_files(root):
            name = f"{root.name}/{path.relative_to(root).as_posix()}"
            if path.is_symlink():
                info = zipfile.ZipInfo(name)
                info.create_system = 3
                info.external_attr = (stat.S_IFLNK | 0o777) << 16
                archive.writestr(info, os.readlink(path).encode())
            else:
                archive.write(path, name)
    with zipfile.ZipFile(destination) as archive:
        require(archive.testzip() is None, "Artifact ZIP CRC check failed")


def isolated_environment(work: Path) -> dict[str, str]:
    env = {key: os.environ[key] for key in ("PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "SYSTEMDRIVE") if key in os.environ}
    home, temporary = work / "home", work / "tmp"
    for path in (home, temporary):
        path.mkdir(parents=True, exist_ok=True)
    env.update(HOME=str(home), USERPROFILE=str(home), TEMP=str(temporary), TMP=str(temporary), TMPDIR=str(temporary),
               UV_CACHE_DIR=str(work / "uv-cache"), UV_NO_CONFIG="1", UV_PYTHON_DOWNLOADS="never", PYTHONUTF8="1",
               BABELOXIDE_APP_DIR=str(work / "app"), NO_PROXY="127.0.0.1,localhost", no_proxy="127.0.0.1,localhost")
    return env


def run(command: list[str | Path], work: Path, env: dict[str, str], *, timeout: int = 1800, label: str) -> None:
    print(label + "...", flush=True)
    with (work / f"{label}.log").open("wb") as stream:
        result = subprocess.run([str(arg) for arg in command], cwd=work, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=timeout)
    require(result.returncode == 0, f"{label} failed; see {work / (label + '.log')}")


def _macho_commands(data: bytes) -> dict | None:
    if data[:4] in {b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"}:
        wide = data[:4] == b"\xca\xfe\xba\xbf"
        count = struct.unpack_from(">I", data, 4)[0]
        require(0 < count <= 32, "Invalid universal Mach-O header")
        for index in range(count):
            at = 8 + index * (32 if wide else 20)
            cpu = struct.unpack_from(">I", data, at)[0]
            if cpu == 0x0100000C:
                offset, size = struct.unpack_from(">QQ" if wide else ">II", data, at + 8)
                require(offset + size <= len(data), "Truncated universal Mach-O")
                return _macho_commands(data[offset:offset + size])
        raise BuildError("Native library has no arm64 slice")
    if data[:4] != b"\xcf\xfa\xed\xfe":
        return None
    require(len(data) >= 32 and struct.unpack_from("<I", data, 4)[0] == 0x0100000C, "Native binary is not macOS arm64")
    count, size = struct.unpack_from("<II", data, 16)
    require(count <= 10000 and 32 + size <= len(data), "Truncated Mach-O commands")
    dependencies, rpaths = [], []
    offset = 32
    for _ in range(count):
        command, length = struct.unpack_from("<II", data, offset)
        require(length >= 8 and offset + length <= 32 + size, "Invalid Mach-O command size")
        if command in {0xC, 0x18, 0x80000018, 0x1F, 0x8000001F, 0x23, 0x80000023, 0x8000001C}:
            start = struct.unpack_from("<I", data, offset + 8)[0]
            require(12 <= start < length, "Invalid Mach-O load name")
            name = data[offset + start:offset + length].split(b"\0", 1)[0].decode("utf-8")
            (rpaths if command == 0x8000001C else dependencies).append(name)
        offset += length
    return {"dependencies": dependencies, "rpaths": rpaths}


def audit_macos(root: Path, runtime: Path) -> list[dict]:
    records = []
    main_paths = [root / "BabelOxide", root / "BabelOxide-gui", runtime / "bin/python3.13"]
    mains = {path: _macho_commands(path.read_bytes()) for path in main_paths}
    for path in checked_files(root):
        if path.is_symlink():
            continue
        with path.open("rb") as stream:
            magic = stream.read(4)
        if magic not in {b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf", b"\xce\xfa\xed\xfe"}:
            continue
        commands = _macho_commands(path.read_bytes())
        require(commands is not None, "Invalid native binary")
        executable = runtime / "bin/python3.13" if path.is_relative_to(runtime) else root / "BabelOxide-gui"
        origins = [(path, value) for value in commands["rpaths"]]
        origins += [(executable, value) for value in (mains[executable] or {}).get("rpaths", [])]

        def expand(value: str, origin: Path) -> Path:
            if value.startswith("@loader_path/"):
                return origin.parent / value.removeprefix("@loader_path/")
            if value.startswith("@executable_path/"):
                return executable.parent / value.removeprefix("@executable_path/")
            if value == "@loader_path":
                return origin.parent
            return Path(value)

        for dependency in commands["dependencies"]:
            if dependency.startswith(("/usr/lib/", "/System/Library/")):
                continue
            if dependency.startswith("@rpath/"):
                candidates = [expand(value, origin) / dependency.removeprefix("@rpath/") for origin, value in origins]
            else:
                candidates = [expand(dependency, path)]
            require(any(item.exists() and item.resolve().is_relative_to(root.resolve()) for item in candidates), f"Unresolved/nonportable native dependency in {path.relative_to(root)}: {dependency}")
        records.append({"path": path.relative_to(root).as_posix(), "architecture": "arm64", **commands})
    require(bool(records), "No macOS native binaries were audited")
    return records
