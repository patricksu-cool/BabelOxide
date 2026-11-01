"""Local/Actions build entry point. Never pushes, publishes or uses API keys."""
from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import venv
import uuid
from pathlib import Path

from build_support import ROOT, PINS, fetch, run, verify_manifest, zip_bundle
from build_hybrid_windows import BuildError, require
from build_portable import CLI_NAME, product_version


def bootstrap(name: str, work: Path) -> Path:
    pins = json.loads(PINS.read_bytes())
    current = shutil.which("uv")
    if current:
        version = subprocess.check_output([current, "--version"], text=True).split()[1]
        if version == pins["uv_version"]:
            return Path(current)
    tools = ROOT / ".build-tools"
    python = tools / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    uv = tools / ("Scripts/uv.exe" if os.name == "nt" else "bin/uv")
    if not python.exists():
        venv.EnvBuilder(with_pip=True).create(tools)
    pin = pins["platforms"][name]["uv_wheel"]
    wheel = fetch(pin, ROOT / "build-inputs/tools" / pin["filename"])
    run([python, "-m", "pip", "install", "--no-index", "--no-deps", wheel], work, dict(os.environ), label="bootstrap-pinned-uv")
    require(subprocess.check_output([uv, "--version"], text=True).split()[1] == pins["uv_version"], "Unexpected uv version")
    return uv


def check(uv: Path, work: Path, env: dict[str, str]) -> None:
    run([uv, "--no-config", "sync", "--project", ROOT, "--locked", "--python", sys.executable], work, env, label="dev-sync")
    python = ROOT / (".venv/Scripts/python.exe" if os.name == "nt" else ".venv/bin/python")
    env["BABELOXIDE_TEST_PYTHON"] = str(python)
    cargo = ["cargo"]
    manifest = ROOT / "Cargo.toml"
    run(cargo + ["fmt", "--manifest-path", manifest, "--all", "--", "--check"], work, env, label="rustfmt")
    run(cargo + ["clippy", "--manifest-path", manifest, "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"], work, env, label="clippy")
    run(cargo + ["test", "--manifest-path", manifest, "--workspace", "--locked"], work, env, label="rust-tests")
    run([python, "-m", "pytest", ROOT / "python-worker/tests", "-q", "--basetemp", work / "pytest"], work, env, label="worker-tests")
    run([python, ROOT / "script/build_hybrid_windows.py", "--self-test"], work, env, label="windows-builder-tests")
    run([python, "-m", "unittest", "discover", "-s", ROOT / "script", "-p", "test_build_portable.py"], work, env, label="portable-builder-tests")
    run([python, "-m", "ruff", "check", ROOT / "python-worker", ROOT / "script"], work, env, label="python-lint")


def build(name: str, uv: Path, work: Path, env: dict[str, str]) -> None:
    run_id = uuid.uuid4().hex[:8]
    work = work / ("build-" + run_id)
    work.mkdir()
    run(["cargo", "build", "--manifest-path", ROOT / "Cargo.toml", "--release", "--workspace", "--locked"], work, env, label="release-build")
    output = ROOT / "dist" / f"BabelOxide-v{product_version()}-{name}"
    registry = Path(env.get("CARGO_HOME", str(Path.home() / ".cargo"))) / "registry/src"
    revision = subprocess.check_output(["git", "-C", ROOT, "rev-parse", "--verify", "HEAD"], text=True).strip()
    if subprocess.check_output(["git", "-C", ROOT, "status", "--porcelain", "--untracked-files=normal"], text=True).strip():
        revision += "-dirty"
    command = [sys.executable, ROOT / "script/build_portable.py", "--platform", name, "--output", output, "--uv", uv, "--source-commit", revision, "--cargo-registry-src", registry]
    if name == "windows-x64":
        command += ["--allow-unresolved-native"]
    run(command, work, env, timeout=3600, label="portable-bundle")
    bundle = output if name == "windows-x64" else output / "BabelOxide.app/Contents/MacOS"
    python = bundle / ("runtime/python/python.exe" if name == "windows-x64" else "runtime/python/bin/python3")
    executable = bundle / (CLI_NAME + (".exe" if name == "windows-x64" else ""))
    # A known official VC++ Runtime prerequisite is explicit. Any other missing
    # native import is a build failure, even with the diagnostic builder flag.
    native = json.loads((bundle / "NATIVE_DEPENDENCIES.json").read_bytes())
    vc_runtime = {"msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll", "vcruntime140.dll", "vcruntime140_1.dll", "concrt140.dll"}
    require(all(row.get("dll", "").lower() in vc_runtime for row in native["unresolved"]), "Unresolved non-system native dependencies")
    isolated = dict(env)
    home = work / "runtime-test-home"
    home.mkdir()
    isolated.update(HOME=str(home), USERPROFILE=str(home))
    run([python, "-I", "-B", ROOT / "script/test_engine_regression.py"], work, isolated, label="bounded-engine-regressions")
    fixture = work / "synthetic.pdf"
    code = "import pymupdf,sys; d=pymupdf.open(); p=d.new_page(); p.insert_text((72,72),'A synthetic bilingual translation fixture.'); d.save(sys.argv[1])"
    run([python, "-I", "-B", "-c", code, fixture], work, isolated, label="make-public-fixture")
    dev_python = ROOT / (".venv/Scripts/python.exe" if os.name == "nt" else ".venv/bin/python")
    run([dev_python, ROOT / "script/rust_migration_smoke.py", fixture, "--label", "ci-" + name + "-" + run_id, "--exe", executable, "--packaged", "--isolated-cwd", "--timeout-seconds", "600"], work, env, timeout=700, label="cli-ipc-mock-smoke")
    native_env = dict(env, BABELOXIDE_PREVIEW_NATIVE_APP_ROOT=str(bundle), BABELOXIDE_PREVIEW_NATIVE_PDF=str(fixture))
    run(["cargo", "test", "--manifest-path", ROOT / "Cargo.toml", "-p", "babeloxide-pdf-preview", "--test", "native_smoke", "--locked", "--", "--ignored"], work, native_env, label="pdfium-native-smoke")
    run([python, "-I", "-B", ROOT / "script/check_workflow.py"], work, isolated, label="workflow-static-check")
    manifest_root = output if name == "windows-x64" else output / "BabelOxide.app"
    verify_manifest(manifest_root, bundle / "RELEASE_FILES.json")
    zip_bundle(output, output.with_name(output.name + ".zip"))
    print(json.dumps({"artifact": output.name + ".zip", "bytes": output.with_name(output.name + ".zip").stat().st_size, "gui_desktop_interaction": "unverified", "release_published": False}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=["windows-x64", "macos-arm64"], required=True)
    parser.add_argument("--metadata", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--rust-toolchain", help="Existing toolchain with the exact pinned rustc version; CI defaults to the versioned toolchain")
    args = parser.parse_args()
    if args.metadata:
        if output := os.environ.get("GITHUB_OUTPUT"):
            with Path(output).open("a", encoding="utf-8") as stream:
                stream.write(f"version={product_version()}\n")
        print("BabelOxide v" + product_version())
        return
    require((os.name == "nt" and platform.machine().lower() in {"amd64", "x86_64"}) if args.platform == "windows-x64" else (sys.platform == "darwin" and platform.machine() == "arm64"), "Runner architecture differs from the artifact")
    work = ROOT / ".test-output/ci" / args.platform
    work.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    pinned_rust = json.loads(PINS.read_bytes())["rust_toolchain"]
    env.update(UV_CACHE_DIR=str(ROOT / ".uv-cache"), UV_PYTHON_DOWNLOADS="never", PYTHONUTF8="1", RUSTUP_TOOLCHAIN=args.rust_toolchain or pinned_rust)
    require(subprocess.check_output(["rustc", "--version"], env=env, text=True).split()[1] == pinned_rust, "Rust compiler differs from the pinned build toolchain")
    uv = bootstrap(args.platform, work)
    if args.check:
        check(uv, work, env)
    if args.build:
        build(args.platform, uv, work, env)


if __name__ == "__main__":
    try:
        main()
    except (BuildError, OSError, subprocess.SubprocessError) as error:
        print("CI failed: " + str(error), file=sys.stderr)
        raise SystemExit(1)
