"""Real SDK parsing against loopback HTTP; no external provider credentials."""

import json
import threading
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from types import SimpleNamespace

import httpx
import openai
import pytest
from pdf2zh_next.config.model import SettingsModel
from pdf2zh_next.translator.translator_impl.openai import OpenAITranslator


@pytest.fixture
def endpoint():
    state = SimpleNamespace(responses=[], requests=[])

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers["Content-Length"])
            state.requests.append(json.loads(self.rfile.read(length)))
            status, response = state.responses.pop(0)
            body = json.dumps(response).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            # Exercise the application's outer 429 policy, not SDK retries too.
            self.send_header("x-should-retry", "false")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    state.url = f"http://127.0.0.1:{server.server_port}/v1"
    try:
        yield state
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        assert not thread.is_alive()


@pytest.fixture
def translator(endpoint, monkeypatch, tmp_path):
    monkeypatch.setenv("USERPROFILE", str(tmp_path))
    settings = SettingsModel(
        translate_engine_settings={
            "translate_engine_type": "OpenAI",
            "openai_base_url": endpoint.url,
            "openai_api_key": "synthetic-loopback-only",
            "openai_model": "offline-fixture",
        }
    )
    result = OpenAITranslator(settings, None)
    try:
        yield result
    finally:
        result.client.close()
        assert result.client.is_closed()


def completion(content="  測試譯文  "):
    return {
        "id": "local-fixture",
        "object": "chat.completion",
        "created": 0,
        "model": "offline-fixture",
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop",
            }
        ],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8},
    }


def test_sdk_parses_translation_and_usage(endpoint, translator):
    endpoint.responses = [(200, completion())]
    assert translator.do_translate("test") == "測試譯文"
    assert translator.token_count.value == 8
    assert len(endpoint.requests) == 1
    assert endpoint.requests[0]["model"] == "offline-fixture"


def test_http_429_retries_then_parses_success(endpoint, translator):
    endpoint.responses = [
        (429, {"error": {"message": "local quota", "type": "rate_limit_error"}}),
        (200, completion()),
    ]
    assert translator.do_translate("test") == "測試譯文"
    assert len(endpoint.requests) == 2


def test_http_400_preserves_provider_error_without_retry(endpoint, translator):
    endpoint.responses = [
        (
            400,
            {
                "error": {
                    "message": "local invalid model",
                    "type": "invalid_request_error",
                }
            },
        )
    ]
    with pytest.raises(openai.BadRequestError, match="local invalid model"):
        translator.do_translate("test")
    assert len(endpoint.requests) == 1


def test_empty_choices_is_not_reported_as_success(endpoint, translator):
    response = completion()
    response["choices"] = []
    endpoint.responses = [(200, response)]
    with pytest.raises(IndexError):
        translator.do_translate("test")
    assert len(endpoint.requests) == 1


def test_sdk_timeout_retries_are_distinct_from_application_policy(translator):
    calls = []

    def timeout(request):
        calls.append(request)
        raise httpx.ReadTimeout("synthetic read timeout", request=request)

    translator.client.close()
    translator.client = openai.OpenAI(
        api_key="synthetic-loopback-only",
        base_url="http://127.0.0.1:1/v1",
        http_client=httpx.Client(transport=httpx.MockTransport(timeout)),
    )
    with pytest.raises(openai.APITimeoutError):
        translator.do_translate("test")
    assert len(calls) == 3  # SDK's original two retries; outer policy excludes timeout.
