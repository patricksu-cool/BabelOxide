"""Development launch boundaries; synthetic bundles, no downloads or API use."""
import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import build_support as support
import dev
from build_hybrid_windows import BuildError


class DevelopmentLaunchTests(unittest.TestCase):
    def test_native_platforms_and_unsupported_hosts(self):
        for system, machine, expected in [("win32", "AMD64", "windows-x64"), ("darwin", "arm64", "macos-arm64")]:
            with patch.object(dev.sys, "platform", system), patch.object(dev.platform, "machine", return_value=machine), patch.object(dev.platform, "mac_ver", return_value=("14.0", (), "")):
                self.assertEqual(dev.host_platform(), expected)
        for system, machine in [("darwin", "x86_64"), ("win32", "ARM64"), ("linux", "x86_64")]:
            with patch.object(dev.sys, "platform", system), patch.object(dev.platform, "machine", return_value=machine):
                with self.assertRaisesRegex(BuildError, "native"):
                    dev.host_platform()
        with patch.object(dev.sys, "platform", "darwin"), patch.object(dev.platform, "machine", return_value="arm64"), patch.object(dev.platform, "mac_ver", return_value=("13.0", (), "")):
            with self.assertRaisesRegex(BuildError, "14"):
                dev.host_platform()

    def test_locked_inputs_get_platform_separated_reusable_output_paths(self):
        win = dev.runtime_output("windows-x64")
        mac = dev.runtime_output("macos-arm64")
        self.assertNotEqual(win, mac)
        self.assertEqual(win, dev.runtime_output("windows-x64"))
        self.assertEqual(dev.bundle_root(mac, "macos-arm64"), mac / "BabelOxide.app/Contents/MacOS")

    def test_missing_binaries_check_does_not_build_but_normal_launch_builds_locked(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(dev, "ROOT", Path(directory)):
            root = Path(directory)
            with patch.object(dev.subprocess, "run") as run:
                with self.assertRaisesRegex(BuildError, "--build"):
                    dev.release_binaries("windows-x64", build=False, check=True)
                run.assert_not_called()

                def build(*args, **kwargs):
                    for name in ("BabelOxide.exe", "BabelOxide-gui.exe"):
                        path = root / "target/release" / name
                        path.parent.mkdir(parents=True, exist_ok=True)
                        path.write_bytes(b"synthetic executable")
                run.side_effect = build
                with patch.object(dev.shutil, "which", return_value="cargo"):
                    cli, gui = dev.release_binaries("windows-x64", build=False, check=False)
                self.assertTrue(cli.is_file() and gui.is_file())
                self.assertEqual(run.call_args.args[0], ["cargo", "build", "--release", "--workspace", "--locked"])
                self.assertEqual(run.call_args.kwargs["cwd"], root)

    def test_prepare_delegates_to_existing_builder_without_an_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "中文 空格 專案"
            output = root / "dist/dev-runtime"
            with patch.object(dev, "runtime_output", return_value=output), patch.object(dev.ci, "bootstrap", return_value=Path(__file__)), patch.object(dev.portable, "main", return_value=0) as build:
                self.assertEqual(dev.prepare_runtime("windows-x64", root), output)
                command = build.call_args.args[0]
                self.assertEqual(command[:4], ["--platform", "windows-x64", "--output", str(output)])
                self.assertIn("--allow-unresolved-native", command)
                output.mkdir(parents=True)
                build.reset_mock()
                self.assertEqual(dev.prepare_runtime("windows-x64", root), output)
                build.assert_not_called()

    @contextlib.contextmanager
    def bundle(self, name):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory) / "中文 空格 runtime"
            root = dev.bundle_root(base, name)
            root.mkdir(parents=True)
            manifest_root = root if name == "windows-x64" else root.parents[1]
            info = {"platform": name, "babeldoc": "0.6.4", "runtime_python": "3.13.16"}
            if name == "windows-x64":
                info["artifact_kind"] = "babeloxide-windows-x64"
                del info["platform"]
            (root / "BUILD_INFO.json").write_text(json.dumps(info))
            (root / "NATIVE_DEPENDENCIES.json").write_text('{"unresolved": []}')
            files = {"runtime/python/placeholder": b"locked Python",
                     "babeldoc/worker.py": b"locked Worker",
                     "native/pdfium/placeholder": b"locked PDFium",
                     "resources/engine-assets.json": b"{}",
                     "resources/worker-home/placeholder": b"locked font",
                     "engine-assets/placeholder.zip": b"locked archive",
                     "provenance/WHEELS.json": b"[]",
                     "provenance/build-inputs.lock.json": dev.portable.PINS.read_bytes()}
            for filename, data in files.items():
                path = root / filename
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            support.manifest(manifest_root, root / "RELEASE_FILES.json")
            with patch.object(dev.portable, "platform_profile", return_value=([], Path("unused"))), patch.object(dev.windows, "engine_seed_manifest", return_value={}):
                yield root

    def test_both_manifest_layouts_allow_private_state_but_reject_mutated_runtime(self):
        for name in ("windows-x64", "macos-arm64"):
            with self.subTest(name=name), self.bundle(name) as root:
                private = root / "config/config.v3.toml"
                private.parent.mkdir()
                private.write_bytes(b"private state must be retained")
                dev.verify_runtime(root, name)
                self.assertEqual(private.read_bytes(), b"private state must be retained")
                library = root / "native/pdfium/placeholder"
                library.write_bytes(b"tampered")
                with self.assertRaises(BuildError):
                    dev.verify_runtime(root, name)
                self.assertEqual(private.read_bytes(), b"private state must be retained")

    def test_wrong_platform_unlisted_files_missing_files_and_unknown_native_imports_fail(self):
        with self.bundle("windows-x64") as root:
            with self.assertRaisesRegex(BuildError, "platform"):
                dev.verify_runtime(root, "macos-arm64")
            extra = root / "native/pdfium/extra.dll"
            extra.write_bytes(b"unlisted")
            with self.assertRaisesRegex(BuildError, "Missing/unexpected"):
                dev.verify_runtime(root, "windows-x64")
            extra.unlink()
            (root / "native/pdfium/placeholder").unlink()
            with self.assertRaisesRegex(BuildError, "Missing/unexpected"):
                dev.verify_runtime(root, "windows-x64")
        with self.bundle("windows-x64") as root:
            (root / "NATIVE_DEPENDENCIES.json").write_text('{"unresolved": [{"dll": "unknown.dll"}]}')
            with self.assertRaisesRegex(BuildError, "unresolved native"):
                dev.verify_runtime(root, "windows-x64")

    def test_check_missing_runtime_never_downloads_or_launches_gui(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(dev, "ROOT", Path(directory)), patch.object(dev, "host_platform", return_value="windows-x64"), patch.object(dev, "release_binaries", return_value=(Path("cli"), Path("gui"))), patch.object(dev, "runtime_output", return_value=Path(directory) / "missing"), patch.object(dev, "prepare_runtime") as prepare, patch.object(dev.subprocess, "run") as run:
            with self.assertRaisesRegex(BuildError, "--prepare"):
                dev.main(["--check"])
            prepare.assert_not_called()
            run.assert_not_called()

    def test_cli_uses_one_absolute_runtime_and_current_source_worker_and_preserves_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "中文 空格 完整包"
            root.mkdir()
            cli = Path(directory) / "target/release/BabelOxide.exe"
            with patch.object(dev, "ROOT", Path(directory)), patch.object(dev, "host_platform", return_value="windows-x64"), patch.object(dev, "release_binaries", return_value=(cli, Path("gui"))), patch.object(dev, "check_runtime"), patch.object(dev, "prepare_runtime") as prepare, patch.object(dev.subprocess, "run", return_value=subprocess.CompletedProcess([], 7)) as run:
                self.assertEqual(dev.main(["--runtime-dir", str(root), "--cli", "--", "--version"]), 7)
                self.assertEqual(run.call_args.args[0], [cli, "--runtime-dir", str(root.resolve()), "--worker", str(Path(directory) / "python-worker/worker.py"), "--version"])
                prepare.assert_not_called()

    def test_runtime_check_isolated_from_provider_credentials_and_private_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root, work = Path(directory) / "bundle", Path(directory) / "logs"
            work.mkdir()
            with patch.object(dev, "verify_runtime"), patch.dict(os.environ, {"BABELOXIDE_OPENAI_API_KEY": "${SYNTHETIC_TEST_KEY}"}), patch.object(dev.ci, "run") as run:
                dev.check_runtime(root, "windows-x64", Path("cli"), work)
                self.assertEqual(run.call_count, 2)
                for call in run.call_args_list:
                    env = call.args[2]
                    self.assertNotIn("BABELOXIDE_OPENAI_API_KEY", env)
                    self.assertTrue(Path(env["BABELOXIDE_APP_DIR"]).is_relative_to(work))
                self.assertIn("--warmup", run.call_args_list[0].args[0])
                self.assertIn("FPDF_InitLibrary", run.call_args_list[1].args[0][4])

    def test_conflicting_or_mixed_runtime_arguments_are_rejected_before_work(self):
        for arguments in (["--check", "--build"], ["--cli", "--", "--python", "elsewhere"], ["--", "--version"]):
            with self.subTest(arguments=arguments), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                dev.main(arguments)

    @unittest.skipUnless(sys.platform == "darwin", "native macOS executable launcher test")
    def test_mac_executable_launcher_quotes_finder_project_path_and_preserves_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "中文 空格 O'Brien (專案)"
            root.mkdir()
            launcher = root / "啟動開發版.command"
            shutil.copy2(dev.ROOT / launcher.name, launcher)
            home = Path(directory) / "home"
            uv = home / ".local/bin/uv"
            uv.parent.mkdir(parents=True)
            uv.write_text('#!/bin/bash\nprintf "%s\\n" "$@" > "$DEV_TEST_LOG"\nexit 23\n')
            uv.chmod(0o755)
            log = Path(directory) / "arguments.txt"
            env = dict(os.environ, HOME=str(home), DEV_TEST_LOG=str(log))
            result = subprocess.run([launcher, "--check"], cwd=directory, env=env, capture_output=True)
            self.assertEqual(result.returncode, 23)
            physical = root.resolve()  # macOS /var is a system alias of /private/var.
            self.assertEqual(log.read_text().splitlines(), ["run", "--locked", "--no-dev", "--python", "3.13.16", "--project", str(physical), "python", "-B", str(physical / "script/dev.py"), "--check"])


if __name__ == "__main__":
    unittest.main()
