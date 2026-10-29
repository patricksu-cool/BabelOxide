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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--label", required=True)
    parser.add_argument("--exe", type=Path, default=ROOT / "target/debug/pdf2zh.exe")
    parser.add_argument(
        "--python", type=Path, default=ROOT / ".venv/Scripts/python.exe"
    )
    parser.add_argument("--worker-home", type=Path, required=True)
    parser.add_argument("--pages")
    parser.add_argument("--debug", action="store_true")
    args = parser.parse_args()
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
            if batch:
                items = json.loads(translated.rsplit(batch_marker, 1)[-1])
                translated = json.dumps(
                    [{"id": item["id"], "output": item["input"]} for item in items],
                    ensure_ascii=False,
                )
            calls.append({"batch": batch, "model": body.get("model")})
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
            data = json.dumps(reply).encode()
            self.send_response(200)
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
        key: value for key, value in os.environ.items() if not key.startswith("PDF2ZH_")
    }
    environment["PDF2ZH_OPENAI_API_KEY"] = synthetic
    environment["NO_PROXY"] = environment["no_proxy"] = "127.0.0.1,localhost"
    command = [
        str(args.exe.resolve()),
        str(args.source.resolve()),
        "--output",
        str(output),
        "--lang-in",
        "en",
        "--lang-out",
        "zh-CN",
        "--openai",
        "--openai-base-url",
        f"http://127.0.0.1:{server.server_port}/v1",
        "--openai-model",
        "offline-identity",
        "--no-auto-extract-glossary",
        "--ignore-cache",
        "--qps",
        "200",
        "--pool-max-workers",
        "16",
        "--watermark-output-mode",
        "no_watermark",
        "--python",
        str(args.python.resolve()),
        "--worker",
        str(ROOT / "python-worker/worker.py"),
        "--worker-home",
        str(args.worker_home.resolve()),
        "--json",
    ]
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
                cwd=ROOT,
                env=environment,
                stdout=stdout_file,
                stderr=stderr_file,
                timeout=300,
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
        "source_sha256": hashlib.sha256(args.source.read_bytes()).hexdigest(),
        "api_requests": len(calls),
        "batch_requests": sum(call["batch"] for call in calls),
        "pages": args.pages or "all",
        "provider": "loopback identity fixture",
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
    (output / "summary.json").write_text(
        json.dumps(record, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(json.dumps(record, indent=2, ensure_ascii=False))
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
