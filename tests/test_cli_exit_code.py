"""CLI status reflects how many input PDFs failed."""

import asyncio
import sys
from contextlib import nullcontext
from pathlib import Path
from types import ModuleType
from types import SimpleNamespace

import pytest
from pdf2zh_next import high_level
from pdf2zh_next import main as entry


@pytest.mark.parametrize(
    ("outcomes", "expected_failures"),
    [
        (["finish", "finish"], 0),
        (["finish", "error"], 1),
        (["error", "error"], 2),
    ],
)
def test_translation_runner_counts_file_failures(
    monkeypatch, outcomes, expected_failures
):
    files = [Path("first.pdf"), Path("second.pdf")]
    settings = SimpleNamespace(basic=SimpleNamespace(input_files=files, debug=False))
    monkeypatch.setattr(high_level, "BabelDOCConfig", lambda **_kwargs: object())
    monkeypatch.setattr(
        high_level,
        "create_progress_handler",
        lambda _config: (nullcontext(), lambda _event: None),
    )

    async def stream(_settings, file):
        outcome = outcomes[files.index(file)]
        if outcome == "finish":
            yield {
                "type": "finish",
                "translate_result": SimpleNamespace(
                    original_pdf_path=file,
                    total_seconds=0.1,
                    mono_pdf_path=Path("mono.pdf"),
                    dual_pdf_path=Path("dual.pdf"),
                ),
            }
        else:
            yield {
                "type": "error",
                "error": "synthetic translation failure",
                "error_type": "SyntheticError",
            }

    monkeypatch.setattr(high_level, "do_translate_async_stream", stream)
    result = asyncio.run(
        high_level.do_translate_file_async(settings, ignore_error=True)
    )
    assert result == expected_failures


@pytest.mark.parametrize(
    ("failure_count", "expected_exit"),
    [(0, 0), (1, 1), (2, 1)],
)
def test_cli_exit_status_for_success_partial_and_total_failure(
    monkeypatch, failure_count, expected_exit
):
    settings = SimpleNamespace(
        basic=SimpleNamespace(
            debug=False,
            version=False,
            gui=False,
            input_files={Path("first.pdf"), Path("second.pdf")},
        )
    )
    monkeypatch.setattr(
        entry.ConfigManager, "initialize_config", lambda _self: settings
    )
    monkeypatch.setattr(entry.babeldoc.assets.assets, "warmup", lambda: None)

    async def translate(_settings, ignore_error):
        assert ignore_error is True
        return failure_count

    monkeypatch.setattr(entry, "do_translate_file_async", translate)

    with pytest.raises(SystemExit) as result:
        entry.cli()

    assert result.value.code == expected_exit


def test_gui_branch_keeps_success_status(monkeypatch):
    settings = SimpleNamespace(
        basic=SimpleNamespace(debug=False, version=False, gui=True),
        gui_settings=SimpleNamespace(
            auth_file=None, welcome_page=None, server_port=7860,
            share=False, server_name="127.0.0.1"
        ),
    )
    monkeypatch.setattr(
        entry.ConfigManager, "initialize_config", lambda _self: settings
    )
    monkeypatch.setattr(entry.babeldoc.assets.assets, "warmup", lambda: None)
    gui_stub = ModuleType("pdf2zh_next.gui")
    gui_stub.setup_gui = lambda **_kwargs: None
    monkeypatch.setitem(sys.modules, "pdf2zh_next.gui", gui_stub)

    async def unexpected_translation(*_args, **_kwargs):
        raise AssertionError("GUI startup should not run CLI translation")

    monkeypatch.setattr(entry, "do_translate_file_async", unexpected_translation)
    assert asyncio.run(entry.main()) == 0
