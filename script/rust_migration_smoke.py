"""Translate a trusted PDF with the Rust binary and a bounded loopback fixture.

This is development verification, not part of the shipped Python engine worker.
No real provider or existing user credentials are used.
"""

import argparse
import hashlib
import json
import os
import subprocess
import threading
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Executed by the locked engine interpreter, not the development environment.
# Opening and rasterizing every output catches truncated/invalid PDFs even if
# the CLI reports success. Actual translation quality is verified separately.
PDF_VALIDATION = """
import json,sys
import pymupdf
reports=[]
for filename in sys.argv[1:]:
    with pymupdf.open(filename) as doc:
        assert doc.page_count > 0 and not doc.is_encrypted, 'empty/encrypted output PDF'
        assert not doc.is_repaired, 'output PDF required structural repair'
        characters=images=0
        for page in doc:
            pix=page.get_pixmap(matrix=pymupdf.Matrix(0.5,0.5), colorspace=pymupdf.csRGB)
            assert pix.width > 0 and pix.height > 0 and len(pix.samples) > 0
            characters += len(page.get_text())
            images += len(page.get_images())
        assert characters > 0, 'translated output has no extractable text'
        reports.append({'path':filename,'pages':doc.page_count,'characters':characters,'images':images,'rendered_pages':doc.page_count})
print(json.dumps(reports))
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--label", required=True)
    parser.add_argument("--exe", type=Path, default=ROOT / ("target/debug/BabelOxide.exe" if os.name == "nt" else "target/debug/BabelOxide"))
    parser.add_argument(
        "--python", type=Path, default=ROOT / (".venv/Scripts/python.exe" if os.name == "nt" else ".venv/bin/python")
    )
    parser.add_argument("--worker-home", type=Path)
    parser.add_argument("--runtime-dir", type=Path)
    parser.add_argument("--packaged", action="store_true")
    parser.add_argument(
        "--portable-config",
        action="store_true",
        help="Use the copied bundle's own config/cache",
    )
    parser.add_argument("--isolated-cwd", action="store_true")
    parser.add_argument("--pages")
    parser.add_argument("--auto-extract-glossary", action="store_true")
    parser.add_argument("--http-status", type=int, choices=(200, 400, 500), default=200)
    parser.add_argument("--failure-mode", choices=("none", "batches", "batch-disconnect", "document"), default="none",
                        help="Inject batch HTTP 500 or connection drops after healthy preflight to exercise BabelDOC fallback")
    parser.add_argument(
        "--fixed-text",
        help="Fixed local response for font/output smoke; default is identity",
    )
    parser.add_argument("--lang-out", default="zh-CN")
    parser.add_argument("--debug", action="store_true")
    parser.add_argument("--timeout-seconds", type=int, default=300)
    args = parser.parse_args()
    executable = str(args.exe.absolute())
    if (
        os.name == "nt"
        and len(executable) >= 260
        and not executable.startswith("\\\\?\\")
    ):
        executable = (
            "\\\\?\\UNC\\" + executable[2:]
            if executable.startswith("\\\\")
            else "\\\\?\\" + executable
        )
    if not 1 <= args.timeout_seconds <= 7200:
        parser.error("timeout must be between 1 and 7200 seconds")
    if not args.packaged and args.worker_home is None and args.runtime_dir is None:
        parser.error("development smoke requires --worker-home")
    if not args.source.is_file():
        parser.error("source PDF does not exist or is not a file")
    if not args.label or any(
        char not in "abcdefghijklmnopqrstuvwxyz0123456789-_" for char in args.label
    ):
        parser.error(
            "label must contain only lowercase letters, numbers, hyphens and underscores"
        )
    output = ROOT / ".test-output/rust-migration" / args.label
    output.mkdir(parents=True, exist_ok=False)
    source_sha256 = hashlib.sha256(args.source.read_bytes()).hexdigest()
    calls = []
    synthetic = "synthetic-rust-loopback-only"

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            if self.path != "/v1/chat/completions":
                self.send_error(404)
                return
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 1024 * 1024:
                self.send_error(413)
                return
            body = json.loads(self.rfile.read(length))
            prompt = body["messages"][-1]["content"]
            marker = "Now translate the following text:"
            translated = (
                prompt.rsplit(marker, 1)[-1]
                if marker in prompt
                else prompt.rsplit("Input:\n\n", 1)[-1]
            )
            batch_marker = "## Here is the input:\n\n"
            batch = batch_marker in translated
            glossary = "You are an expert multilingual terminologist." in prompt
            if glossary:
                translated = "[]"
            elif batch:
                items = json.loads(translated.rsplit(batch_marker, 1)[-1])
                translated = json.dumps(
                    [
                        {
                            "id": item["id"],
                            "output": args.fixed_text
                            if args.fixed_text is not None
                            else item["input"],
                        }
                        for item in items
                    ],
                    ensure_ascii=False,
                )
            elif args.fixed_text is not None:
                translated = args.fixed_text
            status = args.http_status
            if ((args.failure_mode == "batches" and batch)
                    or (args.failure_mode == "document" and not glossary and translated.strip() != "Hello")):
                status = 500
            if args.failure_mode == "batch-disconnect" and batch:
                status = None
            calls.append({"batch": batch, "glossary": glossary, "model": body.get("model"), "status": status})
            if status is None:
                self.close_connection = True
                return  # Close the real socket without any HTTP response.
            reply = {
                "id": "local-fixture",
                "object": "chat.completion",
                "created": 0,
                "model": body.get("model"),
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": translated},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {
                    "prompt_tokens": 0,
                    "completion_tokens": 0,
                    "total_tokens": 0,
                },
            }
            if status != 200:
                reply = {"error": {"message": "Local fixture translation service failure"}}
            data = json.dumps(reply).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    environment = {
        key: os.environ[key]
        for key in ("SystemRoot", "WINDIR", "COMSPEC", "PATH")
        if key in os.environ
    }
    environment["BABELOXIDE_OPENAI_API_KEY"] = synthetic
    # The full Rust CLI now loads configuration. Keep the fixture independent
    # of the user's legacy home and application settings.
    isolated = output / "isolation"
    isolated_home = isolated / "home"
    isolated_temp = isolated / "tmp"
    isolated_app = isolated / "app"
    for directory in (isolated_home, isolated_temp, isolated_app):
        directory.mkdir(parents=True)
    for directory in ("Desktop", "Documents", "Downloads"):
        (isolated_home / directory).mkdir()
    child_cwd = isolated / "working-directory" if args.isolated_cwd else ROOT
    if args.isolated_cwd:
        child_cwd.mkdir()
    environment["HOME"] = environment["USERPROFILE"] = str(isolated_home)
    environment["TEMP"] = environment["TMP"] = str(isolated_temp)
    if not args.portable_config:
        environment["BABELOXIDE_APP_DIR"] = str(isolated_app)
    environment["NO_PROXY"] = environment["no_proxy"] = "127.0.0.1,localhost"
    command = [
        executable,
        str(args.source.resolve()),
        "--output",
        str(output),
        "--lang-in",
        "en",
        "--lang-out",
        args.lang_out,
        "--openai",
        "--openai-base-url",
        f"http://127.0.0.1:{server.server_port}/v1",
        "--openai-model",
        "offline-identity",
        "--ignore-cache",
        "--qps",
        "200",
        "--pool-max-workers",
        "16",
        "--watermark-output-mode",
        "no_watermark",
    ]
    if not args.auto_extract_glossary:
        command.append("--no-auto-extract-glossary")
    if args.runtime_dir is not None:
        command.extend(["--runtime-dir", str(args.runtime_dir.resolve()), "--worker", str(ROOT / "python-worker/worker.py")])
        args.python = args.runtime_dir / ("runtime/python/python.exe" if os.name == "nt" else "runtime/python/bin/python3")
    elif not args.packaged:
        command.extend(
            [
                "--python",
                str(args.python.resolve()),
                "--worker",
                str(ROOT / "python-worker/worker.py"),
            ]
        )
    if args.worker_home is not None:
        command.extend(["--worker-home", str(args.worker_home.resolve())])
    command.append("--json")
    if args.pages:
        command.extend(["--pages", args.pages])
    if args.debug:
        command.append("--debug")
    try:
        with (
            (output / "stdout.jsonl").open("wb") as stdout_file,
            (output / "stderr.txt").open("wb") as stderr_file,
        ):
            completed = subprocess.run(
                command,
                executable=executable,
                cwd=child_cwd,
                env=environment,
                stdout=stdout_file,
                stderr=stderr_file,
                timeout=args.timeout_seconds,
            )
    except subprocess.TimeoutExpired:
        completed = subprocess.CompletedProcess(command, 124)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    stdout = (output / "stdout.jsonl").read_text(encoding="utf-8", errors="replace")
    stderr = (output / "stderr.txt").read_text(encoding="utf-8", errors="replace")
    events = [json.loads(line) for line in stdout.splitlines() if line.strip()]
    record = {
        "exit_code": completed.returncode,
        "timed_out": completed.returncode == 124,
        "source_sha256": source_sha256,
        "source_unchanged": source_sha256 == hashlib.sha256(args.source.read_bytes()).hexdigest(),
        "api_requests": len(calls),
        "batch_requests": sum(call["batch"] for call in calls),
        "glossary_requests": sum(call["glossary"] for call in calls),
        "http_statuses": sorted({call["status"] for call in calls if call["status"] is not None}),
        "transport_disconnects": sum(call["status"] is None for call in calls),
        "pages": args.pages or "all",
        "failure_mode": args.failure_mode,
        "provider": "loopback fixed-text fixture"
        if args.fixed_text is not None
        else "loopback identity fixture",
        "response_mode": "fixed text" if args.fixed_text is not None else "identity",
        "executable_argument": executable,
        "worker_runtime": "bundle defaults"
        if args.packaged
        else "explicit development paths",
        "working_directory": "isolated non-source directory"
        if args.isolated_cwd
        else "repository root",
        "contains_synthetic_key": synthetic in stdout + stderr,
        "progress_events": sum(
            event.get("type", "").startswith("progress") for event in events
        ),
        "terminal_events": [
            event
            for event in events
            if event.get("type") in {"finish", "error", "cancelled"}
        ],
        "pdfs": [
            {"name": path.name, "bytes": path.stat().st_size}
            for path in output.glob("*.pdf")
        ],
    }
    record["pdf_validation"] = []
    if completed.returncode == 0:
        try:
            finishes = [event for event in events if event.get("type") == "finish"]
            if len(finishes) != 1 or not calls:
                raise ValueError("Successful smoke requires one finish event and actual API requests")
            if finishes[0]["result"]["translation_metrics"]["main"]["calls"] <= 1:
                raise ValueError("Successful smoke requires document translation beyond the API health probe")
            audit = finishes[0]["result"].get("translation_audit", {})
            if args.failure_mode in {"batches", "batch-disconnect"} and not (
                any(call["status"] in {500, None} and call["batch"] for call in calls)
                and audit.get("batch_errors", 0) > 0
                and audit.get("required_paragraphs", 0) == audit.get("completed_paragraphs")
            ):
                raise ValueError("Batch failure smoke did not prove complete BabelDOC paragraph fallback")
            paths = []
            for key in ("mono_pdf_path", "dual_pdf_path"):
                path = Path(finishes[0]["result"][key]).resolve(strict=True)
                # samefile also handles the Windows extended-length namespace
                # used by Rust canonicalize (\\?\C:\...) without string aliases.
                if not path.is_file() or not any(parent.samefile(output) for parent in path.parents):
                    raise ValueError("PDF output escaped the isolated test directory")
                paths.append(str(path))
            if args.packaged:
                runtime = args.exe.resolve().parent / "runtime/python"
                python = runtime / ("python.exe" if os.name == "nt" else "bin/python3")
            else:
                python = args.python.resolve()
            validation = subprocess.run(
                [str(python), "-I", "-B", "-c", PDF_VALIDATION, *paths],
                cwd=child_cwd, env=environment, capture_output=True, timeout=120,
            )
            (output / "pdf-validation.stderr.txt").write_bytes(validation.stderr)
            if validation.returncode:
                raise ValueError("Output PDF parse/render validation failed; see pdf-validation.stderr.txt")
            record["pdf_validation"] = json.loads(validation.stdout)
            if args.auto_extract_glossary and not record["glossary_requests"]:
                raise ValueError("Automatic glossary extraction made no term requests")
        except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
            record["validation_error"] = str(error)
    valid = not record["contains_synthetic_key"] and record["source_unchanged"] and "validation_error" not in record
    record["smoke_exit_code"] = completed.returncode or (0 if valid else 1)
    (output / "summary.json").write_text(
        json.dumps(record, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(json.dumps(record, indent=2, ensure_ascii=False))
    return record["smoke_exit_code"]


if __name__ == "__main__":
    raise SystemExit(main())
