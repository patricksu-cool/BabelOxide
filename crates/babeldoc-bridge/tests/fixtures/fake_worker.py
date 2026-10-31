"""Transport/process fixture only: no BabelDOC, provider SDK, or network calls.

The Rust test writes scenario.json beside a copy of this script. Descendants
inherit stdout deliberately, reproducing a process which exits before pipe EOF.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SESSION = "0123456789abcdef0123456789abcdef"
MAX_FRAME_BYTES = 1024 * 1024


def send(kind, **fields):
    value = {"v": 1, "type": kind, "session": SESSION, **fields}
    if kind != "ready":
        value["task_id"] = "job-1"
    raw = (json.dumps(value) + "\n").encode("utf-8")
    view = memoryview(raw)
    while view:
        count = sys.stdout.buffer.write(view)
        if not count:
            raise BrokenPipeError("Fixture short write")
        view = view[count:]
    sys.stdout.buffer.flush()


def wait_for_file(path):
    deadline = time.monotonic() + 15
    while not path.exists():
        if time.monotonic() >= deadline:
            raise TimeoutError(f"Fixture did not receive {path.name}")
        time.sleep(0.01)


def hang():
    # Intentionally never read cancel/shutdown from stdin.
    while True:
        time.sleep(1)


def publish_pid(name):
    # Existence is the readiness gate: never expose a created-but-empty file.
    path = ROOT / name
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(str(os.getpid()), encoding="ascii")
    temporary.replace(path)


def spawn_descendant():
    # No CREATE_BREAKAWAY_FROM_JOB: the bridge must contain this process too.
    subprocess.Popen([sys.executable, "-I", "-u", str(Path(__file__).resolve()), "--descendant"])
    wait_for_file(ROOT / "descendant.pid")


def finish(job, scenario, **metrics):
    output = Path(job["output_dir"])
    result = {}
    for name in ("mono", "dual"):
        if scenario == f"missing_{name}":
            continue
        path = output / f"fixture-{name}.pdf"
        # The fixture tests artifact-path contracts, not PDF format/rendering.
        path.write_bytes(b"%PDF-1.4\n% transport fixture only\n%%EOF\n")
        result[f"{name}_pdf_path"] = str(path)
    result.update(metrics)
    send("finished", result=result)


def finish_assets(start, scenario):
    if "job" in start or "translator" in start:
        raise RuntimeError("Maintenance must not receive PDF or translator options")
    operation = start["operation"]
    tag = "a" * 64
    name = f"offline_assets_{tag}.zip"
    if operation == "generate_offline_assets" and scenario != "asset_missing":
        (Path(start["path"]) / name).write_bytes(b"complete controlled asset archive")
    if scenario == "asset_error":
        send("error", code="engine_failed")
        return
    if scenario == "asset_hang":
        spawn_descendant()
        send("progress", event={"stage": "assets blocked"})
        hang()
    if scenario == "asset_progress_forever":
        while True:
            send("progress", event={"stage": "still busy"})
            time.sleep(0.01)
    if scenario == "asset_translation":
        send("translate", id="w-1", text="must reject", method="translate")
        hang()
    result = {"kind": operation, "tag": "bad" if scenario == "asset_bad_tag" else tag}
    if operation == "generate_offline_assets":
        result["archive_name"] = "../outside.zip" if scenario == "asset_escape" else name
    send("finished", result=result)
    if scenario == "asset_nonzero":
        os._exit(23)


def main():
    if "--descendant" in sys.argv:
        publish_pid("descendant.pid")
        hang()

    scenario = json.loads((ROOT / "scenario.json").read_text(encoding="utf-8"))["scenario"]
    publish_pid("worker.pid")
    with (ROOT / "worker-starts.log").open("a", encoding="ascii") as starts:
        starts.write(f"{os.getpid()}\n")
    if scenario == "startup_crash":
        os._exit(23)
    if scenario == "startup_hang":
        hang()

    send("ready", babeldoc_version="0.6.3", operations=[
        "pdf", "warmup", "generate_offline_assets", "restore_offline_assets"
    ])
    start = json.loads(sys.stdin.buffer.readline())
    if start.get("type") != "start" or start.get("session") != SESSION:
        raise RuntimeError("Fixture expected a valid start frame")
    if start.get("operation", "pdf") != "pdf":
        finish_assets(start, scenario)
        return 0
    if scenario == "runtime_flags":
        finish(start["job"], scenario, runtime_flags={
            "isolated": sys.flags.isolated,
            "ignore_environment": sys.flags.ignore_environment,
            "no_user_site": sys.flags.no_user_site,
            "dont_write_bytecode": sys.flags.dont_write_bytecode,
        })
        return 0
    if scenario == "progress_flood":
        for number in range(2000):
            send("progress", event={"stage": "progress flood", "number": number})
        finish(start["job"], scenario)
        return 0
    if scenario in {"translation_routing", "translation_shared", "request_capacity"}:
        count = 200 if scenario == "request_capacity" else 2
        for number in range(count):
            send(
                "translate", id=f"w-{number}", method="llm_translate",
                text=f"fixture text {number}", ignore_cache=False,
                rate_limit_params=None,
                translator="term" if number % 2 else "main",
            )
        translated = {}
        while len(translated) < count:
            reply = json.loads(sys.stdin.buffer.readline())
            if reply.get("type") != "translated" or "error" in reply:
                raise RuntimeError("Fixture expected a successful translation")
            translated[reply["id"]] = reply["text"]
        finish(
            start["job"], scenario, translations=translated,
            received_capacity=start["max_outstanding"], received_job=start["job"],
        )
        return 0
    if scenario in {"cli_pending", "cache_pending"}:
        send(
            "translate", id="w-cli", method="translate", text="offline CLI fixture",
            ignore_cache=scenario == "cli_pending", rate_limit_params=None, translator="main",
        )
        send("progress", event={"stage": "cli_requested"})
        hang()
    if scenario == "exit_without_terminal":
        return 0
    if scenario in {"finished_then_exit", "missing_mono", "missing_dual"}:
        finish(start["job"], scenario)
        return 0
    if scenario == "malformed":
        sys.stdout.buffer.write(b"{not-json}\n")
        sys.stdout.buffer.flush()
        return 0
    if scenario == "partial_eof":
        sys.stdout.buffer.write(b'{"v":1,"type":')
        sys.stdout.buffer.flush()
        return 0
    if scenario == "oversize":
        sys.stdout.buffer.write(b"x" * (MAX_FRAME_BYTES + 1) + b"\n")
        sys.stdout.buffer.flush()
        return 0
    if scenario == "wrong_session":
        send("progress", session="f" * 32, event={"stage": "invalid"})
        hang()
    if scenario == "wrong_task":
        value = {"v": 1, "type": "progress", "session": SESSION,
                 "task_id": "other-job", "event": {"stage": "invalid"}}
        sys.stdout.buffer.write((json.dumps(value) + "\n").encode("utf-8"))
        sys.stdout.buffer.flush()
        hang()
    if scenario == "crash_with_descendant":
        spawn_descendant()
        send("progress", event={"stage": "descendant_ready"})
        # The test obtains stable OS process handles before allowing the crash.
        wait_for_file(ROOT / "release-worker")
        os._exit(23)
    if scenario == "cancel_hang_with_descendant":
        spawn_descendant()
        send("progress", event={"stage": "descendant_ready"})
        hang()
    if scenario == "cancel_hang":
        send("progress", event={"stage": "worker_ready"})
        hang()
    raise RuntimeError("Unknown fixture scenario")


if __name__ == "__main__":
    raise SystemExit(main())
