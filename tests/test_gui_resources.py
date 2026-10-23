import sys

import pytest


@pytest.fixture
def gui(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["pytest"])
    import pdf2zh_next.gui as module

    return module


def test_gui_resources_load_from_another_working_directory(gui, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    translations = gui.get_translation_dic(gui.translation_file_path)
    assert {"en", "zh", "zh-TW", "ja", "de"} <= set(translations)
    assert translations["zh-TW"]["Cancel"]
    assert ".pdf-preview-fixed" in gui.custom_css
    assert gui.logo_path.is_file()


def test_preview_uses_only_native_components(gui):
    components = gui.demo.config["components"]
    assert not any(item["type"] == "pdf" for item in components)
    assert any(item["type"] == "image" for item in components)


def test_native_preview_events_are_private(gui):
    callbacks = {"first_page", "render_page", "previous_page", "next_page"}
    bound = [
        fn for fn in gui.demo.fns.values() if fn.fn and fn.fn.__name__ in callbacks
    ]
    assert len(bound) == 4
    assert all(fn.api_visibility == "private" for fn in bound)
