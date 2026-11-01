"""Build a complete native Windows x64 or macOS arm64 bundle from pinned inputs.

No upload, signing, release creation or discovery of user credentials occurs.
macOS builds require an actual Apple Silicon Mac; unsupported inputs fail closed.
"""
from __future__ import annotations

import argparse
import importlib.metadata
import json
import os
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import build_hybrid_windows as windows
from build_support import (ROOT, PINS, audit_macos, checked_files, extract_archive, fetch,
                           isolated_environment, manifest, run, sha256, verify_manifest, zip_bundle)
from build_hybrid_windows import BuildError, require, write_json

CLI_NAME = "BabelOxide"
GUI_NAME = "BabelOxide-gui"


def product_version() -> str:
    import tomllib
    return tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]


def platform_profile(name: str) -> tuple[list[dict], Path]:
    if name == "windows-x64":
        return windows.apply_wheel_overrides(windows.select_wheels(windows.read_json(windows.INVENTORY), windows.read_json(windows.BASELINE_WHEELS)), windows.read_json(windows.WHEEL_OVERRIDES)), windows.REQUIREMENTS
    profile = windows.read_json(ROOT / "python-worker/wheels-macos-arm64.json")
    require(profile["python"] == "3.13.16" and profile["platform"] == "macosx_14_0_arm64", "Wrong macOS worker profile")
    wheels = profile["wheels"]
    require(len(wheels) == 84 and len({wheel["name"] for wheel in wheels}) == 84, "Wrong macOS closure")
    require(next(wheel["version"] for wheel in wheels if wheel["name"] == "babeldoc") == "0.6.4", "Wrong engine version")
    lock = ROOT / "python-worker/requirements-macos-arm64.lock.txt"
    expected = "\n".join(f"{wheel['name']} @ {wheel['url']} --hash=sha256:{wheel['sha256']}" for wheel in wheels)
    actual = "\n".join(line for line in lock.read_text(encoding="utf-8").splitlines() if line and not line.startswith("#"))
    require(actual == expected, "macOS wheel lock differs from the reviewed profile")
    return wheels, lock


def python_layout(root: Path, name: str) -> tuple[Path, Path]:
    if name == "windows-x64":
        return root / "python.exe", root / "site-packages"
    return root / "bin/python3", root / "lib/python3.13/site-packages"


def prepare_inputs(name: str, inputs: Path, uv: Path, bin_dir: Path, work: Path) -> tuple[Path, Path]:
    pins = windows.read_json(PINS)["platforms"][name]
    inputs.mkdir(parents=True, exist_ok=True)
    runtime = inputs / "runtime"
    if not runtime.exists():
        archive = fetch(pins["python_archive"], inputs / "downloads/python.archive")
        if name == "windows-x64":
            extract_archive(archive, runtime)
            write_json(inputs / "runtime-files.json", [{"path": f"runtime/{path.name}", "bytes": path.stat().st_size, "sha256": sha256(path)} for path in checked_files(runtime)])
        else:
            unpacked = inputs / "python-unpacked"
            extract_archive(archive, unpacked, standalone=True)
            (unpacked / "python").rename(runtime)
    if name == "windows-x64":
        windows.runtime_files(runtime, inputs / "runtime-files.json")
    pdfium = inputs / "pdfium"
    if not pdfium.exists():
        archive = fetch(pins["pdfium_archive"], inputs / "downloads/pdfium.tgz")
        extract_archive(archive, pdfium)
    assets = inputs / f"offline_assets_{windows.ASSET_TAG}.zip"
    expected = windows.asset_members(windows.read_json(windows.ASSET_INVENTORY))
    if not assets.exists():
        # A separate copied interpreter is only a build input, never the user's
        # venv or a source of mutable configuration for the published bundle.
        seed_runtime = work / "seed-runtime"
        shutil.copytree(runtime, seed_runtime, symlinks=True)
        python, site = python_layout(seed_runtime, name)
        if name == "windows-x64":
            (seed_runtime / "python313._pth").write_text(windows.PTH_CONTENT, encoding="ascii")
        _, lock = platform_profile(name)
        env = isolated_environment(work)
        run([uv, "--no-config", "pip", "install", "--python", python, "--target", site, "--no-deps", "--no-index", "--require-hashes", "--only-binary", ":all:", "--link-mode", "copy", "-r", lock], work, env, label="seed-runtime-install")
        executable = bin_dir / (CLI_NAME + (".exe" if name == "windows-x64" else ""))
        run([executable, "--python", python, "--worker", ROOT / "python-worker/worker.py", "--worker-home", work / "seed-home", "--generate-offline-assets", inputs, "--json"], work, env, timeout=1800, label="download-locked-engine-assets")
    windows.verify_zip(assets, expected)
    return runtime, assets


def mac_entrypoints(stage: Path, python: Path, site: Path) -> list[dict]:
    excluded = []
    interpreters = {str(python), str(python.resolve())}
    for distribution in importlib.metadata.distributions(path=[str(site)]):
        for entry in distribution.entry_points:
            if entry.group not in {"console_scripts", "gui_scripts"}:
                continue
            module, _, function = entry.value.partition(":")
            function = function.split("[", 1)[0].strip()
            for member in distribution.files or []:
                if Path(str(member)).name != entry.name:
                    continue
                path = Path(distribution.locate_file(member)).resolve()
                require(path.resolve().is_relative_to(stage.resolve()), "Installed wrapper escaped stage")
                if not path.is_file():
                    continue
                body = path.read_bytes()
                if any(interpreter.encode() in body for interpreter in interpreters):
                    require(len(body) < 65536 and body.startswith(b"#!") and f"from {module} import {function}".encode() in body and f"{function}()".encode() in body, "Unclassified installed entrypoint contains build paths")
                    excluded.append({"path": path.relative_to(stage).as_posix(), "sha256": sha256(path), "reason": "verified installer-generated absolute Python wrapper"})
                    path.unlink()
    return excluded


def build_macos(args, runtime: Path, assets: Path, work: Path) -> Path:
    require(sys.platform == "darwin" and platform.machine() == "arm64", "macOS bundle requires a native Apple Silicon Mac")
    wheels, lock = platform_profile("macos-arm64")
    prepared = work / "payload"
    app = prepared / "BabelOxide.app"
    stage = app / "Contents/MacOS"
    stage.mkdir(parents=True)
    shutil.copytree(runtime, stage / "runtime/python", symlinks=True)
    python, site = python_layout(stage / "runtime/python", "macos-arm64")
    for name in (CLI_NAME, GUI_NAME):
        source = args.bin_dir / name
        require(source.is_file(), "Build both native Rust binaries before packaging")
        shutil.copy2(source, stage / name)
    env = isolated_environment(work)
    run([args.uv, "--no-config", "pip", "install", "--python", python, "--target", site, "--no-deps", "--no-index", "--require-hashes", "--only-binary", ":all:", "--link-mode", "copy", "-r", lock], work, env, label="install-macos-wheels")
    require(not any(site.rglob("*.pth")), "Wheel introduced an unreviewed .pth hook")
    excluded = mac_entrypoints(stage, python, site)
    (stage / "babeldoc").mkdir()
    shutil.copy2(ROOT / "python-worker/worker.py", stage / "babeldoc/worker.py")
    pdfium = args.inputs / "pdfium"
    library = pdfium / "lib/libpdfium.dylib"
    require(library.is_file(), "Pinned PDFium arm64 library is missing")
    (stage / "native/pdfium").mkdir(parents=True)
    shutil.copy2(library, stage / "native/pdfium/libpdfium.dylib")
    for origin in (pdfium / "LICENSE", pdfium / "VERSION", pdfium / "args.gn"):
        windows.checked_copy(origin, stage / "licenses/pdfium" / origin.name)
    shutil.copytree(pdfium / "licenses", stage / "licenses/pdfium/licenses")
    windows.checked_copy(ROOT / "LICENSE", stage / "licenses/project/LICENSE")
    windows.checked_copy(ROOT / "THIRD_PARTY_NOTICES.md", stage / "licenses/project/THIRD_PARTY_NOTICES.md")
    windows.checked_copy(stage / "runtime/python/lib/python3.13/LICENSE.txt", stage / "licenses/python-runtime/LICENSE.txt")
    shutil.copytree(ROOT / "resources/licenses/engine-assets", stage / "licenses/engine-assets")
    windows.checked_copy(ROOT / "resources/config.example.toml", stage / "config/config.example.toml")
    provenance = stage / "provenance"
    for source in (PINS, lock, ROOT / "python-worker/wheels-macos-arm64.json", windows.ASSET_INVENTORY, windows.INVENTORY, ROOT / "Cargo.lock", ROOT / "Cargo.toml", Path(__file__), ROOT / "script/build_support.py", ROOT / "script/build_hybrid_windows.py", ROOT / "docs/BUILD.md"):
        windows.checked_copy(source, provenance / source.name)
    write_json(provenance / "WHEELS.json", wheels)
    (stage / "engine-assets").mkdir()
    shutil.copy2(assets, stage / "engine-assets" / assets.name)
    info = {"CFBundleName": "BabelOxide", "CFBundleDisplayName": "BabelOxide", "CFBundleIdentifier": "org.babeloxide.desktop", "CFBundleExecutable": GUI_NAME, "CFBundlePackageType": "APPL", "CFBundleShortVersionString": product_version(), "CFBundleVersion": product_version(), "LSMinimumSystemVersion": "14.0", "NSHighResolutionCapable": True}
    info["CFBundleIconFile"] = "BabelOxide.icns"
    (app / "Contents/Resources").mkdir()
    shutil.copy2(ROOT / "resources/babeloxide.icns", app / "Contents/Resources/BabelOxide.icns")
    (app / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
    # Relocation happens before native loading, asset restore and smoke checks.
    relocated = work / "relocated payload"
    prepared.rename(relocated)
    app = relocated / "BabelOxide.app"
    stage = app / "Contents/MacOS"
    python, site = python_layout(stage / "runtime/python", "macos-arm64")
    env["BABELOXIDE_APP_DIR"] = str(work / "smoke-app")
    home = stage / "resources/worker-home"
    home.mkdir(parents=True)
    run([stage / CLI_NAME, "--restore-offline-assets", stage / "engine-assets" / assets.name, "--worker-home", home, "--json"], work, env, label="restore-macos-assets")
    expected = windows.asset_members(windows.read_json(windows.ASSET_INVENTORY))
    windows.verify_restored(home, expected)
    probe = windows.PROBE.replace("root = pathlib.Path(sys.executable).parent", "root = pathlib.Path(sys.executable).parent.parent")
    probe += '\nassert "CPUExecutionProvider" in importlib.import_module("onnxruntime").get_available_providers()\n'
    probe_env = dict(env, HOME=str(home), USERPROFILE=str(home))
    run([python, "-I", "-B", "-c", probe], work, probe_env, label="relocated-macos-native-probe")
    for name in (CLI_NAME, GUI_NAME):
        run([stage / name, "--version"], work, env, label=name + "-version")
        require((work / (name + "-version.log")).read_text(encoding="utf-8").strip() == f"BabelOxide {product_version()}", "Native executable version differs from the workspace")
    components, issues = windows.python_inventory(stage, wheels, windows.read_json(windows.INVENTORY)["engine"], site=site)
    rust, rust_issues = windows.rust_inventory(stage, args.cargo_registry_src)
    issues += rust_issues
    for path in checked_files(stage):
        if not path.is_symlink() and (path.suffix in {".pyc", ".pyo"} or path.name == ".DS_Store"):
            path.unlink()
    windows.remove_empty_engine_cache(stage)
    write_json(stage / "resources/engine-assets.json", windows.engine_seed_manifest(home, expected))
    natives = audit_macos(stage, stage / "runtime/python")
    write_json(stage / "NATIVE_DEPENDENCIES.json", {"files": natives, "unresolved": [], "scope": "Mach-O arm64 slices, load commands and bundled rpaths; import/CLI smoke also required"})
    write_json(stage / "THIRD_PARTY_COMPONENTS.json", {"python": components, "rust_lock_all_targets": rust, "python_runtime": windows.read_json(PINS)["platforms"]["macos-arm64"]["python_archive"], "pdfium": windows.read_json(PINS)["platforms"]["macos-arm64"]["pdfium_archive"], "license_review_items": issues})
    write_json(stage / "ARTIFACT_EXCLUSIONS.json", {"generated_wrappers": excluded, "standalone_pip": "excluded from verified runtime archive before installing the exact engine closure"})
    write_json(stage / "BUILD_INFO.json", {"product": "BabelOxide", "version": product_version(), "platform": "macos-arm64", "minimum_macos": "14.0", "babeldoc": "0.6.4", "runtime_python": "3.13.16", "source_commit": args.source_commit, "release_ready": False, "signed": False, "notarized": False, "checks": {"relocated_native_imports": "passed", "asset_restore": "passed", "cli_version": "passed", "gui_version_only": "passed", "gui_desktop_interaction": "unverified"}, "limitations": ["Apple Silicon only; no Intel artifact", "Unsigned and not notarized; Gatekeeper may block launch", "CPU fallback verified by import; CoreML acceleration is not guaranteed", "Interactive GUI and corresponding-source distribution review remain required"] + issues})
    windows.checked_copy(ROOT / "resources/macos-README.md", stage / "README_zh_TW.md")
    records = manifest(app, stage / "RELEASE_FILES.json")
    verify_manifest(app, stage / "RELEASE_FILES.json")
    require(not args.output.exists(), "Output already exists; refusing replacement")
    relocated.rename(args.output)
    print(json.dumps({"output": str(args.output), "files": len(records), "release_ready": False}))
    return args.output


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=["windows-x64", "macos-arm64"], required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--inputs", type=Path)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "target/release")
    parser.add_argument("--uv", type=Path)
    parser.add_argument("--cargo-registry-src", type=Path)
    parser.add_argument("--source-commit")
    parser.add_argument("--allow-unresolved-native", action="store_true", help="Windows: explicitly require the official VC++ Runtime on the target")
    parser.add_argument("--zip", action="store_true")
    args = parser.parse_args(argv)
    args.output = windows.plain_path(args.output).resolve()
    args.inputs = (args.inputs or ROOT / "build-inputs" / args.platform).absolute()
    args.bin_dir = args.bin_dir.absolute()
    args.uv = (args.uv or Path(shutil.which("uv") or "")).absolute()
    require(args.uv.is_file(), "Pass an existing uv executable")
    require(args.output.is_relative_to(ROOT / "dist") and not args.output.exists(), "Use a new output directory under dist")
    require((os.name == "nt" and platform.machine().lower() in {"amd64", "x86_64"}) if args.platform == "windows-x64" else (sys.platform == "darwin" and platform.machine() == "arm64"), "Requested bundle architecture does not match this build host")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix=".portable-", dir=args.output.parent))
    print("Preserved build logs: " + str(work), flush=True)
    runtime, assets = prepare_inputs(args.platform, args.inputs, args.uv, args.bin_dir, work)
    if args.platform == "windows-x64":
        arguments = ["--output", str(args.output), "--bin-dir", str(args.bin_dir), "--runtime-source", str(runtime), "--runtime-manifest", str(args.inputs / "runtime-files.json"), "--pdfium-source", str(args.inputs / "pdfium"), "--assets-zip", str(assets), "--online", "--uv", str(args.uv)]
        if args.cargo_registry_src:
            arguments += ["--cargo-registry-src", str(args.cargo_registry_src)]
        if args.source_commit:
            arguments += ["--source-commit", args.source_commit]
        if args.allow_unresolved_native:
            arguments.append("--allow-unresolved-native")
        require(windows.main(arguments) == 0, "Windows bundle failed")
        output = args.output
    else:
        output = build_macos(args, runtime, assets, work)
    if args.zip:
        zip_bundle(output, output.with_name(output.name + ".zip"))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (BuildError, OSError, subprocess.SubprocessError) as error:
        print("Build failed: " + str(error), file=sys.stderr)
        raise SystemExit(1)
