import gradio as gr
import pymupdf
import pytest

from pdf2zh_next.ui.preview import first_page
from pdf2zh_next.ui.preview import next_page
from pdf2zh_next.ui.preview import previous_page
from pdf2zh_next.ui.preview import render_page


@pytest.fixture
def pdf(tmp_path):
    path = tmp_path / "pages.pdf"
    with pymupdf.open() as document:
        for color in ((1, 0, 0), (0, 1, 0), (0, 0, 1)):
            page = document.new_page(width=200, height=300)
            page.draw_rect(page.rect, color=color, fill=color)
        document.save(path)
    return str(path)


def test_page_navigation_and_clamping(pdf):
    image, selector, label = first_page(pdf)
    assert image.getpixel((50, 50)) == (255, 0, 0)
    assert selector["value"] == 1 and selector["maximum"] == 3
    assert label == "Page 1 / 3"
    image, selector, _ = next_page(pdf, 1)
    assert image.getpixel((50, 50)) == (0, 255, 0)
    assert selector["value"] == 2
    assert previous_page(pdf, 1)[1]["value"] == 1
    assert next_page(pdf, 3)[1]["value"] == 3


def test_preview_clear_resets_page_and_image():
    image, selector, label = first_page(None)
    assert image is None and selector["value"] == 1 and label == ""


def test_large_page_raster_is_bounded(tmp_path):
    path = tmp_path / "large.pdf"
    with pymupdf.open() as document:
        document.new_page(width=10000, height=20000)
        document.save(path)
    image, _, _ = render_page(str(path))
    assert max(image.size) <= 1600


def test_unreadable_pdf_is_reported(tmp_path):
    path = tmp_path / "broken.pdf"
    path.write_bytes(b"not a pdf")
    with pytest.raises(gr.Error, match="Unable to preview"):
        first_page(str(path))
