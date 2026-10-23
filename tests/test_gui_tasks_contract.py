"""Characterize session/result/cancellation behavior before controller extraction."""

import asyncio
import sys
import zipfile
from pathlib import Path
from types import SimpleNamespace

import gradio as gr
import pytest


@pytest.fixture
def setup_task(monkeypatch, tmp_path):
    monkeypatch.setattr(sys, "argv", ["pytest"])
    from pdf2zh_next import gui
    from pdf2zh_next.ui.session import new_session_state

    state = new_session_state()
    state["uploaded_files"] = ["paper.pdf"]
    source = tmp_path / "paper.pdf"
    source.write_bytes(b"%PDF-1.4\n")
    mono, dual = tmp_path / "mono.pdf", tmp_path / "dual.pdf"
    mono.write_bytes(b"mono")
    dual.write_bytes(b"dual")
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(gui, "settings", SimpleNamespace(clone=lambda: None))
    monkeypatch.setattr(gui, "build_ui_inputs", lambda *_: {"state": state})
    monkeypatch.setattr(gui, "_prepare_input_file", lambda *_: [source])
    monkeypatch.setattr(gui, "_build_translate_settings", lambda *_: None)
    return gui, state, mono, dual


def test_success_keeps_output_contract_archives_and_session_cleanup(
    setup_task, monkeypatch
):
    gui, state, mono, dual = setup_task

    async def run(*_args, **_kwargs):
        return mono, dual, None, {"main": {"total": 10}, "term": {"total": 2}}

    monkeypatch.setattr(gui, "_run_translation_task", run)

    async def consume():
        return [
            update
            async for update in gui.translate_files(
                "File", [], "", progress=lambda *_a, **_k: None
            )
        ]

    updates = asyncio.run(consume())
    assert len(updates) == 2 and all(len(update) == 19 for update in updates)
    assert state["current_task"] is None and state["uploaded_files"] == []
    assert len(state["results"]) == 1
    assert "Total 12" in updates[-1][7]["value"]
    with zipfile.ZipFile(updates[-1][11]) as archive:
        assert archive.namelist() == ["mono_mono.pdf", "dual_dual.pdf"]
    assert Path(updates[-1][13]).is_file() and Path(updates[-1][15]).is_file()
    assert updates[-1][17] is None


def test_task_error_surfaces_and_clears_current_task(setup_task, monkeypatch):
    gui, state, _, _ = setup_task

    async def fail(*_args, **_kwargs):
        raise ValueError("synthetic controller failure")

    monkeypatch.setattr(gui, "_run_translation_task", fail)

    async def consume():
        return [update async for update in gui.translate_files("File", [], "")]

    with pytest.raises(gr.Error, match="synthetic controller failure"):
        asyncio.run(consume())
    assert state["current_task"] is None


def test_multiple_files_keep_archive_order_and_optional_outputs(
    setup_task, monkeypatch
):
    gui, state, mono, dual = setup_task
    second = mono.parent / "second.pdf"
    second.write_bytes(b"second")
    terms = mono.parent / "terms.csv"
    terms.write_text("source,target\n")
    monkeypatch.setattr(
        gui, "_prepare_input_file", lambda *_: [mono.parent / "paper.pdf", second]
    )

    async def run(_settings, path, *_args, **_kwargs):
        return (None, None, terms, None) if path == second else (mono, dual, None, None)

    monkeypatch.setattr(gui, "_run_translation_task", run)

    async def consume():
        return [update async for update in gui.translate_files("File", [], "")]

    updates = asyncio.run(consume())
    assert len(updates) == 3 and len(state["results"]) == 2
    with zipfile.ZipFile(updates[-1][11]) as archive:
        assert archive.namelist() == [
            "mono_mono.pdf",
            "dual_dual.pdf",
            "glossary_terms.csv",
        ]
    assert updates[-1][0] is None and updates[-1][2] is None
    assert updates[-1][3] == str(terms)
    assert updates[-1][16]["visible"] is True


def test_cancel_reaches_worker_and_preserves_output_contract(setup_task, monkeypatch):
    gui, state, _, _ = setup_task

    async def exercise():
        started, released = asyncio.Event(), asyncio.Event()

        async def waiting(*_args, **_kwargs):
            started.set()
            try:
                await asyncio.Event().wait()
            finally:
                released.set()

        monkeypatch.setattr(gui, "_run_translation_task", waiting)

        async def consume():
            return [update async for update in gui.translate_files("File", [], "")]

        consumer = asyncio.create_task(consume())
        await asyncio.wait_for(started.wait(), timeout=5)
        await gui.stop_translate_file(state)
        updates = await asyncio.wait_for(consumer, timeout=5)
        assert released.is_set()
        assert len(updates) == 1 and len(updates[0]) == 19

    asyncio.run(exercise())
    assert state["current_task"] is None and state["uploaded_files"] == []


@pytest.mark.parametrize("exception_name", ["TranslationError", "BabeldocError"])
def test_structured_translation_error_keeps_original_message(
    setup_task, monkeypatch, exception_name
):
    gui, state, mono, _ = setup_task
    from pdf2zh_next import high_level

    async def stream(_settings, _path):
        yield {
            "type": "progress_start",
            "stage": "test",
            "overall_progress": 0,
            "part_index": 1,
            "total_parts": 1,
            "stage_current": 0,
            "stage_total": 1,
        }
        raise getattr(high_level, exception_name)("synthetic preserved error")

    monkeypatch.setattr(gui, "do_translate_async_stream", stream)
    settings = SimpleNamespace(basic=SimpleNamespace(input_files=set()))
    with pytest.raises(gr.Error, match="synthetic preserved error"):
        asyncio.run(
            gui._run_translation_task(settings, mono, state, lambda *_a, **_k: None)
        )
