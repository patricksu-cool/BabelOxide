"""Synthetic credential checks for CLI, config and exception logging."""

import importlib
import io
import json
import logging
import os
import subprocess
import sys

import pytest
from pdf2zh_next.config.main import ConfigManager
from pdf2zh_next.config.translate_engine_model import GUI_PASSWORD_FIELDS
from pdf2zh_next.security_logging import REDACTED
from pdf2zh_next.security_logging import RedactingLogFilter
from pdf2zh_next.security_logging import redact_for_log
from pdf2zh_next.security_logging import redact_text
from pdf2zh_next.security_logging import register_secret_values


def test_nested_sensitive_fields_are_masked_without_changing_settings():
    command_key = "SYNTHETIC_CLI_ARGUMENT_ABC123"
    settings = {
        "translate_engine_settings": {
            **{name: f"SYNTHETIC_{name}_ABC123" for name in GUI_PASSWORD_FIELDS},
            "clitranslator_command": f"mock-tool --api-key {command_key}",
        },
        "basic": {"qps": 7, "debug": True},
    }

    safe = redact_for_log(settings)

    assert all(
        value == REDACTED for value in safe["translate_engine_settings"].values()
    )
    assert safe["basic"] == {"qps": 7, "debug": True}
    assert settings["translate_engine_settings"]["openai_api_key"].startswith(
        "SYNTHETIC_"
    )
    assert command_key not in redact_text(f"Subprocess failed with {command_key}")


def test_exception_and_authorization_header_are_redacted():
    sample_key = "SYNTHETIC_EXCEPTION_SECRET_ABC123"
    register_secret_values({"api_key": sample_key})
    stream = io.StringIO()
    logger = logging.getLogger("test.security_logging.exception")
    logger.setLevel(logging.DEBUG)
    logger.propagate = False
    handler = logging.StreamHandler(stream)
    handler.addFilter(RedactingLogFilter())
    logger.addHandler(handler)
    try:
        try:
            raise RuntimeError(f"Provider failed for {sample_key}")
        except RuntimeError:
            logger.exception("Authorization: Bearer %s; qps=7", sample_key)
    finally:
        logger.removeHandler(handler)
        handler.close()

    output = stream.getvalue()
    assert sample_key not in output
    assert REDACTED in output
    assert "RuntimeError" in output
    assert "qps=7" in output


def test_header_url_and_labeled_password_are_redacted_without_registration():
    header_key = "SYNTHETIC_HEADER_ABC123"
    url_value = "SYNTHETIC_URL_PASSWORD_ABC123"
    labeled_value = "SYNTHETIC_PASSWORD_ABC123"
    message = (
        f"Authorization: Bearer {header_key}; "
        f"proxy=https://user:{url_value}@localhost; password={labeled_value}; qps=7"
    )

    output = redact_text(message)

    assert header_key not in output
    assert url_value not in output
    assert labeled_value not in output
    assert "qps=7" in output


def test_unregistered_exception_message_is_not_logged():
    sample_key = "SYNTHETIC_UNKNOWN_EXCEPTION_ABC123"
    stream = io.StringIO()
    logger = logging.getLogger("test.security_logging.unknown_exception")
    logger.setLevel(logging.ERROR)
    logger.propagate = False
    handler = logging.StreamHandler(stream)
    handler.addFilter(RedactingLogFilter())
    logger.addHandler(handler)
    try:
        try:
            raise RuntimeError(sample_key)
        except RuntimeError:
            logger.exception("Provider request failed")
    finally:
        logger.removeHandler(handler)
        handler.close()

    assert sample_key not in stream.getvalue()
    assert "RuntimeError" in stream.getvalue()


def test_cli_unhandled_exception_is_redacted(monkeypatch, caplog, capsys):
    sample_key = "SYNTHETIC_FATAL_EXCEPTION_ABC123"
    main_module = importlib.import_module("pdf2zh_next.main")

    async def fail():
        raise RuntimeError(sample_key)

    monkeypatch.setattr(main_module, "main", fail)
    with caplog.at_level(logging.ERROR), pytest.raises(SystemExit) as error:
        main_module.cli()

    output = caplog.text + capsys.readouterr().err
    assert error.value.code == 1
    assert sample_key not in output
    assert "RuntimeError" in output


def test_config_debug_log_masks_cli_values(caplog):
    sample_key = "SYNTHETIC_CONFIG_SECRET_ABC123"
    with caplog.at_level(logging.DEBUG, logger="pdf2zh_next.config.main"):
        ConfigManager().parse_dict_vars(dict_vars={"OPENAI_API_KEY": sample_key})

    assert sample_key not in caplog.text
    assert "openai_api_key" in caplog.text
    assert REDACTED in caplog.text


def test_configured_cli_command_is_not_written_to_debug_log(monkeypatch, caplog):
    module = importlib.import_module(
        "pdf2zh_next.translator.translator_impl.clitranslator"
    )
    sample_key = "SYNTHETIC_CLI_DEBUG_ABC123"

    class Process:
        returncode = 0

        def communicate(self, input, timeout):  # noqa: A002
            return "translated", ""

    monkeypatch.setattr(module.subprocess, "Popen", lambda *_args, **_kwargs: Process())
    translator = object.__new__(module.CLITranslatorTranslator)
    translator.command = "mock-tool"
    translator.args = ["--api-key", sample_key]
    translator.timeout = 1
    translator.postprocess_command = None

    with caplog.at_level(logging.DEBUG, logger=module.__name__):
        assert translator.do_translate("source text") == "translated"

    assert sample_key not in caplog.text
    assert "configured CLI translator" in caplog.text


def test_postprocess_stderr_is_not_written_to_log(monkeypatch, caplog):
    module = importlib.import_module(
        "pdf2zh_next.translator.translator_impl.clitranslator"
    )
    sample_key = "SYNTHETIC_POSTPROCESS_STDERR_ABC123"

    class Process:
        returncode = 1

        def communicate(self, input, timeout):  # noqa: A002
            return "", sample_key

    monkeypatch.setattr(module.subprocess, "Popen", lambda *_args, **_kwargs: Process())
    translator = object.__new__(module.CLITranslatorTranslator)
    translator.postprocess_command = ["mock-tool"]
    translator.timeout = 1

    with caplog.at_level(logging.ERROR, logger=module.__name__):
        with pytest.raises(subprocess.CalledProcessError):
            translator._run_postprocess("source text")

    assert sample_key not in caplog.text
    assert "exit 1" in caplog.text


def test_claude_input_and_output_are_not_written_to_logs(monkeypatch, caplog):
    module = importlib.import_module(
        "pdf2zh_next.translator.translator_impl.claudecode"
    )
    sample_key = "SYNTHETIC_CLAUDE_CONTENT_ABC123"

    class Process:
        returncode = 0

        def communicate(self, input, timeout):  # noqa: A002
            return json.dumps(
                {"type": "text", "text": "translated", "raw": sample_key}
            ), ""

    monkeypatch.setattr(module.subprocess, "Popen", lambda *_args, **_kwargs: Process())
    translator = object.__new__(module.ClaudeCodeTranslator)
    translator.claude_code_path = "mock-tool"
    translator.claude_code_model = "offline-model"
    translator.prompt = lambda text: [{"role": "user", "content": text}]

    with caplog.at_level(logging.DEBUG, logger=module.__name__):
        assert translator.do_translate(sample_key) == "translated"

    assert sample_key not in caplog.text
    assert "input characters" in caplog.text


@pytest.mark.parametrize("debug", [False, True])
def test_cli_version_log_never_exposes_synthetic_key(tmp_path, debug):
    sample_key = "SYNTHETIC_ROUND2_SECRET_ABC123"
    env = os.environ.copy()
    env["USERPROFILE"] = str(tmp_path)
    env["PDF2ZH_APP_DIR"] = str(tmp_path)
    env["GRADIO_ANALYTICS_ENABLED"] = "False"
    args = [
        sys.executable,
        "-m",
        "pdf2zh_next.main",
        "--version",
        "--openai",
        "--openai-api-key",
        sample_key,
    ]
    if debug:
        args.append("--debug")

    result = subprocess.run(args, capture_output=True, text=True, env=env, timeout=30)
    output = result.stdout + result.stderr

    assert result.returncode == 0
    assert "pdf2zh-next version: 2.9.0" in output
    # RichHandler wraps long lines, so check both ends of the synthetic value.
    assert "SYNTHETIC_ROUND2" not in output
    assert "ABC123" not in output
    if debug:
        assert REDACTED in output
