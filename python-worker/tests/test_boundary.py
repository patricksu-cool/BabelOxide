"""Engine-boundary contracts without importing the legacy application."""

import asyncio
import concurrent.futures
import importlib.util
import io
import json
import sys
import threading
import time
from pathlib import Path
from types import SimpleNamespace

import pytest

SPEC = importlib.util.spec_from_file_location(
    "babeldoc_worker", Path(__file__).resolve().parents[1] / "worker.py"
)
worker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(worker)


@pytest.mark.parametrize(
    "platform,path_length,expected",
    [("win32", 300, [1]), ("win32", 100, []), ("linux", 300, [])],
)
def test_native_parallel_workaround_only_applies_to_long_windows_runtime(
    monkeypatch, platform, path_length, expected
):
    threads = []
    monkeypatch.setattr(worker.sys, "platform", platform)
    monkeypatch.setattr(worker.os.path, "realpath", lambda _: "x" * path_length)
    monkeypatch.setitem(sys.modules, "cv2", SimpleNamespace(setNumThreads=threads.append))
    worker.configure_native_runtime()
    assert threads == expected


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


@pytest.mark.parametrize("operation", ["warmup", "generate_offline_assets", "restore_offline_assets"])
def test_asset_api_receipts_need_no_translator(monkeypatch, tmp_path, operation):
    boundary = worker.EngineBoundary(io.BytesIO())
    calls = []
    assets = SimpleNamespace(
        warmup=lambda: calls.append(("warmup", None)),
        generate_offline_assets_package=lambda path: calls.append(("generate_offline_assets", path)),
        restore_offline_assets_package=lambda path: calls.append(("restore_offline_assets", path)),
        get_offline_assets_tag=lambda: "a" * 64,
    )
    monkeypatch.setitem(sys.modules, "babeldoc.assets", SimpleNamespace(assets=assets))
    path = None if operation == "warmup" else str(tmp_path)
    result = boundary.run_assets(operation, path)
    assert calls == [(operation, None if path is None else tmp_path.resolve())]
    assert result["kind"] == operation and result["tag"] == "a" * 64
    if operation == "generate_offline_assets":
        assert result["archive_name"] == f"offline_assets_{'a' * 64}.zip"
    assert not boundary.pending


def test_asset_api_return_after_cancel_cannot_report_success(monkeypatch):
    boundary = worker.EngineBoundary(io.BytesIO())
    assets = SimpleNamespace(warmup=boundary.cancel)
    monkeypatch.setitem(sys.modules, "babeldoc.assets", SimpleNamespace(assets=assets))
    with pytest.raises(concurrent.futures.CancelledError):
        boundary.run_assets("warmup", None)


def test_asset_system_exit_is_a_safe_terminal_error(monkeypatch):
    boundary = worker.EngineBoundary(io.BytesIO())

    def fail():
        raise SystemExit("sensitive engine diagnostic")

    monkeypatch.setitem(sys.modules, "babeldoc.assets", SimpleNamespace(assets=SimpleNamespace(warmup=fail)))
    boundary.start({"task_id": "job-1", "operation": "warmup"})
    boundary.task.join(timeout=5)
    assert boundary.terminal
    messages = [json.loads(line) for line in boundary.output.getvalue().splitlines()]
    assert messages[-1]["type"] == "error"
    assert messages[-1]["code"] == "engine_failed"
    assert not any(message["type"] == "finished" for message in messages)
    assert b"sensitive engine diagnostic" not in boundary.output.getvalue()


def test_unknown_operation_does_not_start_engine():
    boundary = worker.EngineBoundary(io.BytesIO())
    with pytest.raises(worker.ProtocolError):
        boundary.start({"task_id": "job-1", "operation": "arbitrary-import"})
    assert not boundary.started


def test_partial_pipe_writes_are_completed():
    class PartialWriter(io.BytesIO):
        def write(self, value):
            return super().write(value[:3])

    output = PartialWriter()
    boundary = worker.EngineBoundary(output)
    boundary.send("ready")
    assert json.loads(output.getvalue())["type"] == "ready"


@pytest.mark.parametrize("capacity", [None, True, False, 0, -1, 1_000_001, "128", 1.5])
def test_invalid_capacity_never_starts_an_engine_thread(capacity):
    boundary = worker.EngineBoundary(io.BytesIO())
    with pytest.raises(worker.ProtocolError, match="capacity"):
        boundary.start({"task_id": "job-1", "max_outstanding": capacity})
    assert not boundary.started
    assert boundary.task is None


def test_term_adapter_forwards_role_and_cache_options():
    calls = []

    class Boundary:
        def request(self, *args):
            calls.append(args)
            return "譯文"

    adapter = worker.RustTranslatorAdapter(
        Boundary(),
        {"name": "openai", "llm": True},
        {"lang_in": "en", "lang_out": "zh"},
        role="term",
    )
    assert adapter.llm_translate("prompt", True, {"tokens": 7}) == "譯文"
    assert calls == [("term", "llm_translate", "prompt", True, {"tokens": 7})]


def test_engine_options_glossary_split_and_metrics_stay_at_engine_boundary(
    tmp_path,
    monkeypatch,
):
    initialized = []
    closed = []

    class Config:
        def __init__(self, **options):
            self.options = options

        @staticmethod
        def create_max_pages_per_part_split_strategy(pages):
            return ("max-pages", pages)

    async def translate(config):
        try:
            yield {
                "type": "finish",
                "translate_result": SimpleNamespace(
                    mono_pdf_path=tmp_path / "mono.pdf",
                    total_seconds=1.25,
                    peak_memory_usage=2048,
                    total_valid_character_count=17,
                    total_valid_text_token_count=9,
                ),
            }
        finally:
            closed.append(config)

    modules = {
        "babeldoc.format.pdf.high_level": SimpleNamespace(
            async_translate=translate,
            init=lambda: initialized.append(True),
        ),
        "babeldoc.format.pdf.translation_config": SimpleNamespace(
            TranslationConfig=Config,
            WatermarkOutputMode=str,
        ),
        "babeldoc.glossary": SimpleNamespace(
            Glossary=SimpleNamespace(
                from_csv=lambda path, target_lang_out: ("csv", path, target_lang_out),
            )
        ),
    }
    for name, module in modules.items():
        monkeypatch.setitem(sys.modules, name, module)
    source = tmp_path / "input.pdf"
    source.write_bytes(b"%PDF-1.4\n")
    glossary = tmp_path / "glossary.csv"
    glossary.write_text("source,target\na,b\n", encoding="utf-8")
    boundary = worker.EngineBoundary(io.BytesIO())
    result = asyncio.run(
        boundary.run_engine(
            {
                "input_file": str(source),
                "output_dir": str(tmp_path),
                "lang_in": "en",
                "lang_out": "zh",
                "pages": None,
                "qps": 4,
                "pool_max_workers": 8,
                "term_pool_max_workers": 5,
                "no_auto_extract_glossary": False,
                "watermark_output_mode": "no_watermark",
                "glossary_files": [str(glossary)],
                "max_pages_per_part": 3,
                "save_auto_extracted_glossary": True,
            },
            {"name": "openai", "llm": True},
            {"name": "ollama", "llm": True},
        )
    )
    options = boundary.config.options
    assert initialized == [True]
    assert closed == [boundary.config]
    assert options["translator"].role == "main"
    assert options["term_extraction_translator"].role == "term"
    assert options["term_extraction_translator"].name == "ollama"
    assert options["term_pool_max_workers"] == 5
    assert options["glossaries"] == [("csv", glossary, "zh")]
    assert options["split_strategy"] == ("max-pages", 3)
    assert options["save_auto_extracted_glossary"] is True
    assert result["total_seconds"] == 1.25
    assert result["total_valid_character_count"] == 17
