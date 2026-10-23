"""Token accounting is checked through the existing subprocess event wrapper."""

import logging
import threading
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock
from unittest.mock import call

import pytest
from pdf2zh_next import high_level


def counters(total, prompt=0, completion=0, cache=0):
    return SimpleNamespace(
        token_count=SimpleNamespace(value=total),
        prompt_token_count=SimpleNamespace(value=prompt),
        completion_token_count=SimpleNamespace(value=completion),
        cache_hit_prompt_token_count=SimpleNamespace(value=cache),
    )


@pytest.mark.parametrize(
    ("main", "term", "fallback", "expected"),
    [
        (object(), object(), {}, {}),
        (
            SimpleNamespace(token_count=SimpleNamespace(value=10)),
            None,
            {},
            {
                "main": {
                    "total": 10,
                    "prompt": 0,
                    "completion": 0,
                    "cache_hit_prompt": 0,
                }
            },
        ),
        (
            counters(100, 70, 30, 20),
            counters(15, 10, 5, 2),
            {"unused": "separate translator takes precedence"},
            {
                "main": {
                    "total": 100,
                    "prompt": 70,
                    "completion": 30,
                    "cache_hit_prompt": 20,
                },
                "term": {
                    "total": 15,
                    "prompt": 10,
                    "completion": 5,
                    "cache_hit_prompt": 2,
                },
            },
        ),
        (
            counters(100, 70, 30, 20),
            "shared",
            {
                "total_tokens": 15,
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "cache_hit_prompt_tokens": 2,
            },
            {
                "main": {
                    "total": 85,
                    "prompt": 60,
                    "completion": 25,
                    "cache_hit_prompt": 18,
                },
                "term": {
                    "total": 15,
                    "prompt": 10,
                    "completion": 5,
                    "cache_hit_prompt": 2,
                },
            },
        ),
        (
            object(),
            None,
            {
                "total_tokens": 0,
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "cache_hit_prompt_tokens": 0,
            },
            {},
        ),
        (
            object(),
            None,
            {
                "total_tokens": 5,
                "prompt_tokens": 3,
                "completion_tokens": 2,
                "cache_hit_prompt_tokens": 0,
            },
            {"term": {"total": 5, "prompt": 3, "completion": 2, "cache_hit_prompt": 0}},
        ),
    ],
)
def test_finish_event_token_accounting_and_pipe_cleanup(
    monkeypatch, main, term, fallback, expected
):
    if term == "shared":
        term = main
    config = SimpleNamespace(
        translator=main,
        term_extraction_translator=term,
        term_extraction_token_usage=fallback,
    )
    event = {"type": "finish", "translate_result": "unchanged"}

    async def translate(received):
        assert received is config
        yield event

    monkeypatch.setattr(high_level, "create_babeldoc_config", Mock(return_value=config))
    monkeypatch.setattr(high_level, "babeldoc_translate", translate)
    monkeypatch.setattr(high_level, "register_secret_values", Mock())
    monkeypatch.setattr(high_level, "install_redacting_filters", Mock())
    monkeypatch.setattr(
        high_level,
        "logging",
        SimpleNamespace(
            getLogger=Mock(return_value=Mock()),
            basicConfig=Mock(),
            WARNING=logging.WARNING,
            INFO=logging.INFO,
        ),
    )
    monkeypatch.setattr(
        high_level,
        "threading",
        SimpleNamespace(
            Event=threading.Event,
            Thread=lambda **_kwargs: SimpleNamespace(start=lambda: None),
        ),
    )
    pipe, logger_queue = Mock(), Mock()
    high_level._translate_wrapper(None, Path("mock.pdf"), pipe, Mock(), logger_queue)
    assert event == {
        "type": "finish",
        "translate_result": "unchanged",
        "token_usage": expected,
    }
    assert pipe.send.call_args_list == [call(event), call(None)]
    pipe.close.assert_called_once_with()
    logger_queue.put.assert_called_once_with(None)
    logger_queue.close.assert_called_once_with()
