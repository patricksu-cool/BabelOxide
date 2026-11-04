"""Prepare/check the existing portable runtime and launch source-built binaries.

Only the existing builders download or install runtime inputs. No provider is
contacted by preparation/checking; GUI/CLI retain their normal private settings.
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import build_hybrid_windows as windows
import build_portable as portable
import ci
from build_support import ROOT, checked_files, isolated_environment
from build_hybrid_windows import BuildError, require


def host_platform() -> str:
    machine = platform.machine().lower()
    if sys.platform == "win32" and machine in {"amd64", "x86_64"}:
        return "windows-x64"
    if sys.platform == "darwin" and machine == "arm64":
        require(int(platform.mac_ver()[0].split(".")[0]) >= 14, "macOS 14+ is required")
        return "macos-arm64"
    raise BuildError("Development runtime requires native Windows x64 or macOS ARM64 (14+)")


def runtime_output(name: str) -> Path:
    # Rust/Worker edits do not invalidate Python and model caches. A change to
    # locked inputs or their builder gets a new directory, never an overwrite.
    inputs = [
        "resources/build-inputs.lock.json", "resources/engine-assets.lock.json",
        "python-worker/dependencies-win64.json", "python-worker/baseline-wheels.json",
        "python-worker/wheel-overrides.json", "python-worker/requirements-win64.lock.txt",
        "python-worker/wheels-macos-arm64.json", "python-worker/requirements-macos-arm64.lock.txt",
        "script/build_portable.py", "script/build_support.py", "script/build_hybrid_windows.py",
    ]
    digest = hashlib.sha256()
    for filename in inputs:
        digest.update(filename.encode())
        digest.update((ROOT / filename).read_bytes())
    return ROOT / "dist" / f"BabelOxide-dev-{name}-{digest.hexdigest()[:12]}"


def bundle_root(output: Path, name: str) -> Path:
    return output if name == "windows-x64" else output / "BabelOxide.app/Contents/MacOS"


def release_binaries(name: str, *, build: bool, check: bool) -> tuple[Path, Path]:
    suffix = ".exe" if name == "windows-x64" else ""
    cli, gui = (ROOT / "target/release" / (base + suffix) for base in ("BabelOxide", "BabelOxide-gui"))
    missing = not cli.is_file() or not gui.is_file()
    require(not check or not missing, "Rust binaries are missing; run the development launcher with --build")
    if build or missing:
        require(shutil.which("cargo") is not None, "Install Rust/Cargo, then run the launcher again")
        print("Building Rust Release GUI and CLI...", flush=True)
        subprocess.run(["cargo", "build", "--release", "--workspace", "--locked"], cwd=ROOT, check=True)
    require(cli.is_file() and gui.is_file(), "Cargo did not create target/release/BabelOxide and BabelOxide-gui")
    return cli, gui


def prepare_runtime(name: str, work: Path) -> Path:
    output = runtime_output(name)
    if not output.exists():
        uv = ci.bootstrap(name, work).resolve()
        registry = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))) / "registry/src"
        arguments = ["--platform", name, "--output", str(output), "--uv", str(uv),
                     "--cargo-registry-src", str(registry)]
        if name == "windows-x64":
            arguments.append("--allow-unresolved-native")
        print("Preparing the pinned portable runtime (first run needs internet)...", flush=True)
        require(portable.main(arguments) == 0, "Portable runtime preparation failed")
    return bundle_root(output, name)


def verify_runtime(root: Path, name: str) -> None:
    """Check immutable bundle trees, allowing the bundle's separate user data.

    RELEASE_FILES is rooted at .app on macOS. Config/cache/outputs may have been
    created by an earlier launch and must neither invalidate nor be modified by
    a development check. Extra files inside immutable trees are still rejected.
    """
    info = windows.read_json(root / "BUILD_INFO.json")
    runtime_platform = info.get("platform")
    if runtime_platform is None and info.get("artifact_kind") == "babeloxide-windows-x64":
        runtime_platform = "windows-x64"
    require(runtime_platform == name, "Runtime platform differs from this host; do not mix platform bundles")
    require(info["babeldoc"] == "0.6.4" and info["runtime_python"] == "3.13.16", "Runtime versions differ from the pinned development environment")
    unresolved = windows.read_json(root / "NATIVE_DEPENDENCIES.json")["unresolved"]
    vc_runtime = {"msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll", "vcruntime140.dll", "vcruntime140_1.dll", "concrt140.dll"}
    require(all(name == "windows-x64" and row.get("dll", "").lower() in vc_runtime for row in unresolved), "Runtime has unresolved native dependencies other than the official Windows VC++ Runtime")
    manifest_root = root if name == "windows-x64" else root.parents[1]
    rows = windows.read_json(root / "RELEASE_FILES.json")
    expected = {row["path"]: row for row in rows}
    require(len(expected) == len(rows), "Duplicate runtime manifest entries")
    scopes = ("runtime", "babeldoc", "native", "resources", "engine-assets", "provenance")
    prefix = root.relative_to(manifest_root)
    selected = {key: row for key, row in expected.items()
                if windows.relative_member(key).parts[:len(prefix.parts)] == prefix.parts
                and windows.relative_member(key).parts[len(prefix.parts):len(prefix.parts) + 1]
                in {(scope,) for scope in scopes}}
    actual = {path.relative_to(manifest_root).as_posix(): path
              for scope in scopes for path in checked_files(root / scope)}
    require(actual.keys() == selected.keys(), "Missing/unexpected files in immutable runtime trees; prepare a new complete runtime")
    for key, path in actual.items():
        row = selected[key]
        if row.get("kind") == "symlink":
            require(path.is_symlink() and os.readlink(path) == row["target"], "Runtime symlink differs from manifest")
        else:
            require(not path.is_symlink(), "Unexpected runtime symlink")
            windows.verify_file(path, row["bytes"], row["sha256"])
    wheels, _ = portable.platform_profile(name)
    require(windows.read_json(root / "provenance/WHEELS.json") == wheels, "Runtime wheel profile differs from the current source lock")
    require((root / "provenance/build-inputs.lock.json").read_bytes() == portable.PINS.read_bytes(), "Runtime download pins differ from the current source lock")
    assets = windows.asset_members(windows.read_json(windows.ASSET_INVENTORY))
    seed = windows.engine_seed_manifest(root / "resources/worker-home", assets)
    require(windows.read_json(root / "resources/engine-assets.json") == seed, "Runtime asset seed manifest differs from the verified models/fonts")


PDFIUM_PROBE = r'''
import ctypes, os
library = pathlib.Path(sys.argv[1])
if sys.platform == "win32":
    handle = os.add_dll_directory(str(library.parent))
    pdfium = ctypes.CDLL(str(library), winmode=0x1100)
else:
    pdfium = ctypes.CDLL(str(library))
pdfium.FPDF_InitLibrary.argtypes = []
pdfium.FPDF_InitLibrary.restype = None
pdfium.FPDF_DestroyLibrary.argtypes = []
pdfium.FPDF_DestroyLibrary.restype = None
pdfium.FPDF_InitLibrary()
pdfium.FPDF_DestroyLibrary()
print("Native PDFium initialization: passed")
'''


def check_runtime(root: Path, name: str, cli: Path, work: Path) -> None:
    print("Checking immutable Python, wheels, PDFium and model/font resources...", flush=True)
    verify_runtime(root, name)
    env = isolated_environment(work)
    home = work / "worker-home"
    home.mkdir()
    # This shared Rust/Worker operation seeds the verified assets and initializes
    # BabelDOC without constructing a translation provider or using credentials.
    ci.run([cli, *runtime_arguments(root), "--worker-home", home, "--warmup", "--json"],
           work, env, timeout=600, label="worker-warmup")
    python, _ = portable.python_layout(root / "runtime/python", name)
    probe = windows.PROBE
    if name == "macos-arm64":
        probe = probe.replace("root = pathlib.Path(sys.executable).parent", "root = pathlib.Path(sys.executable).parent.parent")
    library = root / "native/pdfium" / ("pdfium.dll" if name == "windows-x64" else "libpdfium.dylib")
    probe_env = dict(env, HOME=str(home), USERPROFILE=str(home))
    ci.run([python, "-I", "-B", "-c", probe + PDFIUM_PROBE, library],
           work, probe_env, timeout=180, label="python-native-probe")
    print(f"Runtime checks passed; no translation API used.\nRuntime: {root}\nLogs: {work}", flush=True)


def runtime_arguments(root: Path) -> list[str]:
    return ["--runtime-dir", str(root), "--worker", str(ROOT / "python-worker/worker.py")]


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", action="store_true", help="Run cargo build --release --workspace --locked even if binaries exist")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="Check existing Rust/runtime files without building/downloading or launching GUI")
    mode.add_argument("--prepare", action="store_true", help="Prepare and check the runtime, then exit without launching GUI")
    mode.add_argument("--cli", action="store_true", help="Run source Release CLI; put its arguments after --")
    parser.add_argument("--runtime-dir", type=Path, help="Reuse a complete trusted bundle's runtime directory instead of preparing one")
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.check and args.build:
        parser.error("--check does not build; use --build --prepare instead")
    forwarded = args.arguments[1:] if args.arguments[:1] == ["--"] else args.arguments
    if forwarded and not args.cli:
        parser.error("only --cli accepts arguments after --")
    if any(value.split("=", 1)[0] in {"--runtime-dir", "--python", "--worker", "--worker-home"} for value in forwarded):
        parser.error("runtime paths are managed by the launcher; choose its --runtime-dir before --cli")
    # Keep the invoked CLI's stdout clean, including its --json protocol.
    with contextlib.redirect_stdout(sys.stderr):
        name = host_platform()
        cli, gui = release_binaries(name, build=args.build, check=args.check)
        log_root = ROOT / ".test-output/dev-launch"
        log_root.mkdir(parents=True, exist_ok=True)
        work = Path(tempfile.mkdtemp(prefix=name + "-", dir=log_root))
        root = args.runtime_dir.resolve() if args.runtime_dir else bundle_root(runtime_output(name), name)
        if not args.runtime_dir and not args.check:
            root = prepare_runtime(name, work)
        require(root.is_dir(), "Development runtime is missing; run the launcher with --prepare (internet required on first run)")
        check_runtime(root, name, cli, work)
        if args.check or args.prepare:
            return 0
        executable = cli if args.cli else gui
        print("Starting source Release " + ("CLI" if args.cli else "GUI") + "...", flush=True)
    return subprocess.run([executable, *runtime_arguments(root), *forwarded],
                          cwd=None if args.cli else ROOT, check=False).returncode


def entrypoint() -> int:
    try:
        return main()
    except (BuildError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print("Development launch failed: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(entrypoint())
