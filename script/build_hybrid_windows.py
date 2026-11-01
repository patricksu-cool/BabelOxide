"""Build a new Windows x64 Rust/BabelDOC bundle; never build Cargo or reuse a venv.

Python 3.11+ is needed on the build machine, not on the target machine. All external
commands receive an allowlisted environment. --plan, --write-lock and --self-test
do not install packages, launch the application, or create a release directory.
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import importlib.metadata
import json
import os
import re
import shutil
import sqlite3
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest
import urllib.parse
import uuid
import zipfile
from contextlib import closing
from pathlib import Path
from pathlib import PurePosixPath

import tomllib

REPO = Path(__file__).resolve().parents[1]
INVENTORY = REPO / "python-worker/dependencies-win64.json"
ASSET_INVENTORY = REPO / "resources/engine-assets.lock.json"
REQUIREMENTS = REPO / "python-worker/requirements-win64.lock.txt"
WHEEL_OVERRIDES = REPO / "python-worker/wheel-overrides.json"
BASELINE_WHEELS = REPO / "python-worker/baseline-wheels.json"
BUILD_INPUTS = REPO / "build-inputs/windows-x64"
PYTHON_VERSION = "3.13.16"
ENGINE_COUNT = 85
PDFIUM_VERSION = "158.0.8125.0"
PDFIUM_SHA256 = "4b0dec92555d69437bef0df37be630181ab3e83a6823ca264ede355a4f2ef994"
PDFIUM_BYTES = 7586816
PDFIUM_ARCHIVE_SHA256 = "9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b"
PDFIUM_URL = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8125/pdfium-win-x64.tgz"
ASSET_TAG = "b485a82328a20d1e4b5c37bb9c6472e7af9943333bdffd39a05b1f57d6d7a1da"
ASSET_SHA256 = "b32dfc00c5ade7b59094f82b774fef31b5b504e9c2aab783b842bf9a5792ba98"
ASSET_BYTES = 223571497
PTH_CONTENT = "python313.zip\n.\nsite-packages\n"
RUNTIME_PINS = {
    "python.exe": (105696, "31fe10b4ed82a2f960af0ef9931f46a4b84ff286f8b6ffd9dee098cf4f10f17f"),
    "python313.dll": (6142176, "3b29bf9ff39bed8f660f5b935f19657f7ce7f145009851741b11364f6ebcb862"),
    "python313.zip": (3837053, "33c48dd2c3031d27784950aaee96d3d65f916e3c43dd3637ace333a7b9e8021a"),
    "python313._pth": (80, "35ddf94682ff9aa713a8d63557242ad00f3f28fdd39337f02c3bda4c0f791577"),
    "LICENSE.txt": (33861, "62bec384df47b0328307db41455ff6ea2559e5546b394ac69148561b21703120"),
}
# OS components only. VC redistributables are deliberately NOT in this list.
OS_DLLS = frozenset(
    ("advapi32 avrt bcrypt bcryptprimitives cabinet cfgmgr32 comctl32 comdlg32 "
     "combase crypt32 cryptbase cryptsp d2d1 d3d9 d3d11 d3d12 dcomp dbghelp "
     "dnsapi dsound dwmapi dwrite dxgi dxguid gdi32 gdiplus glu32 hid imagehlp "
     "imm32 iphlpapi kernel32 kernelbase mf mfplat mfreadwrite mfuuid mmdevapi "
     "msacm32 mscoree msimg32 msvcrt ncrypt netapi32 normaliz ntdll ole32 "
     "oleacc oleaut32 opengl32 pdh powrprof propsys psapi rasapi32 rpcrt4 runtimeobject "
     "secur32 setupapi shcore shell32 shlwapi srvcli ucrtbase urlmon user32 "
     "userenv usp10 uxtheme version winhttp wininet winmm winspool wintrust "
     "wldap32 wkscli ws2_32 wtsapi32").split()
)


class BuildError(Exception):
    """An actionable build failure; never includes process output or environment."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise BuildError(message)


def rename_with_transient_retry(source: Path, destination: Path) -> Path:
    """Retry short-lived Windows access/sharing failures without replacing output."""
    deadline = time.monotonic() + 8.0
    for attempt in range(33):
        # Recheck between attempts: another process may have created the target.
        if os.path.lexists(destination):
            raise FileExistsError(f"Destination already exists: {destination}")
        try:
            return source.rename(destination)
        except PermissionError as error:
            if getattr(error, "winerror", None) not in (5, 32) or attempt == 32:
                raise
            delay = min(0.25, deadline - time.monotonic())
            if delay <= 0:
                raise
            time.sleep(delay)
    raise AssertionError("unreachable")


def canonical_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def digest(path: Path, algorithm: str = "sha256") -> str:
    result = hashlib.new(algorithm)
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def source_text_digest(path: Path) -> str:
    """Fingerprint reviewed text independently of Git's Windows CRLF checkout.

    Release-file and BUILD_INFO checksums still hash the actual captured bytes.
    """
    return hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2, sort_keys=True)
        stream.write("\n")


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def plain_path(path: Path) -> Path:
    """Refuse links/junctions in either existing inputs or output ancestors."""
    path = Path(os.path.abspath(path))  # noqa: PTH100 -- inspect reparse points before resolving links
    for component in (path, *path.parents):
        try:
            info = component.lstat()
        except FileNotFoundError:
            continue
        require(
            not stat.S_ISLNK(info.st_mode)
            and not getattr(info, "st_file_attributes", 0) & 0x400,
            f"Links or reparse points are not allowed: {component}",
        )
    return path


def relative_member(name: str) -> PurePosixPath:
    """A portable relative file name, including Windows reserved-name checks."""
    path = PurePosixPath(name)
    require(bool(name) and "\\" not in name and not path.is_absolute(), "Unsafe archive/manifest path")
    require(str(path) == name and not name.endswith("/"), "Noncanonical file path")
    reserved = {"con", "prn", "aux", "nul", *(f"com{i}" for i in range(1, 10)), *(f"lpt{i}" for i in range(1, 10))}
    for part in path.parts:
        require(
            part not in {".", ".."}
            and not re.search(r'[<>:"|?*\x00-\x1f]', part)
            and part[-1:] not in {".", " "}
            and part.split(".")[0].lower() not in reserved,
            "Unsafe archive/manifest path",
        )
    return path


def files_below(root: Path) -> list[Path]:
    plain_path(root)
    require(root.is_dir(), f"Required directory is missing: {root}")
    files = []
    for directory, dirs, names in os.walk(root, followlinks=False):
        for name in sorted(dirs + names):
            item = plain_path(Path(directory) / name)
            if item.is_file():
                files.append(item)
            else:
                require(item.is_dir(), f"Not a regular file/directory: {item}")
    return sorted(files)


def checked_copy(source: Path, destination: Path, expected: tuple[int, str] | None = None) -> None:
    plain_path(source)
    require(source.is_file(), f"Required source file is missing: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    require(not destination.exists(), f"Refusing to replace a staged file: {destination}")
    shutil.copyfile(source, destination)
    if expected is not None:
        verify_file(destination, *expected)


def verify_file(path: Path, size: int, sha256: str) -> None:
    plain_path(path)
    require(path.is_file() and path.stat().st_size == size, f"Size mismatch or missing file: {path}")
    require(digest(path) == sha256.lower(), f"SHA256 mismatch: {path}")


def wheel_rank(url: str) -> tuple[int, int, str] | None:
    parsed = urllib.parse.urlsplit(url)
    require(
        parsed.scheme == "https" and parsed.hostname == "files.pythonhosted.org"
        and parsed.username is None and parsed.password is None
        and not parsed.query and not parsed.fragment and parsed.port in {None, 443},
        "Wheel URL must be a credential-free files.pythonhosted.org HTTPS URL",
    )
    filename = urllib.parse.unquote(PurePosixPath(parsed.path).name)
    require(re.fullmatch(r"[A-Za-z0-9_.+-]+\.whl", filename) is not None, "Expected a safe wheel filename")
    pieces = filename[:-4].rsplit("-", 3)
    require(len(pieces) == 4, "Invalid wheel name")
    _, python, abi, platform = pieces
    if platform not in {"win_amd64", "any"}:
        return None
    ranks = []
    for tag in python.split("."):
        if tag == "cp313" and abi == "cp313" and platform == "win_amd64":
            ranks.append((5, 13, filename))
        elif (match := re.fullmatch(r"cp3(\d+)", tag)) and abi == "abi3" and platform == "win_amd64":
            minor = int(match[1])
            if minor <= 13:
                ranks.append((4, minor, filename))
        elif abi == "none" and (tag == "py3" or re.fullmatch(r"py3(?:[0-9]|1[0-3])", tag)):
            ranks.append((3 if platform == "win_amd64" else 2, 0, filename))
    return max(ranks) if ranks else None


def select_wheels(inventory: dict, profile: dict, expected_count: int = ENGINE_COUNT) -> list[dict]:
    require(inventory["environment"]["python_full_version"] == PYTHON_VERSION, "Inventory Python version changed")
    engine = inventory["engine"]
    require(len(engine) == expected_count, "Engine distribution count changed; review the closure first")
    require(engine["babeldoc"]["version"] == "0.6.3", "Unexpected BabelDOC version")
    require(set(profile) == {"schema_version", "python", "platform", "wheels"} and profile["schema_version"] == 1, "Unsupported baseline wheel profile")
    require(profile["python"] == PYTHON_VERSION and profile["platform"] == "win_amd64", "Wrong baseline wheel platform")
    wheels = profile["wheels"]
    require(isinstance(wheels, list) and len(wheels) == expected_count, "Wrong baseline wheel count")
    names = set()
    for wheel in wheels:
        require(isinstance(wheel, dict) and set(wheel) == {"name", "version", "url", "sha256", "bytes"}, "Invalid baseline wheel fields")
        name = wheel["name"]
        require(isinstance(name, str) and canonical_name(name) == name and name in engine and name not in names, "Unexpected or duplicate baseline wheel")
        names.add(name)
        require(wheel["version"] == engine[name]["version"], f"Baseline wheel version differs from inventory: {name}")
        require(wheel_rank(wheel["url"]) is not None, "Incompatible baseline wheel")
        filename = urllib.parse.unquote(PurePosixPath(urllib.parse.urlsplit(wheel["url"]).path).name)
        identity = filename[:-4].rsplit("-", 3)[0].split("-")
        require(len(identity) == 2 and canonical_name(identity[0]) == name and identity[1] == wheel["version"], "Baseline wheel identity differs from profile")
        require(isinstance(wheel["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", wheel["sha256"]) is not None, "Invalid baseline wheel SHA256")
        require(type(wheel["bytes"]) is int and wheel["bytes"] > 0, "Invalid baseline wheel size")
    require(names == set(engine), "Baseline wheel set differs from inventory")
    return sorted(wheels, key=lambda wheel: wheel["name"])


def apply_wheel_overrides(wheels: list[dict], profile: dict) -> list[dict]:
    """Apply reviewed Worker-only replacements to the captured baseline."""
    require(set(profile) == {"schema_version", "packages"} and profile["schema_version"] == 1, "Unsupported Worker wheel profile")
    require(isinstance(profile["packages"], dict), "Worker wheel replacements must be a mapping")
    selected = {wheel["name"]: dict(wheel) for wheel in wheels}
    require(len(selected) == len(wheels), "Duplicate baseline wheel")
    fields = {"from_version", "version", "url", "sha256", "bytes", "reason", "metadata_source"}
    for name, replacement in profile["packages"].items():
        require(canonical_name(name) == name and name in selected, "Replacement is not in the engine closure")
        require(isinstance(replacement, dict) and set(replacement) == fields, "Incomplete or unknown wheel replacement fields")
        require(replacement["from_version"] == selected[name]["version"], f"Stale replacement baseline: {name}")
        version = replacement["version"]
        require(isinstance(version, str) and re.fullmatch(r"[0-9]+(?:\.[0-9]+)+(?:[a-z0-9.]+)?", version) is not None and version != replacement["from_version"], "Invalid replacement version")
        require(isinstance(replacement["reason"], str) and bool(replacement["reason"].strip()), "Wheel replacement needs a review reason")
        require(wheel_rank(replacement["url"]) is not None, "Replacement wheel is not compatible with Windows x64 CPython 3.13")
        filename = urllib.parse.unquote(PurePosixPath(urllib.parse.urlsplit(replacement["url"]).path).name)
        identity = filename[:-4].rsplit("-", 3)[0].split("-")
        require(len(identity) == 2 and canonical_name(identity[0]) == name and identity[1] == version, "Replacement wheel identity differs from profile")
        require(re.fullmatch(r"[0-9a-f]{64}", replacement["sha256"]) is not None, "Replacement wheel SHA256 is invalid")
        require(type(replacement["bytes"]) is int and replacement["bytes"] > 0, "Replacement wheel size is invalid")
        require(replacement["metadata_source"] == f"https://pypi.org/pypi/{name}/{version}/json", "Replacement metadata source is not the exact official PyPI version")
        selected[name] = {"name": name, **{key: replacement[key] for key in ("version", "url", "sha256", "bytes")}}
    return [selected[wheel["name"]] for wheel in wheels]


def lock_text(wheels: list[dict], inventory_sha256: str, baseline_sha256: str, overrides_sha256: str) -> str:
    lines = [
        "# Generated by script/build_hybrid_windows.py --write-lock; do not edit by hand.",
        f"# Windows x64 / CPython {PYTHON_VERSION} / BabelDOC 0.6.3 / {len(wheels)} distributions.",
        "# Source text fingerprints below normalize CRLF to LF; release manifests hash actual bytes.",
        f"# dependencies-win64.json sha256: {inventory_sha256}",
        f"# baseline-wheels.json sha256: {baseline_sha256}",
        f"# wheel-overrides.json sha256: {overrides_sha256}",
        "# One exact compatible wheel per package; no sdist, index resolution or transitive installation.",
        "",
    ]
    for wheel in wheels:
        lines += [f"# {wheel['name']}=={wheel['version']}", f"{wheel['name']} @ {wheel['url']} --hash=sha256:{wheel['sha256']}"]
    return "\n".join(lines) + "\n"


def write_lock_file(path: Path, content: str) -> None:
    """Replace only the authorized lock, without following links or truncation."""
    path = plain_path(path)
    require(path.parent.is_dir(), "Worker lock parent is missing")
    descriptor, name = tempfile.mkstemp(prefix=f".{path.name}.build-", suffix=".tmp", dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        plain_path(path)
        temporary.replace(path)
    finally:
        if temporary.exists():
            plain_path(temporary)
            require(temporary.parent == path.parent, "Lock cleanup escaped its parent")
            temporary.unlink()


def runtime_files(source: Path, manifest: Path) -> list[dict]:
    records = read_json(manifest)
    require(isinstance(records, list), "Runtime manifest must be a file list")
    selected = []
    seen = set()
    for record in records:
        relative = relative_member(record["path"])
        if relative.parts[0] != "runtime":
            continue
        require(len(relative.parts) == 2, "Embedded runtime must not contain installed application packages")
        name = relative.name
        require(name.lower() not in seen, "Duplicate runtime manifest file")
        seen.add(name.lower())
        verify_file(source / name, record["bytes"], record["sha256"])
        selected.append({"name": name, "bytes": record["bytes"], "sha256": record["sha256"].lower()})
    require(bool(selected), "No runtime files in manifest")
    actual = {p.relative_to(source).as_posix() for p in files_below(source)}
    require(actual == {r["name"] for r in selected}, "Unmanifested or missing files in embedded runtime source")
    for name, pin in RUNTIME_PINS.items():
        verify_file(source / name, *pin)
    # An embedded runtime, never a renamed PyStand/application launcher.
    require(not any("pystand" in name or name.endswith(".int") for name in seen), "Legacy launcher in runtime")
    return sorted(selected, key=lambda record: record["name"])


def asset_members(inventory: dict) -> dict[str, dict]:
    require(
        inventory["babeldoc_version"] == "0.6.3" and inventory["asset_tag"] == ASSET_TAG
        and inventory["archive_sha256"] == ASSET_SHA256 and inventory["archive_bytes"] == ASSET_BYTES,
        "Engine asset inventory provenance changed",
    )
    expected = {}
    for entry in inventory["assets"]:
        name = str(relative_member(f"{entry['category']}/{entry['name']}"))
        require(name.lower() not in {key.lower() for key in expected}, "Duplicate asset path")
        require(re.fullmatch(r"[0-9a-f]{64}", entry["sha3_256"]) is not None, "Invalid asset SHA3")
        expected[name] = entry
    require(collections.Counter(e["category"] for e in expected.values()) == {"fonts": 34, "cmap": 146, "models": 1, "tiktoken": 1}, "Unexpected asset closure")
    require(len(expected) == inventory["verified_files"] == 182, "Expected 182 effective engine assets")
    require(sum(e["bytes"] for e in expected.values()) == inventory["content_bytes"] == 352451566, "Asset content size mismatch")
    return expected


def verify_zip(path: Path, expected: dict[str, dict]) -> None:
    verify_file(path, ASSET_BYTES, ASSET_SHA256)
    with zipfile.ZipFile(path) as archive:
        seen = set()
        for entry in archive.infolist():
            name = str(relative_member(entry.filename))
            require(name not in seen, "Duplicate ZIP entry")
            seen.add(name)
            require(name in expected and entry.file_size == expected[name]["bytes"], "Unexpected asset ZIP entry or size")
            require(not entry.flag_bits & 1 and not stat.S_ISLNK(entry.external_attr >> 16), "Encrypted/link asset ZIP entry")
        require(seen == set(expected), "Asset ZIP does not match the effective engine manifest")


def verify_restored(home: Path, expected: dict[str, dict]) -> None:
    cache = home / ".cache/babeldoc"
    for name, entry in expected.items():
        path = plain_path(cache / name)
        require(path.is_file() and path.stat().st_size == entry["bytes"], f"Missing restored asset: {name}")
        require(digest(path, "sha3_256") == entry["sha3_256"], f"Restored asset SHA3 mismatch: {name}")
    actual = {p.relative_to(cache).as_posix() for p in files_below(cache)}
    require(actual == set(expected), "Unexpected files in restored engine cache")


def engine_seed_manifest(home: Path, expected: dict[str, dict]) -> dict:
    """Describe only verified immutable assets for the Rust portable cache seeder."""
    verify_restored(home, expected)
    cache = home / ".cache/babeldoc"
    files = []
    for name, entry in sorted(expected.items()):
        relative_member(name)
        require(name == f"{entry['category']}/{entry['name']}", "Seed asset identity differs from inventory")
        files.append({"category": entry["category"], "name": entry["name"], "bytes": entry["bytes"], "sha256": digest(cache / name)})
    return {"schema_version": 1, "asset_tag": ASSET_TAG, "files": files}


def pe_imports(path: Path) -> dict:
    """Read x64 PE normal/delay imports without loading untrusted native code."""
    data = path.read_bytes()

    def unpack(fmt: str, offset: int):
        require(0 <= offset <= len(data) - struct.calcsize(fmt), f"Truncated PE: {path.name}")
        return struct.unpack_from(fmt, data, offset)

    require(data[:2] == b"MZ", f"Not a PE executable: {path.name}")
    pe = unpack("<I", 0x3C)[0]
    require(data[pe:pe + 4] == b"PE\0\0", f"Invalid PE signature: {path.name}")
    machine, sections = unpack("<HH", pe + 4)
    optional_size = unpack("<H", pe + 20)[0]
    optional = pe + 24
    require(machine == 0x8664 and unpack("<H", optional)[0] == 0x20B, f"Expected x64 PE32+: {path.name}")
    require(optional_size >= 112 + 14 * 8, "PE data directories are incomplete")
    image_base = unpack("<Q", optional + 24)[0]
    headers_size = unpack("<I", optional + 60)[0]
    section_headers = []
    for index in range(sections):
        offset = optional + optional_size + index * 40
        virtual_size, virtual, raw_size, raw = unpack("<IIII", offset + 8)
        section_headers.append((virtual, virtual_size, raw, raw_size))

    def rva_offset(rva: int, size: int = 1) -> int:
        if rva < headers_size and rva + size <= min(headers_size, len(data)):
            return rva
        for virtual, virtual_size, raw, raw_size in section_headers:
            if virtual <= rva < virtual + max(virtual_size, raw_size):
                delta = rva - virtual
                require(delta + size <= raw_size and raw + delta + size <= len(data), "PE RVA outside raw data")
                return raw + delta
        raise BuildError("Unmapped PE RVA")

    def dll_name(rva: int) -> str:
        offset = rva_offset(rva)
        end = data.find(b"\0", offset, min(offset + 512, len(data)))
        require(end != -1, "Unterminated PE import name")
        try:
            name = data[offset:end].decode("ascii").lower()
        except UnicodeDecodeError as exc:
            raise BuildError("Invalid PE import name") from exc
        require(re.fullmatch(r"[a-z0-9_.-]+\.dll", name) is not None, "Unsafe PE import name")
        return name

    imports = {}
    for label, directory, width in (("normal", 1, 20), ("delay", 13, 32)):
        rva, size = unpack("<II", optional + 112 + directory * 8)
        names = []
        if rva:
            require(width <= size <= 1024 * 1024, "Invalid PE import table size")
            for index in range(size // width):
                descriptor = unpack("<" + "I" * (width // 4), rva_offset(rva + index * width, width))
                if not any(descriptor):
                    break
                name_rva = descriptor[3] if label == "normal" else descriptor[1]
                if label == "delay" and not descriptor[0] & 1:
                    name_rva -= image_base
                names.append(dll_name(name_rva))
            else:
                raise BuildError("Unterminated PE import table")
        imports[label] = sorted(set(names))
    return {"machine": "AMD64", **imports}


def native_inventory(stage: Path) -> tuple[list[dict], list[dict]]:
    files = files_below(stage)
    dlls = collections.defaultdict(list)
    for path in files:
        if path.suffix.lower() == ".dll":
            dlls[path.name.lower()].append(path.relative_to(stage).as_posix())
    records, missing = [], []
    for path in files:
        if path.suffix.lower() not in {".exe", ".dll", ".pyd"}:
            continue
        record = {"path": path.relative_to(stage).as_posix(), "bytes": path.stat().st_size, "sha256": digest(path), **pe_imports(path)}
        classifications = {}
        for name in sorted(set(record["normal"] + record["delay"])):
            if name.startswith(("api-ms-", "ext-ms-")) or name[:-4] in OS_DLLS:
                classifications[name] = {"class": "windows-component"}
            elif name in dlls:
                # Presence is static evidence, not proof of the native loader's path.
                classifications[name] = {"class": "bundled-candidate", "paths": dlls[name]}
            else:
                classifications[name] = {"class": "unresolved"}
                missing.append({"importer": record["path"], "dll": name})
        record["dependencies"] = classifications
        records.append(record)
    return records, missing


def sanitized_environment(work: Path, *, stage: Path | None = None) -> dict[str, str]:
    system_root = os.environ.get("SystemRoot") or os.environ.get("WINDIR")
    require(bool(system_root), "Windows SystemRoot is required")
    system32 = Path(system_root) / "System32"
    home, temporary = work / "home", work / "tmp"
    home.mkdir(parents=True, exist_ok=True)
    temporary.mkdir(parents=True, exist_ok=True)
    result = {
        "SystemRoot": system_root, "WINDIR": system_root,
        "COMSPEC": str(system32 / "cmd.exe"), "PATH": str(system32),
        "HOME": str(home), "USERPROFILE": str(home), "TEMP": str(temporary), "TMP": str(temporary),
        "LOCALAPPDATA": str(home / "AppData/Local"), "APPDATA": str(home / "AppData/Roaming"),
        "PYTHONNOUSERSITE": "1", "PYTHONDONTWRITEBYTECODE": "1", "PYTHONUTF8": "1",
        "UV_NO_CONFIG": "1", "UV_PYTHON_DOWNLOADS": "never", "UV_CACHE_DIR": str(work / "uv-cache"),
        "UV_NO_PROGRESS": "1", "NO_COLOR": "1",
    }
    if stage is not None:
        result["PDF2ZH_APP_DIR"] = str(stage)
    return result


def run_step(label: str, command: list[str], work: Path, environment: dict[str, str], timeout: int) -> Path:
    log = work / f"{label}.log"
    print(f"{label}...", flush=True)
    # The command is a list; no shell, user commands, interpolated secrets or profiles.
    with log.open("wb") as output:
        try:
            result = subprocess.run(command, cwd=work, env=environment, stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT, timeout=timeout, check=False)
        except subprocess.TimeoutExpired as exc:
            raise BuildError(f"{label} timed out; see {log}") from exc
    require(result.returncode == 0, f"{label} failed ({result.returncode}); see {log}")
    return log


def is_notice(path: PurePosixPath) -> bool:
    return any(part.lower().startswith(("license", "licence", "copying", "notice", "copyright", "authors")) for part in path.parts)


def verified_notice_bytes(source: Path, expected_sha256: str) -> bytes:
    data = plain_path(source).read_bytes()
    if hashlib.sha256(data).hexdigest() != expected_sha256:
        # Git on Windows may check out the verified upstream LF text as CRLF.
        # Accept only this exact newline transformation, never a new digest.
        data = data.replace(b"\r\n", b"\n")
    require(hashlib.sha256(data).hexdigest() == expected_sha256, "Supplemental license SHA256 mismatch")
    return data


def python_inventory(stage: Path, wheels: list[dict], engine: dict) -> tuple[list[dict], list[str]]:
    site = stage / "runtime/python/site-packages"
    expected = {wheel["name"]: wheel for wheel in wheels}
    installed = {}
    issues = []
    for distribution in importlib.metadata.distributions(path=[str(site)]):
        name = canonical_name(distribution.metadata["Name"])
        require(name not in installed and name in expected, "Duplicate or unexpected installed distribution")
        require(distribution.version == expected[name]["version"], f"Wrong installed version: {name}")
        record = {**expected[name], "license": distribution.metadata.get("License-Expression") or distribution.metadata.get("License") or engine[name]["license"], "license_files": [], "requires_dist": distribution.metadata.get_all("Requires-Dist") or []}
        for member in distribution.files or []:
            if not is_notice(PurePosixPath(str(member).replace("\\", "/"))):
                continue
            source = plain_path(Path(distribution.locate_file(member)))
            require(source.is_relative_to(site), "Distribution license path escapes site-packages")
            if source.is_file():
                relative = source.relative_to(site)
                destination = stage / "licenses/python" / name / relative
                if not destination.exists():
                    checked_copy(source, destination)
                record["license_files"].append(destination.relative_to(stage).as_posix())
        installed[name] = record
    require(set(installed) == set(expected), "Installed distribution closure differs from the 85-package lock")
    supplement = REPO / "resources/licenses/python/ADDITIONAL_LICENSES.json"
    for notice in read_json(supplement):
        for package in notice["packages"]:
            name = canonical_name(package)
            if name in installed:
                source = supplement.parent / str(relative_member(notice["file"]))
                data = verified_notice_bytes(source, notice["sha256"])
                destination = stage / "licenses/python" / name / source.name
                plain_path(destination)
                destination.parent.mkdir(parents=True, exist_ok=True)
                with destination.open("xb") as stream:
                    stream.write(data)
                installed[name]["license_files"].append(destination.relative_to(stage).as_posix())
                installed[name]["supplemental_license_source"] = notice["source"]
                installed[name]["supplemental_license_sha256"] = notice["sha256"]
    for name, record in installed.items():
        record["license_files"] = sorted(set(record["license_files"]))
        if not record["license_files"]:
            issues.append(f"No collected license/notice text for Python distribution {name}")
    return [installed[name] for name in sorted(installed)], issues


def exclude_generated_launcher(path: Path, python: Path, entry_value: str) -> bool:
    """Remove only a proved installer wrapper with an absolute work interpreter.

    Native helpers (including packaged CLI binaries) are never blanket-pruned.
    The original wheel RECORD is retained; exclusions are recorded separately.
    """
    body = path.read_bytes()
    needles = {str(python), python.as_posix()}
    has_interpreter = any(needle.encode(encoding) in body for needle in needles for encoding in ("utf-8", "utf-16-le"))
    if not has_interpreter:
        return False
    module, separator, function = entry_value.partition(":")
    function = function.split("[", 1)[0].strip()
    proven = False
    if separator and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.]*", module) and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", function) and zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as archive:
            if archive.namelist() == ["__main__.py"]:
                main = archive.read("__main__.py").decode("utf-8")
                proven = f"from {module} import {function}" in main
    require(proven, "Unclassified entrypoint embeds the installation interpreter; review before publishing")
    plain_path(path)
    path.unlink()
    return True


def generated_entrypoints(stage: Path) -> list[dict]:
    site = stage / "runtime/python/site-packages"
    records = []
    for distribution in importlib.metadata.distributions(path=[str(site)]):
        for entry in distribution.entry_points:
            if entry.group not in {"console_scripts", "gui_scripts"}:
                continue
            record = {"distribution": canonical_name(distribution.metadata["Name"]), "entrypoint": entry.name, "group": entry.group, "value": entry.value, "wrappers": []}
            for member in distribution.files or []:
                member_name = Path(str(member)).name.lower()
                if member_name not in {entry.name.lower() + ".exe", entry.name.lower() + "-script.py", entry.name.lower()}:
                    continue
                path = plain_path(Path(distribution.locate_file(member)))
                require(path.is_relative_to(stage), "Entrypoint path escapes the new stage")
                if path.is_file():
                    before = digest(path)
                    removed = exclude_generated_launcher(path, stage / "runtime/python/python.exe", entry.value)
                    record["wrappers"].append({"path": path.relative_to(stage).as_posix(), "installed_sha256": before, "action": "excluded-generated-absolute-wrapper" if removed else "retained-native-or-no-install-path"})
            records.append(record)
    return sorted(records, key=lambda record: (record["distribution"], record["entrypoint"]))


def rust_archive_notices(stage: Path, archive_path: Path, package: dict, record: dict) -> None:
    """Read notices from a checksum-verified .crate without extracting source."""
    with plain_path(archive_path).open("rb") as source:
        require(hashlib.file_digest(source, "sha256").hexdigest() == package["checksum"], "Cargo archive checksum mismatch")
        source.seek(0)
        with tarfile.open(fileobj=source, mode="r:gz") as archive:
            prefix = f"{package['name']}-{package['version']}"
            manifest_file = archive.extractfile(prefix + "/Cargo.toml")
            require(manifest_file is not None, "Cargo archive has no package manifest")
            manifest = tomllib.loads(manifest_file.read().decode("utf-8"))["package"]
            require(manifest["name"] == package["name"] and manifest["version"] == package["version"], "Cargo archive package identity differs from lock")
            record["license"] = manifest.get("license")
            record["repository"] = manifest.get("repository")
            record["notice_provenance"] = "Cargo.lock SHA256 verified crate archive"
            for member in archive.getmembers():
                path = relative_member(member.name)
                require(path.parts[0] == prefix, "Cargo archive member escaped the package root")
                relative = PurePosixPath(*path.parts[1:])
                if not is_notice(relative) and relative.as_posix() != manifest.get("license-file"):
                    continue
                if member.isdir():
                    continue
                require(member.isfile() and member.size <= 8 * 1024 * 1024, "Invalid Cargo license archive member")
                contents = archive.extractfile(member)
                require(contents is not None, "Cargo archive license is unavailable")
                destination = plain_path(stage / "licenses/rust" / prefix / relative)
                destination.parent.mkdir(parents=True, exist_ok=True)
                with destination.open("xb") as output:
                    output.write(contents.read())
                record["license_files"].append(destination.relative_to(stage).as_posix())


def rust_inventory(stage: Path, registry: Path | None) -> tuple[list[dict], list[str]]:
    """Keep Cargo.lock checksums and cached license texts; do not invoke Cargo."""
    packages = tomllib.loads((stage / "provenance/Cargo.lock").read_text(encoding="utf-8"))["package"]
    records = []
    if registry is not None:
        plain_path(registry)
        require(registry.is_dir(), "Explicit Cargo registry source directory is missing")
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        record = {key: package[key] for key in ("name", "version", "source", "checksum") if key in package}
        record["license_files"] = []
        source = package.get("source", "")
        if not source:
            record["license"] = "See licenses/project/LICENSE and workspace manifests"
        elif source.startswith("registry+") and registry is not None:
            require(re.fullmatch(r"[a-zA-Z0-9_-]+", package["name"]) is not None and re.fullmatch(r"[a-zA-Z0-9.+_-]+", package["version"]) is not None, "Unsafe Cargo package name")
            matches = list(registry.glob(f"*/{package['name']}-{package['version']}"))
            if len(matches) == 1:
                root = plain_path(matches[0])
                archive_path = registry.parent / "cache" / root.parent.name / f"{package['name']}-{package['version']}.crate"
                if archive_path.is_file():
                    rust_archive_notices(stage, archive_path, package, record)
                elif (root / ".cargo-checksum.json").is_file():
                    checksums = read_json(root / ".cargo-checksum.json")
                    require(checksums.get("package") == package["checksum"], "Cargo source cache checksum provenance mismatch")
                    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]
                    record["license"] = manifest.get("license")
                    record["repository"] = manifest.get("repository")
                    for name, sha256 in sorted(checksums["files"].items()):
                        member = relative_member(name)
                        if not is_notice(member) and name != manifest.get("license-file"):
                            continue
                        origin = plain_path(root / member)
                        require(digest(origin) == sha256, "Cargo cached license text checksum mismatch")
                        destination = stage / "licenses/rust" / f"{package['name']}-{package['version']}" / member
                        checked_copy(origin, destination)
                        record["license_files"].append(destination.relative_to(stage).as_posix())
        records.append(record)
    supplement_rust_notices(stage, records)
    issues = [f"Rust license text not collected: {item['name']} {item['version']}" for item in records if item.get("source") and not item["license_files"]]
    return records, issues


def supplement_rust_notices(stage: Path, records: list[dict]) -> None:
    """Retain reviewed upstream workspace licenses missing from crate archives."""
    root = REPO / "resources/licenses/rust"
    manifest = root / "SUPPLEMENTAL_NOTICES.json"
    if not manifest.exists():
        return
    checked_copy(manifest, stage / "provenance/SUPPLEMENTAL_RUST_NOTICES.json")
    by_identity = {(item["name"], item["version"]): item for item in records}
    for notice in read_json(manifest):
        component = by_identity.get((notice["name"], notice["version"]))
        if component is None or component["license_files"]:
            continue
        source = root / relative_member(notice["file"])
        data = verified_notice_bytes(source, notice["sha256"])
        destination = stage / "licenses/rust" / f"{component['name']}-{component['version']}" / ("upstream-" + source.name)
        plain_path(destination)
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open("xb") as stream:
            stream.write(data)
        component["license_files"].append(destination.relative_to(stage).as_posix())
        component["supplemental_notice_source"] = {key: value for key, value in notice.items() if key not in {"name", "version", "file"}}


def validate_output(output: Path, inputs: list[Path]) -> Path:
    output = plain_path(output)
    require(output.is_relative_to(REPO) and output != REPO, "Output must be a new directory below the repository")
    require(not output.exists(), f"Output already exists; it will not be replaced: {output}")
    forbidden = [REPO / name for name in (".git", ".venv", ".cargo", "crates", "python-worker", "script", "docs", "target", "build-inputs", "pdf2zh_next", "dist/release")]
    for root in forbidden + inputs:
        root = plain_path(root)
        require(not output.is_relative_to(root) and not root.is_relative_to(output), f"Output overlaps a protected/input path: {root}")
    return output


def input_paths(args: argparse.Namespace) -> list[Path]:
    return [getattr(args, name) for name in ("runtime_source", "runtime_manifest", "pdfium_source", "assets_zip", "bin_dir", "asset_notices", "cargo_registry_src", "wheel_cache", "uv") if getattr(args, name) is not None]


def preflight(args: argparse.Namespace, wheels: list[dict], expected_lock: str) -> dict:
    require(REQUIREMENTS.read_text(encoding="utf-8") == expected_lock, "Worker lock is stale; review and run --write-lock")
    runtime = plain_path(args.runtime_source)
    runtime_manifest = plain_path(args.runtime_manifest)
    pdfium = plain_path(args.pdfium_source)
    assets = plain_path(args.assets_zip)
    bins = plain_path(args.bin_dir)
    inputs = input_paths(args)
    output = validate_output(args.output, inputs)
    records = runtime_files(runtime, runtime_manifest)
    for name in ("pdf2zh.exe", "pdf2zh-gui.exe"):
        require((bins / name).is_file(), f"Build the Rust release executable first: {name}")
        pe_imports(plain_path(bins / name))
    verify_file(pdfium / "bin/pdfium.dll", PDFIUM_BYTES, PDFIUM_SHA256)
    pe_imports(pdfium / "bin/pdfium.dll")
    version = dict(line.split("=", 1) for line in (pdfium / "VERSION").read_text().splitlines() if line)
    require(".".join(version[key] for key in ("MAJOR", "MINOR", "BUILD", "PATCH")) == PDFIUM_VERSION, "Wrong PDFium version")
    args_gn = (pdfium / "args.gn").read_text(encoding="utf-8")
    require(all(re.search(rf"(?m)^\s*{key}\s*=\s*{value}\s*$", args_gn) for key, value in (("target_cpu", '"x64"'), ("target_os", '"win"'), ("pdf_enable_v8", "false"), ("pdf_enable_xfa", "false"))), "Unexpected PDFium build flags")
    require((pdfium / "LICENSE").is_file() and bool(files_below(pdfium / "licenses")), "PDFium notices are missing")
    inventory = read_json(ASSET_INVENTORY)
    expected_assets = asset_members(inventory)
    verify_zip(assets, expected_assets)
    uv = plain_path(args.uv or shutil.which("uv") or "")
    require(uv.is_file() and uv.name.lower().endswith(".exe"), "Existing uv.exe is required; pass --uv explicitly")
    if args.asset_notices:
        require(bool(files_below(plain_path(args.asset_notices))), "Explicit asset notice directory is empty")
    if args.wheel_cache:
        require(bool(files_below(plain_path(args.wheel_cache))), "Explicit wheel cache directory is empty")
    return {"output": output, "runtime": records, "assets": expected_assets, "uv": uv, "wheels": wheels, "inputs": inputs + [uv]}


def final_manifest(stage: Path) -> list[dict]:
    if (stage / "config").exists():
        templates = files_below(stage / "config")
        require([path.relative_to(stage / "config").as_posix() for path in templates] == ["config.example.toml"], "Build unexpectedly created application config")
        template = templates[0]
        require(digest(template) == digest(REPO / "resources/config.example.toml"), "Unverified configuration template")
        def empty_credentials(value):
            if isinstance(value, dict):
                for key, item in value.items():
                    if any(word in key.lower() for word in ("api_key", "apikey", "auth_key", "secret_key", "secret_id", "password", "token")):
                        require(item in ("", None), "Configuration template contains credentials")
                    else:
                        empty_credentials(item)
        empty_credentials(tomllib.loads(template.read_text(encoding="utf-8")))
    require(not (stage / "site-packages").exists(), "Unexpected legacy site-packages layout")
    records = []
    for path in files_below(stage):
        name = path.name.lower()
        require(name not in {".env", "credentials.json", "secrets.json", "_pystand_static.int", "config.v3.toml"}, "Local state/legacy launcher in stage")
        require("pdf2zh_next" not in path.relative_to(stage).parts and "gradio" not in path.relative_to(stage).parts, "Legacy application package in stage")
        if path != stage / "RELEASE_FILES.json":
            records.append({"path": path.relative_to(stage).as_posix(), "bytes": path.stat().st_size, "sha256": digest(path)})
    write_json(stage / "RELEASE_FILES.json", records)
    return records


def remove_generated_bytecode(stage: Path) -> int:
    """Remove only new-stage cache files, whose co_filename embeds work paths."""
    count = 0
    for path in files_below(stage):
        if "__pycache__" in path.relative_to(stage).parts and path.suffix.lower() in {".pyc", ".pyo"}:
            require(path.is_relative_to(stage), "Bytecode cleanup escaped stage")
            path.unlink()
            count += 1
    for directory, _, _ in os.walk(stage, topdown=False, followlinks=False):
        path = plain_path(Path(directory))
        if path.name == "__pycache__" and not any(path.iterdir()):
            require(path.is_relative_to(stage), "Bytecode cleanup escaped stage")
            path.rmdir()
    return count


def remove_empty_engine_cache(stage: Path) -> list[dict]:
    """Remove only the known empty DB created by the isolated import smoke."""
    stage = plain_path(stage)
    cache = plain_path(stage / "resources/worker-home/.cache/babeldoc")
    require(cache.is_relative_to(stage) and stage.is_relative_to(REPO), "Engine cache cleanup escaped the generated stage")
    database = cache / "cache.v1.db"
    if not database.exists():
        return []
    plain_path(database)
    require(database.is_file(), "Unexpected engine cache database type")
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as connection:
        connection.execute("PRAGMA trusted_schema=OFF")
        tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        require(tables == {"_translationcache"}, "Unexpected engine cache schema; retained for review")
        require(connection.execute("SELECT COUNT(*) FROM _translationcache").fetchone()[0] == 0, "Nonempty engine cache must not be removed or published")
    excluded = []
    for name in ("cache.v1.db", "cache.v1.db-wal", "cache.v1.db-shm", "cache.v1.db-journal"):
        path = plain_path(cache / name)
        require(path.is_relative_to(cache), "Engine cache file escaped the generated cache")
        if path.exists():
            require(path.is_file(), "Unexpected engine cache sidecar")
            excluded.append({"path": path.relative_to(stage).as_posix(), "bytes": path.stat().st_size, "sha256": digest(path)})
            path.unlink()
    return excluded


PROBE = r'''
import importlib, importlib.metadata, json, pathlib, struct, sys
from packaging.markers import default_environment
from packaging.requirements import Requirement
from packaging.specifiers import SpecifierSet
from packaging.utils import canonicalize_name
root = pathlib.Path(sys.executable).parent
provenance = root.parents[1] / "provenance"
assert sys.version_info[:3] == (3, 13, 16) and struct.calcsize("P") == 8
assert sys.flags.isolated and sys.flags.ignore_environment and sys.flags.no_user_site
assert not any(".venv" in pathlib.Path(p).parts for p in sys.path)
assert all(pathlib.Path(p).is_relative_to(root) for p in sys.path)
expected = {w["name"]: w["version"] for w in json.loads((provenance / "WHEELS.json").read_text())}
installed = {canonicalize_name(d.metadata["Name"]): d for d in importlib.metadata.distributions()}
assert {n: d.version for n, d in installed.items()} == expected
environment = default_environment()
for distribution in installed.values():
    if requirement := distribution.metadata.get("Requires-Python"):
        assert SpecifierSet(requirement).contains(environment["python_full_version"], prereleases=True)
# Recompute the active engine closure and extras from installed metadata. --no-deps
# installation must not hide an override's newly added or incompatible dependency.
active = {"babeldoc": {""}}
pending = ["babeldoc"]
while pending:
    name = pending.pop()
    for text in installed[name].requires or []:
        requirement = Requirement(text)
        if requirement.marker and not any(requirement.marker.evaluate({**environment, "extra": extra}) for extra in active[name]):
            continue
        dependency = canonicalize_name(requirement.name)
        assert dependency in installed, f"Missing dependency of {name}: {dependency}"
        assert requirement.specifier.contains(installed[dependency].version, prereleases=True), f"Incompatible dependency of {name}: {dependency}"
        extras = {"", *requirement.extras}
        if dependency not in active or not extras.issubset(active[dependency]):
            active.setdefault(dependency, set()).update(extras)
            pending.append(dependency)
assert set(active) == set(expected), "Locked distributions differ from active BabelDOC dependency closure"
for name in ("babeldoc", "babeldoc.format.pdf.high_level", "pymupdf", "onnxruntime",
             "numpy", "scipy", "cv2", "rtree", "hyperscan", "uharfbuzz", "freetype", "cryptography"):
    importlib.import_module(name)
assert importlib.metadata.version("babeldoc") == "0.6.3"
assert not {"gradio", "pdf2zh-next"} & {d.metadata["Name"].lower() for d in importlib.metadata.distributions()}
print(json.dumps({"python": sys.version.split()[0], "pillow": installed["pillow"].version, "distributions": len(installed), "isolated": True, "metadata_constraints_and_closure": "passed", "native_imports": "passed"}))
'''


def staged_inputs(stage: Path, wheels: list[dict]) -> dict:
    """Use one captured dependency snapshot throughout a long installation."""
    provenance = stage / "provenance"
    inventory_path = provenance / INVENTORY.name
    baseline_path = provenance / BASELINE_WHEELS.name
    overrides_path = provenance / WHEEL_OVERRIDES.name
    inventory = read_json(inventory_path)
    selected = select_wheels(inventory, read_json(baseline_path))
    selected = apply_wheel_overrides(selected, read_json(overrides_path))
    require(selected == wheels, "Source dependency files changed after preflight")
    expected = lock_text(selected, source_text_digest(inventory_path), source_text_digest(baseline_path), source_text_digest(overrides_path))
    require((provenance / REQUIREMENTS.name).read_text(encoding="utf-8") == expected, "Captured worker lock differs from captured dependency inputs")
    return inventory


def build(args: argparse.Namespace, plan: dict) -> Path:
    require(os.name == "nt" and struct.calcsize("P") == 8, "Bundle execution requires a Windows x64 build host")
    output = plan["output"]
    output.parent.mkdir(parents=True, exist_ok=True)
    # Keep wheel-owned paths below the legacy Windows path limit without
    # changing the machine's long-path policy or discarding package data.
    work = validate_output(output.parent / f".w-{uuid.uuid4().hex}", plan["inputs"])
    work.mkdir()
    stage = work / "stage"
    stage.mkdir()
    print(f"Build work directory (preserved on failure): {work}", flush=True)
    environment = sanitized_environment(work, stage=stage)
    if args.wheel_cache:
        # Never let uv modify an existing cache; use only a private copy and
        # make offline mode mandatory when the caller supplies one.
        shutil.copytree(args.wheel_cache, work / "uv-cache")
        environment["UV_OFFLINE"] = "1"
    try:
        runtime = stage / "runtime/python"
        for record in plan["runtime"]:
            checked_copy(args.runtime_source / record["name"], runtime / record["name"], (record["bytes"], record["sha256"]))
        # Only this verified copy is changed. The original runtime is never written.
        (runtime / "python313._pth").write_text(PTH_CONTENT, encoding="ascii", newline="\n")
        for name in ("pdf2zh.exe", "pdf2zh-gui.exe"):
            origin = args.bin_dir / name
            checked_copy(origin, stage / name, (origin.stat().st_size, digest(origin)))
        checked_copy(REPO / "python-worker/worker.py", stage / "babeldoc/worker.py")
        checked_copy(REPO / "resources/windows-README.md", stage / "README_zh_TW.md")
        checked_copy(REPO / "resources/config.example.toml", stage / "config/config.example.toml")
        checked_copy(args.pdfium_source / "bin/pdfium.dll", stage / "native/pdfium/pdfium.dll", (PDFIUM_BYTES, PDFIUM_SHA256))
        for origin in [args.pdfium_source / "LICENSE", args.pdfium_source / "VERSION", args.pdfium_source / "args.gn", *files_below(args.pdfium_source / "licenses")]:
            checked_copy(origin, stage / "licenses/pdfium" / origin.relative_to(args.pdfium_source))
        checked_copy(REPO / "LICENSE", stage / "licenses/project/LICENSE")
        checked_copy(args.runtime_source / "LICENSE.txt", stage / "licenses/python-runtime/LICENSE.txt", RUNTIME_PINS["LICENSE.txt"])
        provenance = stage / "provenance"
        for origin in (REQUIREMENTS, WHEEL_OVERRIDES, BASELINE_WHEELS, INVENTORY, ASSET_INVENTORY, REPO / "Cargo.lock", REPO / "Cargo.toml", Path(__file__), REPO / "docs/DEVELOPMENT.md"):
            checked_copy(origin, provenance / origin.name, (origin.stat().st_size, digest(origin)))
        captured_inventory = staged_inputs(stage, plan["wheels"])
        write_json(provenance / "RUNTIME_SOURCE_FILES.json", plan["runtime"])
        write_json(provenance / "WHEELS.json", plan["wheels"])
        archive = stage / "engine-assets" / f"offline_assets_{ASSET_TAG}.zip"
        checked_copy(args.assets_zip, archive, (ASSET_BYTES, ASSET_SHA256))
        if args.asset_notices:
            for origin in files_below(args.asset_notices):
                checked_copy(origin, stage / "licenses/engine-assets" / origin.relative_to(args.asset_notices))
        run_step("uv-version", [str(plan["uv"]), "--version"], work, environment, 30)
        python = runtime / "python.exe"
        site = runtime / "site-packages"
        run_step("install-wheels", [str(plan["uv"]), "--no-config", "pip", "install", "--python", str(python), "--target", str(site), "--no-deps", "--no-index", "--require-hashes", "--only-binary", ":all:", "--link-mode", "copy", "-r", str(provenance / REQUIREMENTS.name)], work, environment, args.install_timeout)
        # No blanket 'import site': locked engine wheels currently need no .pth
        # execution. A future wheel introducing hooks needs an explicit review.
        require(not any(p.suffix.lower() == ".pth" for p in files_below(site)), "Wheel added a .pth hook; review it before enabling site initialization")
        entrypoints = generated_entrypoints(stage)
        python_components, license_issues = python_inventory(stage, plan["wheels"], captured_inventory["engine"])
        rust_components, rust_issues = rust_inventory(stage, args.cargo_registry_src)
        license_issues += rust_issues
        if not args.asset_notices:
            license_issues.append("Asset-specific font/model/CMap/tokenizer notice texts have not been supplied; hashes are not a redistribution license")
        # An installed absolute work path must not be available to the smoke.
        # This exercises the shipped application after moving the complete tree.
        relocated = work / "relocation-smoke"
        require(not relocated.exists(), "Relocation smoke directory already exists")
        rename_with_transient_retry(stage, relocated)
        stage = relocated
        runtime = stage / "runtime/python"
        python = runtime / "python.exe"
        provenance = stage / "provenance"
        archive = stage / "engine-assets" / f"offline_assets_{ASSET_TAG}.zip"
        environment["PDF2ZH_APP_DIR"] = str(stage)
        natives, missing = native_inventory(stage)
        write_json(stage / "NATIVE_DEPENDENCIES.json", {"scope": "PE normal/delay imports; bundled-candidate does not prove loader resolution; dynamic LoadLibrary is not covered", "files": natives, "unresolved": missing})
        require(not missing or args.allow_unresolved_native, f"Unresolved native imports; see {stage / 'NATIVE_DEPENDENCIES.json'}. Do not copy System32 DLLs; obtain a licensed redistributable source or explicitly build a diagnostic candidate with --allow-unresolved-native")
        home = stage / "resources/worker-home"
        home.mkdir(parents=True)
        # Exercise the shipped bridge/worker boundary and its default layout;
        # no provider service, API credentials or network warmup is requested.
        run_step("restore-assets", [str(stage / "pdf2zh.exe"), "--restore-offline-assets", str(archive), "--worker-home", str(home), "--json"], work, environment, args.smoke_timeout)
        verify_restored(home, plan["assets"])
        probe_environment = {**environment, "HOME": str(home), "USERPROFILE": str(home)}
        run_step("embedded-import-smoke", [str(python), "-I", "-B", "-c", PROBE], work, probe_environment, args.smoke_timeout)
        run_step("cli-version", [str(stage / "pdf2zh.exe"), "--version"], work, environment, 30)
        run_step("gui-version", [str(stage / "pdf2zh-gui.exe"), "--version"], work, environment, 30)
        removed_bytecode = remove_generated_bytecode(stage)
        empty_engine_cache = remove_empty_engine_cache(stage)
        verify_restored(home, plan["assets"])
        seed_manifest = stage / "resources/engine-assets.json"
        write_json(seed_manifest, engine_seed_manifest(home, plan["assets"]))
        write_json(stage / "ARTIFACT_EXCLUSIONS.json", {"entrypoints": entrypoints, "empty_generated_engine_cache": empty_engine_cache, "removed_generated_bytecode_files": removed_bytecode, "scope": "Application entrypoints are the two Rust EXEs. Only verified generated wrappers, bytecode and the smoke's empty known cache are excluded; original wheel RECORD metadata is retained."})
        write_json(stage / "THIRD_PARTY_COMPONENTS.json", {"python": python_components, "rust_lock_all_targets": rust_components, "pdfium": {"version": PDFIUM_VERSION, "publisher_archive_url": PDFIUM_URL, "publisher_archive_sha256": PDFIUM_ARCHIVE_SHA256, "dll_sha256": PDFIUM_SHA256, "verification": "DLL digest, version/build flags, notices copied; archive/signature/ABI validation is external", "licenses": "licenses/pdfium"}, "python_runtime": {"version": PYTHON_VERSION, "source": "verified embedded-runtime build input; see provenance/RUNTIME_SOURCE_FILES.json", "license": "licenses/python-runtime/LICENSE.txt"}, "engine_assets": {"inventory": "provenance/engine-assets.lock.json", "license_texts_supplied": bool(args.asset_notices)}, "license_review_items": license_issues})
        build_info = {
            "schema_version": 1, "artifact_kind": "hybrid-windows-x64-candidate", "release_ready": False,
            "rust_built_by_builder": False, "source_commit": args.source_commit,
            "runtime_python": PYTHON_VERSION, "babeldoc": "0.6.3", "python_distribution_count": ENGINE_COUNT,
            "pdfium": PDFIUM_VERSION, "pdfium_dll_sha256": PDFIUM_SHA256,
            "assets_tag": ASSET_TAG, "assets_archive_sha256": ASSET_SHA256, "assets_verified_files": len(plan["assets"]),
            "engine_seed_manifest_sha256": digest(seed_manifest),
            "offline_assets_restored_and_sha3_verified": True, "editable": False, "directml": False,
            "pth": PTH_CONTENT.splitlines(), "environment": "allowlisted; no user config/API credentials/proxy/index environment",
            "inputs": {"baseline_wheels_sha256": digest(provenance / BASELINE_WHEELS.name), "cargo_lock_sha256": digest(provenance / "Cargo.lock"), "worker_lock_sha256": digest(provenance / REQUIREMENTS.name), "wheel_overrides_sha256": digest(provenance / WHEEL_OVERRIDES.name), "builder_sha256": digest(provenance / Path(__file__).name), "runtime_source_manifest_sha256": digest(args.runtime_manifest), "worker_sha256": digest(stage / "babeldoc/worker.py")},
            "uv_version": (work / "uv-version.log").read_text(encoding="utf-8").strip(),
            "checks": {"embedded_native_imports_after_relocation": "passed", "cli_version": "passed", "gui_version": "passed", "native_unresolved_count": len(missing)},
            "removed_generated_bytecode_files": removed_bytecode,
            "release_review_items": ["No clean Windows VM/Sandbox validation by this builder", "No GUI render, PDF translation, cancellation or read-only-install smoke performed by this builder", "Static native imports do not cover dynamic loading or guarantee per-module DLL search", "Corresponding sources/source offers and redistribution terms need release review", "Binary reproducibility of supplied Rust EXEs is outside this builder"] + license_issues + (["Unresolved native imports explicitly allowed for a diagnostic candidate"] if missing else []),
        }
        write_json(stage / "BUILD_INFO.json", build_info)
        records = final_manifest(stage)
        write_json(work / "BUILD_LOGS.json", {"logs": [p.name for p in sorted(work.glob("*.log"))], "output": str(output)})
        require(not output.exists(), "Output appeared during build; refusing replacement")
        plain_path(output.parent)
        # On Windows rename fails if the destination exists. Same-volume rename
        # publishes the complete stage, with no recursive move or delete.
        rename_with_transient_retry(stage, output)
        print(json.dumps({"output": str(output), "files": len(records), "bytes": sum(r["bytes"] for r in records), "manifest_sha256": digest(output / "RELEASE_FILES.json"), "release_ready": False, "logs": str(work)}, indent=2))
        return output
    except BaseException:
        print(f"Build not published. Work directory retained: {work}", file=sys.stderr)
        raise


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--plan", action="store_true", help="Verify inputs and show the plan without building/installing")
    action.add_argument("--write-lock", action="store_true", help="Regenerate worker requirements from the captured baseline and reviewed replacements")
    action.add_argument("--self-test", action="store_true", help="Run fast synthetic tests without network, packages, Cargo or real binaries")
    parser.add_argument("--output", type=Path, help="New output directory below the repo; must not already exist")
    parser.add_argument("--bin-dir", type=Path, default=REPO / "target/release")
    parser.add_argument("--runtime-source", type=Path, default=BUILD_INPUTS / "runtime")
    parser.add_argument("--runtime-manifest", type=Path, default=BUILD_INPUTS / "runtime-files.json")
    parser.add_argument("--pdfium-source", type=Path, default=BUILD_INPUTS / "pdfium")
    parser.add_argument("--assets-zip", type=Path, default=BUILD_INPUTS / f"offline_assets_{ASSET_TAG}.zip")
    parser.add_argument("--uv", type=Path, help="Existing uv.exe (resolved once before the environment is isolated)")
    parser.add_argument("--cargo-registry-src", type=Path, help="Explicit read-only Cargo registry/src directory for third-party notices")
    parser.add_argument("--asset-notices", type=Path, default=REPO / "resources/licenses/engine-assets", help="Reviewed font/model/CMap/tokenizer notices directory")
    wheel_source = parser.add_mutually_exclusive_group()
    wheel_source.add_argument("--wheel-cache", type=Path, default=BUILD_INPUTS / "uv-cache", help="Copy an existing uv cache into the private build directory and install offline")
    wheel_source.add_argument("--online", dest="wheel_cache", action="store_const", const=None, help="Download pinned wheels to a new private cache instead of using the offline cache")
    parser.add_argument("--source-commit", help="Caller-supplied source revision for the precompiled EXEs; not inferred from binary hashes")
    parser.add_argument("--allow-unresolved-native", action="store_true", help="Allow a clearly marked diagnostic candidate with unresolved native imports")
    parser.add_argument("--install-timeout", type=int, default=1800)
    parser.add_argument("--smoke-timeout", type=int, default=600)
    args = parser.parse_args(argv)
    if not (args.write_lock or args.self_test) and args.output is None:
        parser.error("--output is required for --plan or building")
    if args.install_timeout <= 0 or args.smoke_timeout <= 0:
        parser.error("Timeouts must be positive")
    for name in ("output", "bin_dir", "runtime_source", "runtime_manifest", "pdfium_source", "assets_zip", "uv", "cargo_registry_src", "asset_notices", "wheel_cache"):
        value = getattr(args, name)
        if value is not None:
            setattr(args, name, plain_path(value))
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.self_test:
        result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(BuilderTests))
        return 0 if result.wasSuccessful() else 1
    inventory = read_json(INVENTORY)
    wheels = select_wheels(inventory, read_json(BASELINE_WHEELS))
    wheels = apply_wheel_overrides(wheels, read_json(WHEEL_OVERRIDES))
    expected = lock_text(wheels, source_text_digest(INVENTORY), source_text_digest(BASELINE_WHEELS), source_text_digest(WHEEL_OVERRIDES))
    if args.write_lock:
        write_lock_file(REQUIREMENTS, expected)
        print(f"Wrote {len(wheels)} exact wheel requirements: {REQUIREMENTS}")
        return 0
    plan = preflight(args, wheels, expected)
    if args.plan:
        print(json.dumps({"output": str(plan["output"]), "build_cargo": False, "python": PYTHON_VERSION, "runtime_files": len(plan["runtime"]), "python_distributions": len(wheels), "downloaded_wheel_bytes": sum(w["bytes"] for w in wheels), "pdfium": PDFIUM_VERSION, "assets": len(plan["assets"]), "assets_archive_sha256": ASSET_SHA256, "uv": str(plan["uv"]), "release_ready": False}, indent=2))
    else:
        build(args, plan)
    return 0


class BuilderTests(unittest.TestCase):
    """Synthetic boundary tests, deliberately independent of the user's setup."""

    def test_rename_retries_transient_windows_permission_then_succeeds(self):
        from unittest.mock import patch

        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            source, destination = root / "stage", root / "relocated"
            source.mkdir()
            (source / "marker").write_text("preserved", encoding="utf-8")
            original = Path.rename
            error = PermissionError("synthetic sharing violation")
            error.winerror = 32
            calls = []

            def transient_once(path, target):
                calls.append((path, target))
                if len(calls) == 1:
                    raise error
                return original(path, target)

            with patch.object(Path, "rename", autospec=True, side_effect=transient_once), patch.object(time, "sleep") as sleep:
                self.assertEqual(rename_with_transient_retry(source, destination), destination)
            self.assertEqual(len(calls), 2)
            sleep.assert_called_once_with(0.25)
            self.assertFalse(source.exists())
            self.assertEqual((destination / "marker").read_text(encoding="utf-8"), "preserved")

    def test_rename_persistent_permission_preserves_source_and_old_output(self):
        from unittest.mock import patch

        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            source, destination, old_output = root / "stage", root / "relocated", root / "final"
            source.mkdir()
            old_output.mkdir()
            (source / "marker").write_text("new", encoding="utf-8")
            (old_output / "marker").write_text("old", encoding="utf-8")
            error = PermissionError("synthetic access denied")
            error.winerror = 5
            with patch.object(Path, "rename", autospec=True, side_effect=error) as rename, patch.object(time, "sleep") as sleep, patch.object(time, "monotonic", return_value=0):
                with self.assertRaises(PermissionError):
                    rename_with_transient_retry(source, destination)
            self.assertEqual(rename.call_count, 33)
            self.assertEqual(sleep.call_count, 32)
            self.assertLessEqual(sum(call.args[0] for call in sleep.call_args_list), 8.0)
            self.assertEqual((source / "marker").read_text(encoding="utf-8"), "new")
            self.assertEqual((old_output / "marker").read_text(encoding="utf-8"), "old")
            self.assertFalse(destination.exists())

    def test_rename_does_not_retry_other_errors(self):
        from unittest.mock import patch

        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            source, destination = root / "stage", root / "relocated"
            source.mkdir()
            unrelated = PermissionError("synthetic unrelated permission error")
            unrelated.winerror = 87
            for error in (unrelated, PermissionError("no Windows error"), OSError("other failure")):
                with self.subTest(error=error), patch.object(Path, "rename", autospec=True, side_effect=error) as rename, patch.object(time, "sleep") as sleep:
                    with self.assertRaises(type(error)):
                        rename_with_transient_retry(source, destination)
                    rename.assert_called_once()
                    sleep.assert_not_called()
            self.assertTrue(source.is_dir())
            self.assertFalse(destination.exists())

    def test_rename_refuses_destination_created_during_retry(self):
        from unittest.mock import patch

        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            source, destination = root / "stage", root / "final"
            source.mkdir()
            (source / "marker").write_text("new", encoding="utf-8")
            error = PermissionError("synthetic access denied")
            error.winerror = 5

            def create_destination(_path, _target):
                destination.mkdir()
                (destination / "marker").write_text("old", encoding="utf-8")
                raise error

            with patch.object(Path, "rename", autospec=True, side_effect=create_destination) as rename, patch.object(time, "sleep"):
                with self.assertRaises(FileExistsError):
                    rename_with_transient_retry(source, destination)
            rename.assert_called_once()
            self.assertEqual((source / "marker").read_text(encoding="utf-8"), "new")
            self.assertEqual((destination / "marker").read_text(encoding="utf-8"), "old")

    def test_stable_build_inputs_default_to_offline_with_explicit_online_override(self):
        args = parse_args(["--plan", "--output", str(REPO / "dist" / "synthetic-plan")])
        self.assertEqual(args.runtime_source, BUILD_INPUTS / "runtime")
        self.assertEqual(args.runtime_manifest, BUILD_INPUTS / "runtime-files.json")
        self.assertEqual(args.pdfium_source, BUILD_INPUTS / "pdfium")
        self.assertEqual(args.wheel_cache, BUILD_INPUTS / "uv-cache")
        self.assertTrue(args.assets_zip.is_relative_to(BUILD_INPUTS))
        online = parse_args(["--plan", "--output", str(REPO / "dist" / "synthetic-plan"), "--online"])
        self.assertIsNone(online.wheel_cache)

    def test_captured_locks_are_stable_across_git_checkout_line_endings(self):
        inventory = read_json(INVENTORY)
        wheels = apply_wheel_overrides(select_wheels(inventory, read_json(BASELINE_WHEELS)), read_json(WHEEL_OVERRIDES))
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            stage = Path(temporary)
            provenance = stage / "provenance"
            provenance.mkdir()
            for newline in (b"\n", b"\r\n"):
                for origin in (INVENTORY, WHEEL_OVERRIDES, BASELINE_WHEELS):
                    content = origin.read_bytes().replace(b"\r\n", b"\n").replace(b"\n", newline)
                    (provenance / origin.name).write_bytes(content)
                shutil.copyfile(REQUIREMENTS, provenance / REQUIREMENTS.name)
                self.assertEqual(staged_inputs(stage, wheels), inventory)
            changed = read_json(provenance / WHEEL_OVERRIDES.name)
            changed["packages"]["pillow"]["reason"] += " changed"
            write_json(provenance / WHEEL_OVERRIDES.name, changed)
            with self.assertRaises(BuildError):
                staged_inputs(stage, wheels)

    def test_worker_security_profile_changes_only_the_reviewed_wheel(self):
        inventory = read_json(INVENTORY)
        baseline = select_wheels(inventory, read_json(BASELINE_WHEELS))
        effective = apply_wheel_overrides(baseline, read_json(WHEEL_OVERRIDES))
        self.assertEqual(len(effective), ENGINE_COUNT)
        self.assertEqual([old["name"] for old, new in zip(baseline, effective, strict=True) if old != new], ["pillow"])
        self.assertEqual(next(w["version"] for w in effective if w["name"] == "pillow"), "13.0.0")
        self.assertEqual(next(w["version"] for w in baseline if w["name"] == "pillow"), "12.3.0")

    def test_effective_wheels_match_v3_recorded_profile(self):
        # Captured from v3 provenance/WHEELS.json, canonicalized as below.
        wheels = apply_wheel_overrides(select_wheels(read_json(INVENTORY), read_json(BASELINE_WHEELS)), read_json(WHEEL_OVERRIDES))
        canonical = json.dumps(wheels, sort_keys=True, separators=(",", ":")).encode()
        self.assertEqual(hashlib.sha256(canonical).hexdigest(), "2afbc367ce4d3f0fbfd8f3b5f1da0e5d34ff9d3bb79aba761b83d045b64dca97")

    def test_baseline_profile_rejects_invalid_or_duplicate_wheels(self):
        inventory = read_json(INVENTORY)
        for mutation in ("duplicate", "url", "identity", "digest", "size", "version"):
            with self.subTest(mutation=mutation):
                profile = read_json(BASELINE_WHEELS)
                wheel = profile["wheels"][0]
                if mutation == "duplicate":
                    profile["wheels"][-1] = wheel.copy()
                elif mutation == "url":
                    wheel["url"] = wheel["url"].replace("files.pythonhosted.org", "example.invalid")
                elif mutation == "identity":
                    wheel["url"] = wheel["url"].replace("annotated_types-", "different-")
                elif mutation == "digest":
                    wheel["sha256"] = "invalid"
                elif mutation == "size":
                    wheel["bytes"] = True
                else:
                    wheel["version"] = "0.0.0"
                with self.assertRaises(BuildError):
                    select_wheels(inventory, profile)

    def test_replacement_cannot_add_a_package_or_use_a_stale_baseline(self):
        profile = read_json(WHEEL_OVERRIDES)
        baseline = [{"name": "pillow", "version": "12.2.0"}]
        with self.assertRaises(BuildError):
            apply_wheel_overrides(baseline, profile)
        with self.assertRaises(BuildError):
            apply_wheel_overrides([{"name": "different", "version": "12.3.0"}], profile)

    def test_replacement_rejects_wrong_platform_identity_and_untrusted_url(self):
        baseline = [{"name": "pillow", "version": "12.3.0"}]
        original = read_json(WHEEL_OVERRIDES)["packages"]["pillow"]["url"]
        for url in (original.replace("win_amd64", "manylinux_2_17_x86_64"), original.replace("pillow-13.0.0", "other-13.0.0"), original.replace("pillow-13.0.0", "pillow-12.3.0"), original.replace("files.pythonhosted.org", "example.invalid"), original.replace("https://", "https://secret@")):
            with self.subTest(url=url):
                profile = read_json(WHEEL_OVERRIDES)
                profile["packages"]["pillow"]["url"] = url
                with self.assertRaises(BuildError):
                    apply_wheel_overrides(baseline, profile)

    def test_replacement_needs_hash_size_reason_and_official_metadata(self):
        for field, value in (("sha256", "0" * 63), ("bytes", 0), ("bytes", True), ("reason", " "), ("metadata_source", "https://pypi.org/pypi/pillow/12.3.0/json")):
            with self.subTest(field=field, value=value):
                profile = read_json(WHEEL_OVERRIDES)
                profile["packages"]["pillow"][field] = value
                with self.assertRaises(BuildError):
                    apply_wheel_overrides([{"name": "pillow", "version": "12.3.0"}], profile)

    def test_captured_profile_change_cannot_silently_change_provenance(self):
        inventory = read_json(INVENTORY)
        wheels = apply_wheel_overrides(select_wheels(inventory, read_json(BASELINE_WHEELS)), read_json(WHEEL_OVERRIDES))
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            stage = Path(temporary)
            provenance = stage / "provenance"
            provenance.mkdir()
            for origin in (INVENTORY, REQUIREMENTS, WHEEL_OVERRIDES, BASELINE_WHEELS):
                shutil.copyfile(origin, provenance / origin.name)
            profile = read_json(provenance / WHEEL_OVERRIDES.name)
            profile["packages"]["pillow"]["reason"] = "Changed after preflight"
            write_json(provenance / WHEEL_OVERRIDES.name, profile)
            with self.assertRaises(BuildError):
                staged_inputs(stage, wheels)
            shutil.copyfile(WHEEL_OVERRIDES, provenance / WHEEL_OVERRIDES.name)
            baseline = read_json(provenance / BASELINE_WHEELS.name)
            baseline["wheels"][0]["bytes"] += 1
            write_json(provenance / BASELINE_WHEELS.name, baseline)
            with self.assertRaises(BuildError):
                staged_inputs(stage, wheels)

    def test_only_the_known_empty_generated_engine_cache_is_removed(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            stage = Path(temporary)
            cache = stage / "resources/worker-home/.cache/babeldoc"
            cache.mkdir(parents=True)
            (cache / "font.ttf").write_bytes(b"must keep asset")
            database = cache / "cache.v1.db"
            with closing(sqlite3.connect(database)) as connection:
                connection.execute("CREATE TABLE _translationcache (translation TEXT)")
                connection.commit()
            removed = remove_empty_engine_cache(stage)
            self.assertEqual([item["path"] for item in removed], ["resources/worker-home/.cache/babeldoc/cache.v1.db"])
            self.assertFalse(database.exists())
            self.assertEqual((cache / "font.ttf").read_bytes(), b"must keep asset")

    def test_nonempty_generated_engine_cache_is_preserved_and_stops_publication(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            stage = Path(temporary)
            cache = stage / "resources/worker-home/.cache/babeldoc"
            cache.mkdir(parents=True)
            database = cache / "cache.v1.db"
            with closing(sqlite3.connect(database)) as connection:
                connection.execute("CREATE TABLE _translationcache (translation TEXT)")
                connection.execute("INSERT INTO _translationcache VALUES ('keep this result')")
                connection.commit()
            with self.assertRaises(BuildError):
                remove_empty_engine_cache(stage)
            with closing(sqlite3.connect(database)) as connection:
                self.assertEqual(connection.execute("SELECT translation FROM _translationcache").fetchone()[0], "keep this result")

    def test_crate_archive_notices_require_locked_digest_and_never_extract_source(self):
        import io
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            stage = Path(temporary)
            archive = stage / "fixture.crate"
            with tarfile.open(archive, "w:gz") as output:
                for name, data in [("Cargo.toml", b'[package]\nname="fixture"\nversion="1.0.0"\nlicense="MIT"\n'), ("LICENSE", b"Synthetic license"), ("src/lib.rs", b"must not extract source")]:
                    member = tarfile.TarInfo("fixture-1.0.0/" + name)
                    member.size = len(data)
                    output.addfile(member, io.BytesIO(data))
            package = {"name": "fixture", "version": "1.0.0", "checksum": digest(archive)}
            record = {"license_files": []}
            rust_archive_notices(stage, archive, package, record)
            self.assertEqual((stage / "licenses/rust/fixture-1.0.0/LICENSE").read_bytes(), b"Synthetic license")
            self.assertFalse((stage / "fixture-1.0.0").exists())
            self.assertEqual(record["license"], "MIT")
            with self.assertRaises(BuildError):
                rust_archive_notices(stage, archive, {**package, "checksum": "0" * 64}, {"license_files": []})

    def test_verified_notice_accepts_only_digest_proven_lf_or_crlf_text(self):
        expected = hashlib.sha256(b"Copyright holder\nLicense text\n").hexdigest()
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            notice = Path(temporary) / "LICENSE.txt"
            notice.write_bytes(b"Copyright holder\r\nLicense text\r\n")
            self.assertEqual(verified_notice_bytes(notice, expected), b"Copyright holder\nLicense text\n")
            notice.write_bytes(b"Different copyright holder\r\nLicense text\r\n")
            with self.assertRaises(BuildError):
                verified_notice_bytes(notice, expected)

    def test_component_inventory_uses_captured_locks_after_source_changes(self):
        from unittest.mock import patch
        inventory = read_json(INVENTORY)
        wheels = select_wheels(inventory, read_json(BASELINE_WHEELS))
        wheels = apply_wheel_overrides(wheels, read_json(WHEEL_OVERRIDES))
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            stage = Path(temporary)
            provenance = stage / "provenance"
            provenance.mkdir()
            for origin in (INVENTORY, REQUIREMENTS, WHEEL_OVERRIDES, BASELINE_WHEELS):
                shutil.copyfile(origin, provenance / origin.name)
            (provenance / "Cargo.lock").write_text('[[package]]\nname="captured-package"\nversion="1.0.0"\n')
            with patch(__name__ + ".REPO", stage / "missing-live-source"):
                self.assertEqual(staged_inputs(stage, wheels), inventory)
                components, _ = rust_inventory(stage, None)
                self.assertEqual([item["name"] for item in components], ["captured-package"])
            (provenance / REQUIREMENTS.name).write_text("changed after capture\n")
            with self.assertRaises(BuildError):
                staged_inputs(stage, wheels)

    def test_windows_wheel_selection_prefers_exact_then_abi3(self):
        base = "https://files.pythonhosted.org/packages/"
        exact = base + "thing-1-cp313-cp313-win_amd64.whl"
        abi3 = base + "thing-1-cp311-abi3-win_amd64.whl"
        self.assertGreater(wheel_rank(exact), wheel_rank(abi3))
        self.assertGreater(wheel_rank(abi3), wheel_rank(base + "thing-1-py3-none-any.whl"))
        self.assertIsNone(wheel_rank(base + "thing-1-cp314-cp314-win_amd64.whl"))
        self.assertIsNone(wheel_rank(base + "thing-1-cp313-cp313-win_arm64.whl"))
        self.assertIsNone(wheel_rank(base + "thing-1-cp313-cp313-manylinux_2_17_x86_64.whl"))
        with self.assertRaises(BuildError):
            wheel_rank("https://synthetic-secret@files.pythonhosted.org/thing.whl")

    def test_atomic_lock_write_and_replace_failure_keep_previous_content(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            path = Path(temporary) / "requirements.txt"
            path.write_text("old synthetic lock\n")
            with patch("os.replace", side_effect=OSError("synthetic sharing violation")):
                with self.assertRaises(OSError):
                    write_lock_file(path, "new synthetic lock\n")
            self.assertEqual(path.read_text(), "old synthetic lock\n")
            self.assertEqual(list(path.parent.glob("*.tmp")), [])
            write_lock_file(path, "new synthetic lock\n")
            self.assertEqual(path.read_bytes(), b"new synthetic lock\n")

    def test_atomic_lock_write_rejects_mocked_reparse_stat_without_touching_sentinel(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            sentinel = root / "sentinel.txt"
            sentinel.write_bytes(b"synthetic user file")
            path = root / "requirements.txt"
            original_lstat = Path.lstat

            def reparse_lstat(item):
                if item == path:
                    return SimpleNamespace(st_mode=stat.S_IFLNK | 0o777, st_file_attributes=0x400)
                return original_lstat(item)

            with patch.object(Path, "lstat", reparse_lstat), self.assertRaises(BuildError):
                write_lock_file(path, "must not be written")
            self.assertEqual(sentinel.read_bytes(), b"synthetic user file")
            self.assertFalse(path.exists())

    def test_unsafe_archive_and_manifest_paths_are_rejected(self):
        for name in ("../outside", "/absolute", "C:/outside", "a\\b", "a//b", "a/./b", "a/../b", "NUL.txt", "a/name.", "a/name ", "a/file:stream", "a/\x00b"):
            with self.subTest(name=name), self.assertRaises(BuildError):
                relative_member(name)
        self.assertEqual(str(relative_member("fonts/a-font.ttf")), "fonts/a-font.ttf")

    def test_output_cannot_replace_or_enclose_inputs(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            existing = root / "existing"
            existing.mkdir()
            with self.assertRaises(BuildError):
                validate_output(existing, [])
            with self.assertRaises(BuildError):
                validate_output(root / "input/new", [root / "input"])
            with self.assertRaises(BuildError):
                validate_output(root / "new", [root / "new/input"])
            with self.assertRaises(BuildError):
                validate_output(REPO / "dist/release/new", [])
            with self.assertRaises(BuildError):
                validate_output(BUILD_INPUTS / "new", [])
            self.assertEqual(validate_output(root / "new", []), root / "new")

    def test_optional_notice_and_registry_inputs_cannot_contain_output_or_work(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            notices = root / "notices"
            notices.mkdir()
            args = parse_args(["--plan", "--output", str(notices / "candidate"), "--asset-notices", str(notices), "--cargo-registry-src", str(root / "registry")])
            inputs = input_paths(args)
            self.assertIn(notices, inputs)
            self.assertIn(root / "registry", inputs)
            with self.assertRaises(BuildError):
                validate_output(args.output, inputs)
            with self.assertRaises(BuildError):
                validate_output(notices / ".candidate.work-synthetic", inputs)
            self.assertFalse((notices / "candidate").exists())
            self.assertEqual(validate_output(root / "candidate", inputs), root / "candidate")

    def test_copy_hash_mismatch_does_not_change_source(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            source = root / "source"
            source.write_bytes(b"synthetic runtime")
            before = digest(source)
            with self.assertRaises(BuildError):
                checked_copy(source, root / "out/file", (17, "0" * 64))
            self.assertEqual(digest(source), before)
            with self.assertRaises(BuildError):
                checked_copy(source, root / "out/file")

    def test_missing_or_tampered_restored_assets_fail(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            home = Path(temporary)
            path = home / ".cache/babeldoc/fonts/synthetic.ttf"
            path.parent.mkdir(parents=True)
            path.write_bytes(b"font")
            expected = {"fonts/synthetic.ttf": {"bytes": 4, "sha3_256": hashlib.sha3_256(b"font").hexdigest()}}
            verify_restored(home, expected)
            path.write_bytes(b"evil")
            with self.assertRaises(BuildError):
                verify_restored(home, expected)
            path.unlink()
            with self.assertRaises(BuildError):
                verify_restored(home, expected)

    def test_seed_manifest_contains_only_verified_asset_bytes(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            home = Path(temporary)
            path = home / ".cache/babeldoc/fonts/synthetic.ttf"
            path.parent.mkdir(parents=True)
            path.write_bytes(b"font")
            expected = {"fonts/synthetic.ttf": {"category": "fonts", "name": "synthetic.ttf", "bytes": 4, "sha3_256": hashlib.sha3_256(b"font").hexdigest()}}
            self.assertEqual(engine_seed_manifest(home, expected), {"schema_version": 1, "asset_tag": ASSET_TAG, "files": [{"category": "fonts", "name": "synthetic.ttf", "bytes": 4, "sha256": hashlib.sha256(b"font").hexdigest()}]})
            path.write_bytes(b"evil")
            with self.assertRaises(BuildError):
                engine_seed_manifest(home, expected)
            path.write_bytes(b"font")
            (path.parent / "extra.ttf").write_bytes(b"extra")
            with self.assertRaises(BuildError):
                engine_seed_manifest(home, expected)

    def test_final_manifest_has_every_file_and_no_self_hash(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            (root / "pdf2zh.exe").write_bytes(b"synthetic executable")
            records = final_manifest(root)
            self.assertEqual(records, [{"path": "pdf2zh.exe", "bytes": 20, "sha256": hashlib.sha256(b"synthetic executable").hexdigest()}])
            self.assertEqual(read_json(root / "RELEASE_FILES.json"), records)
            nested = root / "metadata/RELEASE_FILES.json"
            nested.parent.mkdir()
            nested.write_bytes(b"synthetic nested inventory")
            records = final_manifest(root)
            self.assertEqual(len(records), 2)
            self.assertIn({"path": "metadata/RELEASE_FILES.json", "bytes": nested.stat().st_size, "sha256": digest(nested)}, records)
            (root / ".env").write_text("SYNTHETIC_KEY=not-a-real-key")
            with self.assertRaises(BuildError):
                final_manifest(root)

    def test_only_empty_verified_template_is_allowed_in_config(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            source = root / "resources/config.example.toml"
            target = root / "stage/config/config.example.toml"
            source.parent.mkdir()
            target.parent.mkdir(parents=True)
            source.write_text('[openai_detail]\nopenai_api_key=""\n', encoding="utf-8")
            target.write_bytes(source.read_bytes())
            with patch(__name__ + ".REPO", root):
                self.assertEqual(len(final_manifest(root / "stage")), 1)
                source.write_text('[openai_detail]\nopenai_api_key="synthetic-key"\n', encoding="utf-8")
                target.write_bytes(source.read_bytes())
                with self.assertRaisesRegex(BuildError, "contains credentials"):
                    final_manifest(root / "stage")
                (target.parent / "config.v3.toml").write_text("private runtime config")
                with self.assertRaisesRegex(BuildError, "application config"):
                    final_manifest(root / "stage")

    def test_supplemental_rust_notices_require_identity_and_verified_bytes(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            notices = root / "resources/licenses/rust"
            notices.mkdir(parents=True)
            body = b"Synthetic upstream MIT license fixture"
            (notices / "LICENSE").write_bytes(body)
            write_json(notices / "SUPPLEMENTAL_NOTICES.json", [{"name": "fixture", "version": "1.0", "file": "LICENSE", "sha256": hashlib.sha256(body).hexdigest(), "source": "https://example.invalid/revision/LICENSE"}])
            stage = root / "stage"
            stage.mkdir()
            matching = {"name": "fixture", "version": "1.0", "license_files": []}
            different = {"name": "fixture", "version": "2.0", "license_files": []}
            with patch(__name__ + ".REPO", root):
                supplement_rust_notices(stage, [matching, different])
                self.assertEqual(len(matching["license_files"]), 1)
                self.assertEqual(different["license_files"], [])
                (notices / "LICENSE").write_bytes(b"tampered")
                with self.assertRaisesRegex(BuildError, "SHA256 mismatch"):
                    supplement_rust_notices(root / "other-stage", [{"name": "fixture", "version": "1.0", "license_files": []}])

    def test_environment_drops_api_proxy_and_python_paths(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(dir=REPO) as temporary, patch.dict(os.environ, {"SystemRoot": "C:\\Windows", "OPENAI_API_KEY": "synthetic-secret", "HTTPS_PROXY": "http://synthetic-secret/", "PYTHONPATH": "unsafe", "UV_INDEX_URL": "https://synthetic-secret/"}, clear=True):
            result = sanitized_environment(Path(temporary))
            self.assertFalse({"OPENAI_API_KEY", "HTTPS_PROXY", "PYTHONPATH", "UV_INDEX_URL"} & result.keys())
            self.assertNotIn("synthetic-secret", json.dumps(result))
            self.assertEqual(result["PATH"], str(Path("C:\\Windows") / "System32"))

    def test_bytecode_cleanup_is_confined_to_generated_stage_cache(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            stage = root / "stage"
            cache = stage / "runtime/python/site-packages/thing/__pycache__"
            cache.mkdir(parents=True)
            (cache / "thing.cpython-313.pyc").write_bytes(b"synthetic bytecode")
            (cache.parent / "thing.py").write_text("# synthetic source")
            outside = root / "source.pyc"
            outside.write_bytes(b"original source cache")
            self.assertEqual(remove_generated_bytecode(stage), 1)
            self.assertFalse(cache.exists())
            self.assertEqual(outside.read_bytes(), b"original source cache")
            self.assertTrue((cache.parent / "thing.py").is_file())

    def test_only_proved_absolute_installer_wrapper_is_excluded(self):
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            python = root / "runtime/python/python.exe"
            wrapper = root / "synthetic-console.exe"
            wrapper.write_bytes(b"MZ synthetic installer\n#!" + str(python).encode("utf-8") + b"\n")
            with zipfile.ZipFile(wrapper, "a") as archive:
                archive.writestr("__main__.py", "from synthetic_tool.cli import main\nmain()\n")
            self.assertTrue(exclude_generated_launcher(wrapper, python, "synthetic_tool.cli:main"))
            self.assertFalse(wrapper.exists())
            helper = root / "packaged-native-helper.exe"
            helper.write_bytes(b"MZ synthetic native helper")
            before = digest(helper)
            self.assertFalse(exclude_generated_launcher(helper, python, "synthetic_tool.cli:main"))
            self.assertEqual(digest(helper), before)
            unknown = root / "unclassified.exe"
            unknown.write_bytes(b"MZ unknown\n" + str(python).encode("utf-8"))
            with self.assertRaises(BuildError):
                exclude_generated_launcher(unknown, python, "synthetic_tool.cli:main")
            self.assertTrue(unknown.is_file())

    def test_real_lock_selection_is_exact_engine_closure_without_installing(self):
        inventory = read_json(INVENTORY)
        wheels = select_wheels(inventory, read_json(BASELINE_WHEELS))
        self.assertEqual(len(wheels), 85)
        self.assertEqual({w["name"]: w["version"] for w in wheels}, {name: meta["version"] for name, meta in inventory["engine"].items()})
        self.assertFalse({"gradio", "gradio-client", "pdf2zh-next", "onnxruntime-directml"} & {w["name"] for w in wheels})
        self.assertTrue(all(re.fullmatch(r"[a-f0-9]{64}", w["sha256"]) for w in wheels))

    def test_pe_normal_and_delay_imports_and_truncation(self):
        data = bytearray(1024)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 0x3C, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<HH", data, 68, 0x8664, 0)
        struct.pack_into("<H", data, 84, 240)
        optional = 88
        struct.pack_into("<H", data, optional, 0x20B)
        struct.pack_into("<Q", data, optional + 24, 0x140000000)
        struct.pack_into("<I", data, optional + 60, len(data))
        struct.pack_into("<II", data, optional + 112 + 8, 400, 40)
        struct.pack_into("<II", data, optional + 112 + 13 * 8, 500, 64)
        struct.pack_into("<IIIII", data, 400, 1, 0, 0, 700, 0)
        struct.pack_into("<IIIIIIII", data, 500, 1, 720, 0, 0, 0, 0, 0, 0)
        data[700:713] = b"KERNEL32.dll\0"
        data[720:738] = b"MSVCP140_1.dll\0\0\0\0"
        with tempfile.TemporaryDirectory(dir=REPO) as temporary:
            root = Path(temporary)
            path = root / "synthetic.exe"
            path.write_bytes(data)
            self.assertEqual(pe_imports(path), {"machine": "AMD64", "normal": ["kernel32.dll"], "delay": ["msvcp140_1.dll"]})
            records, missing = native_inventory(root)
            self.assertEqual(missing, [{"importer": "synthetic.exe", "dll": "msvcp140_1.dll"}])
            self.assertEqual(records[0]["dependencies"]["kernel32.dll"]["class"], "windows-component")
            path.write_bytes(data[:96])
            with self.assertRaises(BuildError):
                pe_imports(path)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (BuildError, OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        print(f"Build error: {error}", file=sys.stderr)
        raise SystemExit(1) from None
