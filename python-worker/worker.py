"""BabelDOC engine boundary. All translation policy belongs to the Rust host."""

from __future__ import annotations

import asyncio
import concurrent.futures
import contextlib
import functools
import importlib.metadata
import json
import logging
import os
import re
import secrets
import sys
import threading
import traceback
from pathlib import Path

VERSION = 1
MAX_FRAME_BYTES = 1024 * 1024
MAX_OUTSTANDING = 128
ENGINE_OPTIONS = frozenset(
    {
        "debug",
        "no_dual",
        "no_mono",
        "formular_font_pattern",
        "formular_char_pattern",
        "split_short_lines",
        "short_line_split_factor",
        "disable_rich_text_translate",
        "dual_translate_first",
        "enhance_compatibility",
        "use_alternating_pages_dual",
        "min_text_length",
        "skip_clean",
        "skip_scanned_detection",
        "ocr_workaround",
        "custom_system_prompt",
        "auto_enable_ocr_workaround",
        "primary_font_family",
        "only_include_translated_page",
        "merge_alternating_line_numbers",
        "remove_non_formula_lines",
        "non_formula_line_iou_threshold",
        "figure_table_protection_threshold",
        "skip_formula_offset_calculation",
        "term_pool_max_workers",
        "report_interval",
        "save_auto_extracted_glossary",
    }
)


class ProtocolError(Exception):
    pass


class BatchTranslationError(RuntimeError):
    """A retried batch HTTP failure BabelDOC may recover per paragraph."""


class IncompleteTranslationError(RuntimeError):
    pass


class ParagraphCompletion:
    """Audit the pinned engine's actual paragraph application, without text.

    A batch exception is recoverable only if every required paragraph later
    finishes post-processing. Tracker.output alone is insufficient: BabelDOC
    sets it before parsing/applying a translation, which can still fail.
    """

    def __init__(self):
        self.lock = threading.Lock()
        self.local = threading.local()
        self.paragraphs = {}  # Keep references: tracker IDs must not be reused.
        self.completed = set()
        self.batch_errors = 0

    def require(self, tracker):
        with self.lock:
            self.paragraphs[id(tracker)] = tracker
            self.completed.discard(id(tracker))

    def complete(self, tracker):
        with self.lock:
            if id(tracker) not in self.paragraphs:
                raise ProtocolError("Engine applied an untracked paragraph")
            self.completed.add(id(tracker))

    def in_batch(self):
        return bool(getattr(self.local, "batch", False))

    def recovered_callback(self):
        with self.lock:
            self.batch_errors += 1

    def verify(self):
        with self.lock:
            required, completed = len(self.paragraphs), len(self.completed)
            batch_errors = self.batch_errors
        if required != completed or (batch_errors and not required):
            raise IncompleteTranslationError(
                f"PDF is incomplete: {required - completed} of {required} required "
                "paragraphs were not applied after BabelDOC fallback"
            )
        return {
            "required_paragraphs": required,
            "completed_paragraphs": completed,
            "batch_errors": batch_errors,
        }

    @contextlib.contextmanager
    def watch(self, paragraph_type=None, batch_type=None):
        # One private process owns one engine job. Restore hooks after all its
        # engine threads have joined; never modify the installed engine files.
        if paragraph_type is None:
            from babeldoc.format.pdf.document_il.midend.il_translator import ILTranslator
            from babeldoc.format.pdf.document_il.midend.il_translator_llm_only import ILTranslatorLLMOnly

            paragraph_type, batch_type = ILTranslator, ILTranslatorLLMOnly
        original_pre = paragraph_type.pre_translate_paragraph
        original_post = paragraph_type.post_translate_paragraph
        original_batch = batch_type.translate_paragraph

        @functools.wraps(original_pre)
        def pre(owner, paragraph, tracker, *args, **kwargs):
            try:
                value = original_pre(owner, paragraph, tracker, *args, **kwargs)
            except BaseException:
                self.require(tracker)
                raise
            if value[0] is not None:
                self.require(tracker)
            return value

        @functools.wraps(original_post)
        def post(owner, paragraph, tracker, *args, **kwargs):
            value = original_post(owner, paragraph, tracker, *args, **kwargs)
            self.complete(tracker)
            return value

        @functools.wraps(original_batch)
        def batch(owner, *args, **kwargs):
            previous = self.in_batch()
            self.local.batch = True
            try:
                return original_batch(owner, *args, **kwargs)
            finally:
                self.local.batch = previous

        paragraph_type.pre_translate_paragraph = pre
        paragraph_type.post_translate_paragraph = post
        batch_type.translate_paragraph = batch
        try:
            yield self
        finally:
            paragraph_type.pre_translate_paragraph = original_pre
            paragraph_type.post_translate_paragraph = original_post
            batch_type.translate_paragraph = original_batch


def engine_diagnostic(exc):
    """Keep exception chains/frames, without locals, URLs or credentials."""
    if isinstance(exc, (SystemExit, KeyboardInterrupt, concurrent.futures.CancelledError)):
        return type(exc).__name__
    # Put the immediate exception before the bounded traceback so a long stack
    # cannot hide the actionable reason in the GUI's bounded failure message.
    detail = f"{type(exc).__name__}: {exc}\n" + "".join(traceback.format_exception(exc))
    detail = re.sub(r"https?://[^\s\"'<>]+", "[URL REDACTED]", detail)
    detail = re.sub(r"(?i)\bBearer\s+[^\s\"'<>]+", "Bearer [REDACTED]", detail)
    detail = re.sub(
        r"(?i)((?:api[-_ ]?key|token|secret|password|authorization)\s*[:=]\s*)[^\s,;]+",
        r"\1[REDACTED]", detail,
    )
    detail = re.sub(r"\bsk-[A-Za-z0-9_-]+", "[REDACTED]", detail)
    return detail[:8192]


def read_frame(stream):
    raw = stream.readline(MAX_FRAME_BYTES + 1)
    if not raw:
        return None
    if len(raw) > MAX_FRAME_BYTES or not raw.endswith(b"\n"):
        raise ProtocolError("Frame too large or incomplete")
    try:
        value = json.loads(raw.decode("utf-8"))
    except (ValueError, UnicodeDecodeError) as exc:
        raise ProtocolError("Invalid JSON frame") from exc
    if not isinstance(value, dict) or value.get("v") != VERSION:
        raise ProtocolError("Unsupported protocol version")
    return value


def private_stream(descriptor, mode, buffering=-1):
    duplicate = os.dup(descriptor)
    try:
        # Windows duplicates of fd 0/1/2 can inherit even on modern Python.
        os.set_inheritable(duplicate, False)
        return os.fdopen(duplicate, mode, buffering=buffering)
    except BaseException:
        os.close(duplicate)
        raise


def set_native_standard_handle(number, descriptor):
    if os.name != "nt":
        return
    import ctypes
    import msvcrt

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.SetStdHandle.argtypes = [ctypes.c_ulong, ctypes.c_void_p]
    kernel.SetStdHandle.restype = ctypes.c_int
    if not kernel.SetStdHandle(
        ctypes.c_ulong(number & 0xFFFFFFFF),
        ctypes.c_void_p(msvcrt.get_osfhandle(descriptor)),
    ):
        raise OSError("Cannot isolate native standard handle")


def isolate_stdout():
    """Keep a private protocol handle; redirect Python, CRT and Win32 stdout."""
    sys.stdout.flush()
    protocol = private_stream(sys.stdout.fileno(), "wb", buffering=0)
    try:
        os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
        set_native_standard_handle(-11, sys.stdout.fileno())
    except BaseException:
        protocol.close()
        raise
    sys.stdout = sys.stderr
    return protocol


def isolate_stdin():
    """Keep protocol input private; native engine code must see EOF on stdin.

    A native extension can inspect or lock CRT fd 0 while loading. Sharing that
    descriptor with a blocked protocol read can deadlock extension import.
    """
    descriptor = sys.stdin.fileno()
    protocol = private_stream(descriptor, "rb")
    try:
        with Path(os.devnull).open("rb") as null:
            os.dup2(null.fileno(), descriptor)
        set_native_standard_handle(-10, descriptor)
    except BaseException:
        protocol.close()
        raise
    return protocol


class EngineBoundary:
    def __init__(self, output):
        self.output = output
        self.session = secrets.token_hex(16)
        self.write_lock = threading.Lock()
        self.pending_lock = threading.Lock()
        self.pending = {}
        self.next_id = 0
        self.cancelled = threading.Event()
        self.failed = threading.Event()
        self.config = None
        self.task = None
        self.started = False
        self.terminal = False
        self.capacity = MAX_OUTSTANDING
        self.stage = "Worker startup"
        self.completion = None

    def send(self, kind, **fields):
        if kind == "progress" and isinstance(fields.get("event", {}).get("stage"), str):
            self.stage = fields["event"]["stage"]
        message = {"v": VERSION, "type": kind, "session": self.session, **fields}
        if self.started:
            message["task_id"] = "job-1"
        raw = (json.dumps(message, ensure_ascii=False, allow_nan=False) + "\n").encode(
            "utf-8"
        )
        if len(raw) > MAX_FRAME_BYTES:
            raise ProtocolError("Outgoing frame too large")
        with self.write_lock:
            remaining = memoryview(raw)
            while remaining:
                written = self.output.write(remaining)
                if written is None or written <= 0:
                    raise BrokenPipeError("Protocol writer made no progress")
                remaining = remaining[written:]
            self.output.flush()

    def cancel(self):
        self.cancelled.set()
        with self.pending_lock:
            futures = list(self.pending.values())
            self.pending.clear()
        for future in futures:
            if not future.done():
                future.set_exception(concurrent.futures.CancelledError())
        config = self.config
        if config is not None:
            config.cancel_translation()

    def request(
        self, translator, method, text, ignore_cache=False, rate_limit_params=None
    ):
        if self.cancelled.is_set():
            raise concurrent.futures.CancelledError()
        with self.pending_lock:
            if self.cancelled.is_set():
                raise concurrent.futures.CancelledError()
            if len(self.pending) >= self.capacity:
                self.failed.set()
                raise ProtocolError("Translation request capacity exceeded")
            self.next_id += 1
            request_id = f"w-{self.next_id}"
            future = concurrent.futures.Future()
            self.pending[request_id] = future
        try:
            self.send(
                "translate",
                id=request_id,
                text=text,
                method=method,
                ignore_cache=ignore_cache,
                rate_limit_params=rate_limit_params,
                translator=translator,
                batch_fallback=bool(self.completion and self.completion.in_batch()),
            )
            return future.result(timeout=1800)
        except BatchTranslationError:
            if self.completion and self.completion.in_batch():
                self.completion.recovered_callback()
            else:
                self.failed.set()
                self.cancel()
            raise
        except BaseException:
            if not self.cancelled.is_set():
                self.failed.set()
                self.cancel()
            raise
        finally:
            with self.pending_lock:
                self.pending.pop(request_id, None)

    def respond(self, message):
        request_id = message.get("id")
        with self.pending_lock:
            future = self.pending.get(request_id)
            if future is None or future.done():
                if self.cancelled.is_set():
                    return  # A response already in flight when cancellation won.
                raise ProtocolError("Unknown or duplicate response ID")
            if "error" in message:
                error = message["error"]
                if error.get("code") == "batch_translation_failed":
                    future.set_exception(BatchTranslationError(error.get("message", "Rust batch translation failed")))
                else:
                    self.failed.set()
                    future.set_exception(RuntimeError("Rust translation failed"))
            elif isinstance(message.get("text"), str):
                future.set_result(message["text"])
            else:
                raise ProtocolError("Missing translation text")

    async def run_engine(self, job, translator_options, term_translator_options=None):
        self.send(
            "progress",
            event={
                "type": "progress_start",
                "stage": "Loading PDF engine",
                "overall_progress": 0,
            },
        )
        from babeldoc.format.pdf.high_level import async_translate
        from babeldoc.format.pdf.high_level import init
        from babeldoc.format.pdf.translation_config import TranslationConfig
        from babeldoc.format.pdf.translation_config import WatermarkOutputMode

        input_file = Path(job["input_file"])
        output_dir = Path(job["output_dir"])
        if not input_file.is_absolute() or not output_dir.is_absolute():
            raise ProtocolError("Engine paths must be absolute")
        input_file = input_file.resolve(strict=True)
        output_dir = output_dir.resolve(strict=True)
        if not input_file.is_file() or not output_dir.is_dir():
            raise ProtocolError("Invalid engine paths")
        required = {
            "input_file",
            "output_dir",
            "lang_in",
            "lang_out",
            "pages",
            "qps",
            "pool_max_workers",
            "no_auto_extract_glossary",
            "watermark_output_mode",
            "glossary_files",
            "max_pages_per_part",
        }
        unknown = set(job) - required - ENGINE_OPTIONS
        if unknown:
            raise ProtocolError("Unsupported engine option")
        options = {key: value for key, value in job.items() if key in ENGINE_OPTIONS}
        if job.get("glossary_files"):
            from babeldoc.glossary import Glossary

            options["glossaries"] = [
                Glossary.from_csv(Path(path), target_lang_out=job["lang_out"])
                for path in job["glossary_files"]
            ]
        if job.get("max_pages_per_part"):
            options["split_strategy"] = (
                TranslationConfig.create_max_pages_per_part_split_strategy(
                    job["max_pages_per_part"]
                )
            )
        adapter = RustTranslatorAdapter(self, translator_options, job)
        term_adapter = (
            RustTranslatorAdapter(self, term_translator_options, job, role="term")
            if term_translator_options
            else adapter
        )
        init()
        self.config = TranslationConfig(
            input_file=input_file,
            output_dir=output_dir,
            font=None,
            doc_layout_model=None,
            translator=adapter,
            term_extraction_translator=term_adapter,
            lang_in=job["lang_in"],
            lang_out=job["lang_out"],
            pages=job.get("pages"),
            qps=job["qps"],
            pool_max_workers=job["pool_max_workers"],
            auto_extract_glossary=not job["no_auto_extract_glossary"],
            watermark_output_mode=WatermarkOutputMode(job["watermark_output_mode"]),
            **options,
        )
        if self.cancelled.is_set():
            self.config.cleanup_temp_files()
            raise concurrent.futures.CancelledError()
        result = None
        self.completion = ParagraphCompletion()
        stream = async_translate(self.config)
        with self.completion.watch():
            try:
                async for event in stream:
                    if self.cancelled.is_set():
                        self.config.cancel_translation()
                        raise concurrent.futures.CancelledError()
                    kind = event.get("type")
                    if kind == "finish":
                        value = event["translate_result"]
                        result = {
                            key: str(getattr(value, key))
                            if getattr(value, key, None)
                            else None
                            for key in (
                                "original_pdf_path",
                                "mono_pdf_path",
                                "dual_pdf_path",
                                "no_watermark_mono_pdf_path",
                                "no_watermark_dual_pdf_path",
                                "auto_extracted_glossary_path",
                            )
                        }
                        for key in (
                            "total_seconds",
                            "peak_memory_usage",
                            "total_valid_character_count",
                            "total_valid_text_token_count",
                        ):
                            metric = getattr(value, key, None)
                            if isinstance(metric, (int, float)):
                                result[key] = metric
                        break
                    if kind == "error":
                        error = event.get("error")
                        if isinstance(error, BaseException):
                            raise error
                        raise RuntimeError(str(error or "BabelDOC processing failed"))
                    if kind in {"progress_start", "progress_update", "progress_end"}:
                        # Avoid exception objects, arbitrary engine metadata and document text.
                        progress = {
                            key: event[key]
                            for key in (
                                "type",
                                "stage",
                                "overall_progress",
                                "part_index",
                                "total_parts",
                                "stage_current",
                                "stage_total",
                            )
                            if key in event
                            and isinstance(event[key], (str, int, float, bool))
                        }
                        self.send("progress", event=progress)
            finally:
                await stream.aclose()
        if self.failed.is_set():
            raise RuntimeError("Rust translation failed; PDF is incomplete")
        if self.cancelled.is_set():
            raise concurrent.futures.CancelledError()
        if result is None:
            raise RuntimeError("Engine ended without a result")
        result["translation_audit"] = self.completion.verify()
        return result

    def run_assets(self, operation, path):
        from babeldoc.assets import assets

        if self.cancelled.is_set():
            raise concurrent.futures.CancelledError()
        resolved = None
        if path is not None:
            resolved = Path(path)
            if not resolved.is_absolute():
                raise ProtocolError("Resource path must be absolute")
            resolved = resolved.resolve(strict=True)
        self.send(
            "progress",
            event={"type": "progress_start", "stage": operation},
        )
        if operation == "warmup":
            if resolved is not None:
                raise ProtocolError("Warmup does not accept a path")
            assets.warmup()
        elif operation == "generate_offline_assets":
            if resolved is None or not resolved.is_dir():
                raise ProtocolError("Archive staging directory is unavailable")
            assets.generate_offline_assets_package(resolved)
        elif operation == "restore_offline_assets":
            if resolved is None:
                raise ProtocolError("Offline asset package is unavailable")
            assets.restore_offline_assets_package(resolved)
        else:
            raise ProtocolError("Unsupported resource operation")
        if self.cancelled.is_set():
            raise concurrent.futures.CancelledError()
        tag = assets.get_offline_assets_tag()
        result = {"kind": operation, "tag": tag}
        if operation == "generate_offline_assets":
            result["archive_name"] = f"offline_assets_{tag}.zip"
        return result

    def start(self, message):
        if self.started:
            raise ProtocolError("Only one job is allowed per worker")
        if message.get("task_id") != "job-1":
            raise ProtocolError("Invalid task ID")
        operation = message.get("operation", "pdf")
        if operation not in {
            "pdf", "warmup", "generate_offline_assets", "restore_offline_assets"
        }:
            raise ProtocolError("Unsupported engine operation")
        capacity = message.get("max_outstanding", MAX_OUTSTANDING)
        if type(capacity) is not int or not 1 <= capacity <= 1_000_000:
            raise ProtocolError("Invalid outstanding request capacity")
        self.capacity = capacity
        self.started = True

        def execute():
            import faulthandler

            try:
                if operation == "pdf":
                    if message["job"].get("debug", False):
                        faulthandler.dump_traceback_later(20, repeat=True, file=sys.stderr)
                    self.send(
                        "progress",
                        event={
                            "type": "progress_start",
                            "stage": "Starting PDF engine",
                            "overall_progress": 0,
                        },
                    )
                    result = asyncio.run(
                        self.run_engine(
                            message["job"],
                            message["translator"],
                            message.get("term_translator"),
                        )
                    )
                else:
                    result = self.run_assets(operation, message.get("path"))
                if self.cancelled.is_set():
                    raise concurrent.futures.CancelledError()
                self.send("finished", result=result)
            except BaseException as exc:
                code = (
                    "incomplete_translation" if isinstance(exc, IncompleteTranslationError) else
                    "translation_failed"
                    if self.failed.is_set()
                    else ("cancelled" if self.cancelled.is_set() else "engine_failed")
                )
                diagnostic = engine_diagnostic(exc)
                logging.error("%s: %s", self.stage, diagnostic)
                self.send("error", code=code, stage=self.stage, message=diagnostic)
            finally:
                faulthandler.cancel_dump_traceback_later()
                self.terminal = True

        self.task = threading.Thread(
            target=execute, name="babeldoc-engine", daemon=True
        )
        self.task.start()

    def handle(self, message):
        if message.get("session") != self.session:
            raise ProtocolError("Invalid session")
        kind = message.get("type")
        if kind == "start":
            self.start(message)
        elif self.started and message.get("task_id") != "job-1":
            raise ProtocolError("Invalid task ID")
        elif kind == "translated":
            self.respond(message)
        elif kind == "cancel":
            self.cancel()
            self.send("cancel_ack")
        elif kind == "shutdown":
            if not self.terminal:
                self.cancel()
            self.send("shutdown_ack")
            return False
        else:
            raise ProtocolError("Unsupported message type")
        return True


class RustTranslatorAdapter:
    def __init__(self, boundary, translator, job, role="main"):
        self.boundary = boundary
        self.name = translator["name"]
        self.llm = translator["llm"]
        self.lang_in = job["lang_in"]
        self.lang_out = job["lang_out"]
        self.model = "rust-host"
        self.role = role

    def translate(self, text, ignore_cache=False, rate_limit_params=None):
        return self.boundary.request(
            self.role, "translate", text, ignore_cache, rate_limit_params
        )

    def llm_translate(self, text, ignore_cache=False, rate_limit_params=None):
        return self.boundary.request(
            self.role, "llm_translate", text, ignore_cache, rate_limit_params
        )

    def do_llm_translate(self, text, rate_limit_params=None):
        if not self.llm:
            raise NotImplementedError
        if text is None:
            return None  # BabelDOC's local capability probe must never become an API request.
        return self.llm_translate(text, rate_limit_params=rate_limit_params)

    def get_formular_placeholder(self, placeholder_id):
        return "{v" + str(placeholder_id) + "}", f"{{\\s*v\\s*{placeholder_id}\\s*}}"

    def get_rich_text_left_placeholder(self, placeholder_id):
        return (
            f"<style id='{placeholder_id}'>",
            f"<\\s*style\\s*id\\s*=\\s*'\\s*{placeholder_id}\\s*'\\s*>",
        )

    def get_rich_text_right_placeholder(self, placeholder_id):
        return "</style>", r"<\s*\/\s*style\s*>"


def configure_native_runtime():
    # The locked Windows OpenCV Concurrency backend throws for a deeply nested
    # process image. Its explicit one-thread mode bypasses that backend; only
    # image kernels change, not ONNX or the Rust translator's request pool.
    if sys.platform == "win32" and len(os.path.realpath(sys.executable)) >= 260:
        import cv2

        cv2.setNumThreads(1)
        logging.warning("Long runtime path: using one OpenCV image-processing thread")


def main():
    boundary = EngineBoundary(isolate_stdout())
    try:
        protocol_input = isolate_stdin()
    except BaseException:
        boundary.output.close()
        raise
    logging.basicConfig(stream=sys.stderr, level=logging.WARNING)
    status = 0
    try:
        version = importlib.metadata.version("babeldoc")
        if version != "0.6.4":
            raise ProtocolError("Worker requires locked BabelDOC 0.6.4")
        configure_native_runtime()
        boundary.send(
            "ready", babeldoc_version=version,
            operations=["pdf", "warmup", "generate_offline_assets", "restore_offline_assets"],
            paragraph_completion_audit=1,
        )
        while True:
            message = read_frame(protocol_input)
            if message is None or not boundary.handle(message):
                break
    except (Exception, KeyboardInterrupt) as exc:
        status = 1
        try:
            boundary.send("error", code="protocol_failed", stage=boundary.stage, message=engine_diagnostic(exc))
        except (OSError, ProtocolError):
            pass
    finally:
        boundary.cancel()
        if boundary.task:
            boundary.task.join(timeout=5)
        boundary.output.close()
        protocol_input.close()
    return status


if __name__ == "__main__":
    raise SystemExit(main())
