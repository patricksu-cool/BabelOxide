"""Launch a Release GUI against a controllable, local identity API for desktop QA.

This is developer tooling, never shipped with the application. The control file
accepts mode=identity/error/slow and action=reopen/stop. No real API is used.
"""

import argparse
import hashlib
import json
import os
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("exe", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--portable-config",
        action="store_true",
        help="Use the copied bundle's own config/cache instead of a test override",
    )
    parser.add_argument("--timeout", type=int, default=2400)
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
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    control = output / "control.json"
    control.write_text('{"mode":"identity"}', encoding="utf-8")
    home, temporary, app, cwd = (
        output / name for name in ("home", "tmp", "app", "cwd")
    )
    for directory in (home, temporary, app, cwd):
        directory.mkdir()
    for name in ("Desktop", "Documents", "Downloads"):
        (home / name).mkdir()
    calls = []
    exits = []

    def state():
        try:
            return json.loads(control.read_text(encoding="utf-8-sig"))
        except (OSError, ValueError):
            return {}

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            size = int(self.headers.get("Content-Length", "0"))
            if self.path != "/v1/chat/completions" or not 0 < size <= 1024 * 1024:
                self.send_error(400)
                return
            body = json.loads(self.rfile.read(size))
            mode = state().get("mode", "identity")
            prompt = body["messages"][-1]["content"]
            marker = "Now translate the following text:"
            result = (
                prompt.rsplit(marker, 1)[-1]
                if marker in prompt
                else prompt.rsplit("Input:\n\n", 1)[-1]
            )
            batch_marker = "## Here is the input:\n\n"
            batch = batch_marker in result
            if batch:
                items = json.loads(result.rsplit(batch_marker, 1)[-1])
                result = json.dumps(
                    [{"id": item["id"], "output": item["input"]} for item in items],
                    ensure_ascii=False,
                )
            calls.append({"mode": mode, "batch": batch, "model": body.get("model")})
            if mode == "slow":
                time.sleep(15)
            status = 400 if mode == "error" else 200
            reply = (
                {"error": {"message": "Local fixture rejected request"}}
                if status != 200
                else {
                    "id": "local-gui-fixture",
                    "object": "chat.completion",
                    "created": 0,
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": result},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 0,
                        "completion_tokens": 0,
                        "total_tokens": 0,
                    },
                }
            )
            data = json.dumps(reply).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            try:
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    # Allow only OS/runtime variables and explicit synthetic test configuration.
    environment = {
        key: os.environ[key]
        for key in ("SystemRoot", "WINDIR", "COMSPEC", "PATH")
        if key in os.environ
    }
    environment.update(
        HOME=str(home),
        USERPROFILE=str(home),
        TEMP=str(temporary),
        TMP=str(temporary),
        PDF2ZH_APP_DIR=str(app),
        NO_PROXY="127.0.0.1,localhost",
        no_proxy="127.0.0.1,localhost",
    )
    if args.portable_config:
        environment.pop("PDF2ZH_APP_DIR", None)
    args_first = [
        executable,
        "--openai",
        "--openai-base-url",
        f"http://127.0.0.1:{server.server_port}/v1",
        "--openai-api-key",
        "synthetic-gui-local-only",
        "--openai-model",
        "offline-identity",
        "--lang-in",
        "en",
        "--lang-out",
        "zh-CN",
        "--output",
        str(output),
        "--no-auto-extract-glossary",
        "--ignore-cache",
        "--qps",
        "200",
        "--pool-max-workers",
        "16",
        "--watermark-output-mode",
        "no_watermark",
    ]
    started = time.monotonic()
    process = None
    handles = []

    def launch(command):
        log = (output / f"gui-{len(handles)}.log").open("wb")
        handles.append(log)
        return subprocess.Popen(
            command,
            executable=executable,
            cwd=cwd,
            env=environment,
            stdout=log,
            stderr=log,
        )

    try:
        process = launch(args_first)
        print(
            json.dumps(
                {
                    "pid": process.pid,
                    "control": str(control),
                    "port": server.server_port,
                }
            ),
            flush=True,
        )
        while time.monotonic() - started < args.timeout:
            requested = state()
            if process is not None and process.poll() is not None:
                exits.append(process.returncode)
                process = None
            if requested.get("action") == "stop":
                break
            if requested.get("action") == "reopen" and process is None:
                process = launch([executable])
                control.write_text(
                    json.dumps({"mode": requested.get("mode", "identity")}),
                    encoding="utf-8",
                )
                print(json.dumps({"reopened_pid": process.pid}), flush=True)
            time.sleep(0.2)
    finally:
        if process is not None:
            if process.poll() is None:
                # Harness timeout cleanup is reported separately from GUI exit.
                process.terminate()
                process.wait(timeout=20)
                exits.append("harness-terminated")
            else:
                exits.append(process.returncode)
        server.shutdown()
        server.server_close()
        for handle in handles:
            handle.close()
        summary = {
            "exe_sha256": hashlib.sha256(Path(executable).read_bytes()).hexdigest(),
            "api_requests": len(calls),
            "batch_requests": sum(call["batch"] for call in calls),
            "modes": {
                mode: sum(call["mode"] == mode for call in calls)
                for mode in ("identity", "error", "slow")
            },
            "exits": exits,
            "pdfs": [path.name for path in output.glob("*.pdf")],
            "synthetic_key_in_logs": any(
                "synthetic-gui-" in p.read_text(encoding="utf-8", errors="replace")
                for p in output.glob("*.log")
            ),
        }
        (output / "summary.json").write_text(
            json.dumps(summary, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    main()
