import shutil
import subprocess
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


def test_preview_script_is_bound_once_without_python_callback(gui):
    dependencies = [
        item
        for item in gui.demo.config["dependencies"]
        if item.get("js") and "adjustPDFCanvasScale" in item["js"]
    ]
    assert len(dependencies) == 1
    assert dependencies[0]["inputs"] == []
    assert dependencies[0]["outputs"] == []


def test_preview_script_is_a_callable_javascript_function(gui, tmp_path):
    node = shutil.which("node")
    if node is None:
        pytest.skip("Node.js is required for the optional JavaScript syntax check")
    script = gui.translation_file_path.parent / "preview.js"
    runner = (
        "const fs = require('node:fs');"
        "const source = fs.readFileSync(process.argv[1], 'utf8');"
        "const callback = new Function('return (' + source + ')')();"
        "if (typeof callback !== 'function') throw new Error('Expected a callback function');"
        "const originalFetch = () => {};"
        "global.window = {fetch: originalFetch, addEventListener: () => {}};"
        "global.document = {readyState: 'complete', querySelectorAll: () => [], body: {}};"
        "global.MutationObserver = class {observe() {}};"
        "callback();"
        "if (window.fetch !== originalFetch) throw new Error('Preview must preserve the component worker fetch');"
    )
    result = subprocess.run(
        [node, "-e", runner, str(script)],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
