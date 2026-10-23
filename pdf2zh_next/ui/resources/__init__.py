"""Package-local GUI assets for source, wheel and PyStand execution."""

from pathlib import Path

RESOURCE_DIRECTORY = Path(__file__).resolve().parent
custom_css = (RESOURCE_DIRECTORY / "gui.css").read_text(encoding="utf-8")
pdf_preview_js = (RESOURCE_DIRECTORY / "preview.js").read_text(encoding="utf-8")
translation_file_path = RESOURCE_DIRECTORY / "translation.yaml"
