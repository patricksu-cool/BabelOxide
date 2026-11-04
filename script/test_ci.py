"""CI failure diagnostics, using synthetic subprocesses without Cargo or network."""
import io
import os
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

import ci


class CIDiagnosticsTests(unittest.TestCase):
    def test_success_keeps_stdout_and_stderr_in_log_without_failure_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            console = io.StringIO()
            command = [sys.executable, "-u", "-c", "import sys; print('stdout'); print('stderr', file=sys.stderr)"]
            with redirect_stdout(io.StringIO()), redirect_stderr(console):
                ci.run(command, root, dict(os.environ), label="success")
            self.assertEqual((root / "success.log").read_text(), "stdout\nstderr\n")
            self.assertEqual(console.getvalue(), "")

    def test_failure_prints_last_100_lines_and_preserves_complete_log_and_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            console = io.StringIO()
            code = "import sys; [print(f'line-{n:03d}') for n in range(130)]; print('child-stderr', file=sys.stderr); sys.exit(101)"
            with redirect_stdout(io.StringIO()), redirect_stderr(console):
                with self.assertRaises(subprocess.CalledProcessError) as raised:
                    ci.run([sys.executable, "-u", "-c", code], root, dict(os.environ), label="rust-tests")
            self.assertEqual(raised.exception.returncode, 101)
            original = (root / "rust-tests.log").read_text().splitlines()
            self.assertEqual(original, [f"line-{n:03d}" for n in range(130)] + ["child-stderr"])
            displayed = console.getvalue().splitlines()
            self.assertEqual(displayed[2:-1], original[-100:])
            self.assertNotIn("line-000", console.getvalue())
            self.assertIn(str(root / "rust-tests.log"), console.getvalue())

    def test_short_non_utf8_log_and_missing_final_newline_are_displayed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            console = io.StringIO()
            code = "import sys; sys.stdout.buffer.write(b'valid\\n\\xfftail'); sys.exit(7)"
            with redirect_stdout(io.StringIO()), redirect_stderr(console):
                with self.assertRaises(subprocess.CalledProcessError) as raised:
                    ci.run([sys.executable, "-u", "-c", code], root, dict(os.environ), label="encoding")
            self.assertEqual(raised.exception.returncode, 7)
            self.assertEqual((root / "encoding.log").read_bytes(), b"valid\n\xfftail")
            self.assertIn("valid\n\ufffdtail\n--- End", console.getvalue())

    def test_timeout_prints_partial_log_and_reraises_original_exception(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            console = io.StringIO()
            failure = subprocess.TimeoutExpired(["synthetic"], 3)

            def time_out(*args, **kwargs):
                kwargs["stdout"].write(b"partial output\n")
                raise failure

            with patch.object(ci.subprocess, "run", side_effect=time_out):
                with redirect_stdout(io.StringIO()), redirect_stderr(console):
                    with self.assertRaises(subprocess.TimeoutExpired) as raised:
                        ci.run(["synthetic"], root, dict(os.environ), timeout=3, label="timeout")
            self.assertIs(raised.exception, failure)
            self.assertIn("partial output", console.getvalue())

    def test_console_failure_does_not_replace_command_failure(self):
        class BrokenConsole(io.StringIO):
            def write(self, text):
                raise OSError("synthetic closed console")

        with tempfile.TemporaryDirectory() as directory:
            with redirect_stdout(io.StringIO()), redirect_stderr(BrokenConsole()):
                with self.assertRaises(subprocess.CalledProcessError) as raised:
                    ci.run([sys.executable, "-c", "raise SystemExit(23)"], Path(directory), dict(os.environ), label="broken-console")
                with patch.object(ci, "main", side_effect=raised.exception):
                    self.assertEqual(ci.cli(), 23)
            self.assertEqual(raised.exception.returncode, 23)

    def test_missing_log_reports_diagnostic_without_raising(self):
        with tempfile.TemporaryDirectory() as directory:
            console = io.StringIO()
            with redirect_stderr(console):
                ci.print_log_tail(Path(directory) / "missing.log")
            self.assertIn("Cannot display log", console.getvalue())

    def test_cli_preserves_exit_codes_and_maps_signals(self):
        for child_status, expected in ((1, 1), (17, 17), (101, 101), (-15, 143)):
            with self.subTest(status=child_status):
                with patch.object(ci, "main", side_effect=subprocess.CalledProcessError(child_status, ["synthetic"])):
                    with redirect_stderr(io.StringIO()):
                        self.assertEqual(ci.cli(), expected)
        for failure in (ci.BuildError("synthetic"), OSError("synthetic"), subprocess.TimeoutExpired(["synthetic"], 1)):
            with self.subTest(error=type(failure).__name__):
                with patch.object(ci, "main", side_effect=failure), redirect_stderr(io.StringIO()):
                    self.assertEqual(ci.cli(), 1)
        with patch.object(ci, "main"), redirect_stderr(io.StringIO()):
            self.assertEqual(ci.cli(), 0)

    def test_check_stops_at_failed_rust_tests(self):
        labels = []
        failure = subprocess.CalledProcessError(101, ["cargo", "test"])

        def run_step(*args, **kwargs):
            labels.append(kwargs["label"])
            if kwargs["label"] == "rust-tests":
                raise failure

        with patch.object(ci, "run", side_effect=run_step):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                ci.check(Path("uv"), Path("synthetic-work"), {})
        self.assertIs(raised.exception, failure)
        self.assertEqual(labels, ["dev-sync", "rustfmt", "clippy", "rust-tests"])

    def test_real_ci_entrypoint_exits_with_child_status_after_displaying_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            child = "import sys; print('synthetic failure detail'); sys.exit(17)"
            code = (
                "import os,sys\n"
                f"sys.path.insert(0, {str(Path(ci.__file__).parent)!r})\n"
                "import ci\nfrom pathlib import Path\n"
                "def fail():\n"
                f"    ci.run([sys.executable, '-u', '-c', {child!r}], Path({str(root)!r}), dict(os.environ), label='rust-tests')\n"
                "ci.main = fail\nraise SystemExit(ci.cli())\n"
            )
            result = subprocess.run([sys.executable, "-B", "-c", code], env={**os.environ, "PYTHONUTF8": "1"},
                                    capture_output=True, text=True, encoding="utf-8", timeout=15)
            self.assertEqual(result.returncode, 17)
            self.assertIn("synthetic failure detail", result.stderr)
            self.assertIn("Last 100 lines", result.stderr)
            self.assertEqual((root / "rust-tests.log").read_text(), "synthetic failure detail\n")


if __name__ == "__main__":
    unittest.main()
