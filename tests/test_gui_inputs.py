import sys

import gradio as gr
import pytest


@pytest.fixture
def gui(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["pytest"])
    from pdf2zh_next import gui

    return gui


def test_input_copies_use_distinct_names_and_preserve_sources(gui, tmp_path):
    source = tmp_path / "paper.pdf"
    source.write_bytes(b"original")
    output = tmp_path / "output"
    output.mkdir()
    paths = gui._prepare_input_file("File", [str(source), str(source)], "", output)
    assert [p.name for p in paths] == ["paper.pdf", "paper_2.pdf"]
    assert source.read_bytes() == b"original"
    assert all(p.read_bytes() == b"original" for p in paths)


def test_input_fallback_skips_missing_uploads(gui, tmp_path):
    source = tmp_path / "paper.pdf"
    source.write_bytes(b"original")
    output = tmp_path / "output"
    output.mkdir()
    state = {
        "uploaded_files": ["paper.pdf", "gone.pdf"],
        "display_map": {
            "paper.pdf": str(source),
            "gone.pdf": str(tmp_path / "gone.pdf"),
        },
    }
    assert gui._prepare_input_file("File", None, "", output, state) == [
        output / "paper.pdf"
    ]


@pytest.mark.parametrize(
    ("kind", "error"), [("File", "No file input"), ("Link", "No link input")]
)
def test_empty_input_errors_are_preserved(gui, tmp_path, kind, error):
    with pytest.raises(gr.Error, match=error):
        gui._prepare_input_file(kind, None, "", tmp_path)
