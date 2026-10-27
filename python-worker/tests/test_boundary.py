"""Engine-boundary contracts without importing the legacy application."""

import concurrent.futures
import importlib.util
import io
import json
import threading
import time
from pathlib import Path

import pytest

SPEC = importlib.util.spec_from_file_location(
    "babeldoc_worker", Path(__file__).resolve().parents[1] / "worker.py"
)
worker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(worker)


@pytest.mark.parametrize(
    "raw",
    [
        b"{}\n",
        b"[]\n",
        b'{"v":2}\n',
        b'{"v":1}',
        b"\xff\n",
        b"x" * (worker.MAX_FRAME_BYTES + 1),
    ],
    ids=["empty", "non-object", "version", "partial", "utf8", "oversize"],
)
def test_invalid_frame_is_rejected(raw):
    with pytest.raises(worker.ProtocolError):
        worker.read_frame(io.BytesIO(raw))


def test_unicode_and_eof_frames():
    stream = io.BytesIO('{"v":1,"text":"測試"}\n'.encode())
    assert worker.read_frame(stream)["text"] == "測試"
    assert worker.read_frame(stream) is None


def wait_for_requests(boundary, count):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        with boundary.pending_lock:
            if len(boundary.pending) == count:
                return list(boundary.pending)
        threading.Event().wait(0.001)
    raise AssertionError("Translation requests did not arrive")


def test_parallel_replies_may_arrive_in_reverse_order():
    output = io.BytesIO()
    boundary = worker.EngineBoundary(output)
    boundary.started = True
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        try:
            first = executor.submit(boundary.request, "main", "translate", "first")
            second = executor.submit(boundary.request, "main", "translate", "second")
            wait_for_requests(boundary, 2)
            messages = [json.loads(line) for line in output.getvalue().splitlines()]
            for message in reversed(messages):
                boundary.respond({"id": message["id"], "text": message["text"].upper()})
            assert first.result(timeout=2) == "FIRST"
            assert second.result(timeout=2) == "SECOND"
            assert not boundary.pending
        finally:
            boundary.cancel()


def test_cancellation_releases_a_blocked_synchronous_callback():
    boundary = worker.EngineBoundary(io.BytesIO())
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(boundary.request, "main", "translate", "pending")
        try:
            wait_for_requests(boundary, 1)
            boundary.cancel()
            with pytest.raises(concurrent.futures.CancelledError):
                future.result(timeout=2)
            assert not boundary.pending
        finally:
            boundary.cancel()


def test_rust_failure_is_fatal_even_if_babeldoc_would_fallback():
    boundary = worker.EngineBoundary(io.BytesIO())
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(boundary.request, "main", "translate", "pending")
        try:
            request_id = wait_for_requests(boundary, 1)[0]
            boundary.respond(
                {"id": request_id, "error": {"code": "translation_failed"}}
            )
            with pytest.raises(RuntimeError):
                future.result(timeout=2)
            assert boundary.failed.is_set()
            assert boundary.cancelled.is_set()
        finally:
            boundary.cancel()


def test_duplicate_and_unknown_response_ids_are_protocol_failures():
    boundary = worker.EngineBoundary(io.BytesIO())
    with pytest.raises(worker.ProtocolError):
        boundary.respond({"id": "unknown", "text": "bad"})


def test_capability_probe_and_placeholders_need_no_rust_request():
    boundary = worker.EngineBoundary(io.BytesIO())
    adapter = worker.RustTranslatorAdapter(
        boundary, {"name": "openai", "llm": True}, {"lang_in": "en", "lang_out": "zh"}
    )
    assert adapter.do_llm_translate(None) is None
    assert adapter.get_formular_placeholder(7)[0] == "{v7}"
    assert adapter.get_rich_text_left_placeholder(7)[0] == "<style id='7'>"
    assert adapter.get_rich_text_right_placeholder(7)[0] == "</style>"
    assert boundary.output.getvalue() == b""


def test_session_mismatch_does_not_start_the_engine():
    boundary = worker.EngineBoundary(io.BytesIO())
    with pytest.raises(worker.ProtocolError):
        boundary.handle({"type": "start", "session": "wrong"})
    assert not boundary.started


def test_partial_pipe_writes_are_completed():
    class PartialWriter(io.BytesIO):
        def write(self, value):
            return super().write(value[:3])

    output = PartialWriter()
    boundary = worker.EngineBoundary(output)
    boundary.send("ready")
    assert json.loads(output.getvalue())["type"] == "ready"
