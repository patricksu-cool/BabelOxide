import sys
from types import SimpleNamespace

import pytest


@pytest.fixture
def gui(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["pytest"])
    import pdf2zh_next.gui as gui_module

    return gui_module


def _uploaded(gui):
    return gui.on_file_upload(
        ["/upload/a.pdf", SimpleNamespace(name="/upload/b.pdf")], None
    )[1]


def test_upload_preserves_state_and_existing_results(gui):
    state = _uploaded(gui)
    state["results"]["a.pdf"] = {"mono": "mono.pdf", "dual": None, "glossary": None}
    result = gui.on_file_upload(["/new/a.pdf", "/upload/c.pdf"], state)
    assert len(result) == 3 and result[1] is state
    assert state["uploaded_files"] == ["a.pdf", "b.pdf", "c.pdf"]
    assert state["display_map"]["a.pdf"].replace("\\", "/") == "/new/a.pdf"
    assert state["results"]["a.pdf"]["mono"] == "mono.pdf"
    assert result[0]["value"] == "a.pdf"
    assert result[2]["value"] == "1. a.pdf\n2. b.pdf\n3. c.pdf"


@pytest.mark.parametrize("selected", [None, "missing.pdf"])
def test_preview_empty_or_stale_selection(gui, selected):
    state = _uploaded(gui)
    result = gui.update_preview(selected, state)
    assert len(result) == 7
    assert result[1] == (state["display_map"]["a.pdf"] if selected else None)
    assert all(update["visible"] is False for update in result[4:])


def test_preview_download_paths_and_visibility(gui):
    state = _uploaded(gui)
    state["results"]["a.pdf"] = {
        "mono": "mono.pdf",
        "dual": None,
        "glossary": "terms.csv",
    }
    result = gui.update_preview("a.pdf", state)
    assert result[:4] == ("mono.pdf", state["display_map"]["a.pdf"], None, "terms.csv")
    assert [update["visible"] for update in result[4:]] == [True, False, True]


@pytest.mark.parametrize("cancel_error", [False, True])
def test_clear_cancels_once_and_preserves_translated_files(gui, cancel_error):
    state = _uploaded(gui)
    calls = []

    def cancel():
        calls.append("cancel")
        if cancel_error:
            raise RuntimeError("synthetic cancellation error")

    state["current_task"] = SimpleNamespace(cancel=cancel)
    state["display_map"]["a.mono.pdf"] = "translated.pdf"
    state["parent_map"]["a.mono.pdf"] = "a.pdf"
    result = gui.on_file_clear([], state)
    assert len(result) == 10 and result[1] is state
    assert calls == ["cancel"] and state["current_task"] is None
    assert state["uploaded_files"] == []
    assert list(state["display_map"]) == ["a.mono.pdf"]
    assert result[0]["value"] == "a.mono.pdf"
    assert result[3:7] == (None, None, None, None)


@pytest.mark.parametrize("remove", [False, True])
def test_input_change_cancels_only_for_removal_and_keeps_safe_selection(gui, remove):
    state = _uploaded(gui)
    calls = []
    state["current_task"] = SimpleNamespace(cancel=lambda: calls.append("cancel"))
    files = ["/upload/b.pdf"] if remove else ["/upload/a.pdf", "/upload/b.pdf"]
    result = gui.on_file_input_change(files, state, "a.pdf")
    assert len(result) == 10 and result[1] is state
    assert calls == (["cancel"] if remove else [])
    assert result[0]["value"] == ("b.pdf" if remove else "a.pdf")
    assert result[3] == state["display_map"][result[0]["value"]]


def test_file_callbacks_keep_gradio_output_contract(gui):
    expected = {"on_file_upload": 3, "on_file_clear": 10, "on_file_input_change": 10}
    for name, count in expected.items():
        bound = [fn for fn in gui.demo.fns.values() if fn.fn and fn.fn.__name__ == name]
        assert bound, name
        assert all(len(fn.outputs) == count for fn in bound)


def test_session_factory_does_not_share_mutable_state():
    from pdf2zh_next.ui.session import new_session_state

    first, second = new_session_state(), new_session_state()
    first["uploaded_files"].append("one.pdf")
    first["display_map"]["one.pdf"] = "path"
    assert second["uploaded_files"] == []
    assert second["display_map"] == {}
