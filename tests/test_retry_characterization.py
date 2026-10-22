"""Freeze v2.9.0 retry, rate-limit, timeout, and cancellation behavior.

All service calls use fakes. These tests intentionally document existing behavior,
including behavior that a future change might improve.
"""

from __future__ import annotations

import asyncio
import importlib
import subprocess
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import httpx
import openai
import pytest
import requests
from pdf2zh_next import high_level
from pdf2zh_next.translator.base_translator import BaseTranslator
from pdf2zh_next.translator.rate_limiter.qps_rate_limiter import QPSRateLimiter
from pdf2zh_next.translator.translator_impl.bing import BingTranslator
from pdf2zh_next.translator.translator_impl.claudecode import ClaudeCodeTranslator
from pdf2zh_next.translator.translator_impl.clitranslator import CLITranslatorTranslator
from pdf2zh_next.translator.translator_impl.google import GoogleTranslator
from pdf2zh_next.translator.translator_impl.openai import OpenAITranslator
from pdf2zh_next.translator.translator_impl.siliconflowfree import (
    SiliconFlowFreeTranslator,
)
from tenacity import Future
from tenacity import RetryError

# Every Tenacity decorator in translator_impl, including both stacked decorators.
# (module, class, method, wrapper depth, max attempts, first waits, retry family)
POLICIES = [
    (
        "anythingllm",
        "AnythingLLMTranslator",
        "do_translate",
        0,
        5,
        (1, 2, 4, 8, 15),
        "all",
    ),
    ("azure", "AzureTranslator", "do_translate", 0, 5, (1, 2, 4, 8, 15), "all"),
    (
        "azureopenai",
        "AzureOpenAITranslator",
        "do_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "openai",
    ),
    (
        "azureopenai",
        "AzureOpenAITranslator",
        "do_llm_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "openai",
    ),
    ("bing", "BingTranslator", "do_translate", 0, 5, (1, 2, 4, 8, 15), "predicate"),
    ("claudecode", "ClaudeCodeTranslator", "do_translate", 0, 3, (2, 4, 8), "process"),
    (
        "clitranslator",
        "CLITranslatorTranslator",
        "do_translate",
        0,
        3,
        (2, 4, 8),
        "process",
    ),
    ("deepl", "DeepLTranslator", "do_translate", 0, 5, (1, 2, 4, 8, 15), "predicate"),
    ("dify", "DifyTranslator", "do_translate", 0, 5, (1, 2, 4, 8, 15), "all"),
    ("google", "GoogleTranslator", "do_translate", 0, 5, (1, 2, 4, 8, 15), "all"),
    ("ollama", "OllamaTranslator", "do_translate", 0, 100, (1, 2, 4, 8, 15), "ollama"),
    (
        "ollama",
        "OllamaTranslator",
        "do_llm_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "ollama",
    ),
    ("openai", "OpenAITranslator", "do_translate", 0, 100, (1, 2, 4, 8, 15), "openai"),
    (
        "openai",
        "OpenAITranslator",
        "do_llm_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "openai",
    ),
    ("qwenmt", "QwenMtTranslator", "do_translate", 0, 100, (1, 2, 4, 8, 15), "openai"),
    (
        "siliconflow",
        "SiliconFlowTranslator",
        "do_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "openai",
    ),
    (
        "siliconflow",
        "SiliconFlowTranslator",
        "do_llm_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "openai",
    ),
    (
        "siliconflowfree",
        "SiliconFlowFreeTranslator",
        "do_llm_translate",
        0,
        3,
        (30, 30, 30),
        "httpx",
    ),
    (
        "siliconflowfree",
        "SiliconFlowFreeTranslator",
        "do_llm_translate",
        1,
        100,
        (4, 4, 4, 8, 16),
        "free429",
    ),
    (
        "tencentmechinetranslation",
        "TencentMechineTranslationTranslator",
        "do_translate",
        0,
        5,
        (1, 2, 4, 8, 15),
        "predicate",
    ),
    (
        "xinference",
        "XinferenceTranslator",
        "do_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "runtime",
    ),
    (
        "xinference",
        "XinferenceTranslator",
        "do_llm_translate",
        0,
        100,
        (1, 2, 4, 8, 15),
        "runtime",
    ),
]


def _wrapper(module: str, class_name: str, method_name: str, depth: int):
    cls = getattr(
        importlib.import_module(f"pdf2zh_next.translator.translator_impl.{module}"),
        class_name,
    )
    method = getattr(cls, method_name)
    for _ in range(depth):
        method = method.__wrapped__
    return method


def _retry_exception(family: str):
    if family == "openai":
        response = httpx.Response(
            429, request=httpx.Request("POST", "https://example.invalid")
        )
        return openai.RateLimitError("limit", response=response, body=None)
    if family == "ollama":
        module = importlib.import_module("ollama")
        return module.ResponseError("limit", status_code=429)
    if family == "process":
        return subprocess.TimeoutExpired("mock-tool", 10)
    if family == "httpx":
        return httpx.HTTPError("network")
    if family == "free429":
        module = importlib.import_module(
            "pdf2zh_next.translator.translator_impl.siliconflowfree"
        )
        return module.RateLimitError("limit")
    if family == "runtime":
        return RuntimeError("server")
    return ValueError("any exception")


def _state(exception):
    return SimpleNamespace(outcome=Future.construct(1, exception, True))


@pytest.mark.parametrize(
    ("module", "class_name", "method_name", "depth", "max_attempts", "waits", "family"),
    POLICIES,
)
def test_all_service_retry_policies(
    module, class_name, method_name, depth, max_attempts, waits, family
):
    policy = _wrapper(module, class_name, method_name, depth).retry
    assert not policy.stop(SimpleNamespace(attempt_number=max_attempts - 1))
    assert policy.stop(SimpleNamespace(attempt_number=max_attempts))
    assert [
        policy.wait(SimpleNamespace(attempt_number=n)) for n in range(1, len(waits) + 1)
    ] == list(waits)
    # All local waits are deterministic exponentials: no jitter or Retry-After input.
    assert [
        policy.wait(SimpleNamespace(attempt_number=n)) for n in range(1, len(waits) + 1)
    ] == list(waits)
    assert policy.retry(_state(_retry_exception(family)))
    assert bool(policy.retry(_state(KeyboardInterrupt()))) == (family == "predicate")
    if family not in ("all", "predicate"):
        assert not policy.retry(_state(ValueError("not selected")))


def test_tencent_translator_works_with_compatible_sdk_without_api_call():
    module = importlib.import_module(
        "pdf2zh_next.translator.translator_impl.tencentmechinetranslation"
    )
    translator = object.__new__(module.TencentMechineTranslationTranslator)
    translator.req = SimpleNamespace(SourceText=None)
    translator.client = SimpleNamespace(
        TextTranslate=Mock(return_value=SimpleNamespace(TargetText="translated"))
    )
    assert translator.do_translate("source") == "translated"
    assert translator.req.SourceText == "source"
    assert translator.client.TextTranslate.call_count == 1


def _google(response_or_error):
    translator = object.__new__(GoogleTranslator)
    translator.lang_in = "en"
    translator.lang_out = "zh-CN"
    translator.endpoint = "https://example.invalid"
    translator.headers = {}
    translator.session = SimpleNamespace(get=Mock(side_effect=response_or_error))
    return translator


def _response(status: int, *, retry_after: str | None = None):
    error = None if status < 400 else requests.HTTPError(f"HTTP {status}")
    return SimpleNamespace(
        status_code=status,
        text='<div class="result-container">translated</div>',
        headers={"Retry-After": retry_after} if retry_after else {},
        raise_for_status=Mock(side_effect=error),
    )


@pytest.mark.parametrize("failure", [429, 503, "timeout"])
def test_google_retries_429_5xx_and_timeout_without_retry_after(monkeypatch, failure):
    waits = []
    monkeypatch.setattr(GoogleTranslator.do_translate.retry, "sleep", waits.append)
    failure_response = (
        requests.Timeout("network timed out")
        if failure == "timeout"
        else _response(failure, retry_after="120")
    )
    translator = _google([failure_response] * 5)
    with pytest.raises(RetryError) as error:
        translator.do_translate("source")
    assert translator.session.get.call_count == 5
    assert waits == [1, 2, 4, 8]
    assert isinstance(
        error.value.last_attempt.exception(), (requests.RequestException,)
    )
    assert all(
        "timeout" not in call.kwargs for call in translator.session.get.call_args_list
    )


def test_google_400_returns_sentinel_and_limiter_waits_once_for_retries(monkeypatch):
    waits = []
    monkeypatch.setattr(GoogleTranslator.do_translate.retry, "sleep", waits.append)
    translator = _google([_response(400)])
    assert translator.do_translate("source") == "IRREPARABLE TRANSLATION ERROR"
    assert waits == []

    translator = _google([_response(429), _response(200)])
    translator.ignore_cache = True
    translator.translate_call_count = 0
    translator.rate_limiter = SimpleNamespace(wait=Mock())
    assert BaseTranslator.translate(translator, "source") == "translated"
    assert translator.session.get.call_count == 2
    assert translator.rate_limiter.wait.call_count == 1
    assert waits == [1]


def test_bing_retries_429_and_interrupt_and_refetches_session(monkeypatch):
    waits = []
    monkeypatch.setattr(BingTranslator.do_translate.retry, "sleep", waits.append)
    translator = object.__new__(BingTranslator)
    translator.lang_in = "en"
    translator.lang_out = "zh-Hans"
    translator.headers = {}
    sid = ("https://example.invalid/", "ig", "iid", "key", "token")
    translator.find_sid = Mock(return_value=sid)
    translator.session = SimpleNamespace(
        post=Mock(side_effect=[_response(429, retry_after="60")] * 5)
    )
    with pytest.raises(RetryError) as error:
        translator.do_translate("text")
    assert isinstance(error.value.last_attempt.exception(), requests.HTTPError)
    assert translator.find_sid.call_count == 5
    assert translator.session.post.call_count == 5
    assert waits == [1, 2, 4, 8]
    assert all(
        "timeout" not in call.kwargs for call in translator.session.post.call_args_list
    )

    translator.find_sid = Mock(side_effect=[KeyboardInterrupt(), sid])
    translator.session.post = Mock(
        return_value=SimpleNamespace(
            raise_for_status=Mock(),
            json=lambda: [{"translations": [{"text": "translated"}]}],
        )
    )
    waits.clear()
    assert translator.do_translate("text") == "translated"
    assert translator.find_sid.call_count == 2
    assert waits == [1]


def _openai_translator(create):
    translator = object.__new__(OpenAITranslator)
    translator.options = {}
    translator.enable_json_mode = False
    translator.model = "offline-model"
    translator.prompt = lambda text: [{"role": "user", "content": text}]
    translator.client = SimpleNamespace(
        chat=SimpleNamespace(completions=SimpleNamespace(create=create))
    )
    return translator


def test_openai_retries_only_429_and_propagates_final_retryerror(monkeypatch):
    waits = []
    monkeypatch.setattr(OpenAITranslator.do_translate.retry, "sleep", waits.append)
    response = httpx.Response(
        429,
        request=httpx.Request("POST", "https://example.invalid"),
        headers={"Retry-After": "120"},
    )
    rate_limit = openai.RateLimitError("limit", response=response, body=None)
    create = Mock(
        side_effect=[
            rate_limit,
            rate_limit,
            SimpleNamespace(
                usage=None,
                choices=[SimpleNamespace(message=SimpleNamespace(content="done"))],
            ),
        ]
    )
    assert _openai_translator(create).do_translate("text") == "done"
    assert create.call_count == 3
    assert waits == [1, 2]

    create = Mock(side_effect=rate_limit)
    with pytest.raises(RetryError) as error:
        _openai_translator(create).do_translate("text")
    assert create.call_count == 100
    assert len(waits) == 101  # 2 waits above, then 99 before terminal failure
    assert isinstance(error.value.last_attempt.exception(), openai.RateLimitError)

    for failure in (
        openai.InternalServerError(
            "server", response=httpx.Response(500, request=response.request), body=None
        ),
        openai.APITimeoutError(request=response.request),
    ):
        create = Mock(side_effect=failure)
        with pytest.raises(type(failure)):
            _openai_translator(create).do_translate("text")
        assert create.call_count == 1


def _free_translator(response):
    translator = object.__new__(SiliconFlowFreeTranslator)
    translator.enable_json_mode = False
    translator.url = "https://example.invalid"
    translator.client = SimpleNamespace(post=Mock(return_value=response))
    return translator


def test_siliconflowfree_nested_429_and_http_error_policies(monkeypatch):
    outer_waits = []
    inner_waits = []
    outer = SiliconFlowFreeTranslator.do_llm_translate
    inner = outer.__wrapped__
    monkeypatch.setattr(outer.retry, "sleep", outer_waits.append)
    monkeypatch.setattr(inner.retry, "sleep", inner_waits.append)

    translator = _free_translator(
        SimpleNamespace(status_code=429, headers={"Retry-After": "120"})
    )
    with pytest.raises(RetryError) as error:
        translator.do_llm_translate("text")
    assert translator.client.post.call_count == 100
    assert isinstance(
        error.value.last_attempt.exception(),
        importlib.import_module(
            "pdf2zh_next.translator.translator_impl.siliconflowfree"
        ).RateLimitError,
    )
    assert len(inner_waits) == 99
    assert inner_waits[:5] == [4, 4, 4, 8, 16]
    assert outer_waits == []

    http_error = httpx.HTTPStatusError(
        "server",
        request=httpx.Request("POST", "https://example.invalid"),
        response=httpx.Response(503),
    )
    translator = _free_translator(
        SimpleNamespace(status_code=503, raise_for_status=Mock(side_effect=http_error))
    )
    with pytest.raises(RetryError) as error:
        translator.do_llm_translate("text")
    assert translator.client.post.call_count == 3
    assert outer_waits == [30, 30]
    assert isinstance(error.value.last_attempt.exception(), httpx.HTTPStatusError)


class _TimeoutProcess:
    def __init__(self):
        self.killed = False
        self.communicate_calls = 0

    def communicate(self, input=None, timeout=None):  # noqa: A002
        self.communicate_calls += 1
        if timeout is not None:
            raise subprocess.TimeoutExpired("mock-tool", timeout)
        return "", ""

    def kill(self):
        self.killed = True


def test_cli_timeout_retries_and_reaps_child(monkeypatch):
    module = importlib.import_module(
        "pdf2zh_next.translator.translator_impl.clitranslator"
    )
    processes = []

    def create_process(*_args, **_kwargs):
        process = _TimeoutProcess()
        processes.append(process)
        return process

    monkeypatch.setattr(module.subprocess, "Popen", create_process)
    waits = []
    monkeypatch.setattr(
        CLITranslatorTranslator.do_translate.retry, "sleep", waits.append
    )
    translator = object.__new__(CLITranslatorTranslator)
    translator.command = "mock-tool"
    translator.args = []
    translator.timeout = 10
    translator.postprocess_command = None
    with pytest.raises(RetryError) as error:
        translator.do_translate("text")
    assert len(processes) == 3
    assert all(p.killed and p.communicate_calls == 2 for p in processes)
    assert waits == [2, 4]
    assert isinstance(error.value.last_attempt.exception(), subprocess.TimeoutExpired)


def test_claude_timeout_becomes_non_retryable_valueerror(monkeypatch):
    module = importlib.import_module(
        "pdf2zh_next.translator.translator_impl.claudecode"
    )
    process = _TimeoutProcess()
    create_process = Mock(return_value=process)
    monkeypatch.setattr(module.subprocess, "Popen", create_process)
    waits = []
    monkeypatch.setattr(ClaudeCodeTranslator.do_translate.retry, "sleep", waits.append)
    translator = object.__new__(ClaudeCodeTranslator)
    translator.claude_code_path = "mock-tool"
    translator.claude_code_model = "offline-model"
    translator.prompt = lambda text: [{"role": "user", "content": text}]
    with pytest.raises(ValueError, match="timed out"):
        translator.do_translate("text")
    assert create_process.call_count == 1
    assert process.killed
    assert process.communicate_calls == 1
    assert waits == []


def test_qps_limiter_spacing_and_no_concurrency_cap(monkeypatch):
    module = importlib.import_module(
        "pdf2zh_next.translator.rate_limiter.qps_rate_limiter"
    )
    clock = SimpleNamespace(now=0.0, waits=[])

    def sleep(seconds):
        clock.waits.append(seconds)
        clock.now += seconds

    monkeypatch.setattr(
        module, "time", SimpleNamespace(monotonic=lambda: clock.now, sleep=sleep)
    )
    limiter = QPSRateLimiter(2)
    for _ in range(3):
        limiter.wait()
    assert clock.waits == [0.5, 0.5]
    with pytest.raises(ValueError):
        limiter.set_max_qps(0)

    # The limiter spaces starts but does not bound requests already in flight.
    barrier = threading.Barrier(2)
    active = 0
    peak = 0
    lock = threading.Lock()

    class BlockingTranslator(BaseTranslator):
        def do_translate(self, text, rate_limit_params=None):
            nonlocal active, peak
            with lock:
                active += 1
                peak = max(peak, active)
            barrier.wait(timeout=5)
            with lock:
                active -= 1
            return text

    translator = object.__new__(BlockingTranslator)
    translator.ignore_cache = True
    translator.translate_call_count = 0
    translator.rate_limiter = QPSRateLimiter(100)
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(translator.translate, ("one", "two")))
    assert results == ["one", "two"]
    assert peak == 2


def test_async_cancellation_releases_mocked_subprocess_resources(monkeypatch):
    entered = asyncio.Event()
    observed = SimpleNamespace(
        timeout=None, process=None, queue=None, pipes=[], threads=[]
    )

    class Callback:
        def __init__(self, timeout):
            observed.timeout = timeout

        def __aiter__(self):
            return self

        async def __anext__(self):
            entered.set()
            await asyncio.Event().wait()

        def has_error(self):
            return False

    class Connection:
        def __init__(self):
            self.sent = []
            self.closed = False

        def send(self, value):
            self.sent.append(value)

        def close(self):
            self.closed = True

    def pipe(duplex=False):  # noqa: ARG001
        pair = (Connection(), Connection())
        observed.pipes.append(pair)
        return pair

    class Queue:
        def __init__(self):
            self.sent = []
            self.closed = False
            observed.queue = self

        def put(self, value):
            self.sent.append(value)

        def close(self):
            self.closed = True

    class Process:
        def __init__(self, target, args):
            self.exitcode = None
            self.terminated = False
            observed.process = self

        def start(self):
            pass

        def join(self, timeout=None):
            pass

        def is_alive(self):
            return not self.terminated

        def terminate(self):
            self.terminated = True
            self.exitcode = -15

    class Thread:
        def __init__(self, target):
            self.joined = False
            observed.threads.append(self)

        def start(self):
            pass

        def join(self, timeout=None):
            self.joined = True

        def is_alive(self):
            return False

    monkeypatch.setattr(
        high_level, "asynchronize", SimpleNamespace(AsyncCallback=Callback)
    )
    monkeypatch.setattr(
        high_level,
        "multiprocessing",
        SimpleNamespace(Pipe=pipe, Queue=Queue, Process=Process),
    )
    monkeypatch.setattr(
        high_level, "threading", SimpleNamespace(Event=threading.Event, Thread=Thread)
    )

    async def exercise():
        async def consume():
            async for _ in high_level._translate_in_subprocess(None, Path("mock.pdf")):
                pass

        task = asyncio.create_task(consume())
        await asyncio.wait_for(entered.wait(), 5)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task

    asyncio.run(exercise())
    assert observed.timeout == 30 * 60
    assert observed.process.terminated
    assert observed.queue.closed and observed.queue.sent == [None]
    assert observed.pipes[0][0].closed
    assert observed.pipes[1][1].closed and observed.pipes[1][1].sent == [True]
    assert all(thread.joined for thread in observed.threads)
