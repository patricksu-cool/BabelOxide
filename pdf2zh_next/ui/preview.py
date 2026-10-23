"""PDF page preview using supported Gradio components and PyMuPDF.

Only one bounded raster is generated at a time; no custom Svelte bundle, CDN,
browser PDF plug-in, or document-wide image cache is required.
"""

import gradio as gr
import pymupdf
from PIL import Image


def render_page(path, page=1):
    """Return an image, clamped page selector and readable page count."""
    if not path:
        return None, gr.update(value=1, maximum=1), ""
    try:
        with pymupdf.open(path) as document:
            if not document.is_pdf or not document.page_count:
                raise ValueError("The selected file is not a readable PDF")
            index = max(0, min(int(page or 1) - 1, document.page_count - 1))
            pdf_page = document[index]
            scale = min(2.0, 1600 / max(pdf_page.rect.width, pdf_page.rect.height, 1))
            pixmap = pdf_page.get_pixmap(
                matrix=pymupdf.Matrix(scale, scale),
                colorspace=pymupdf.csRGB,
                alpha=False,
            )
            image = Image.frombytes(
                "RGB", (pixmap.width, pixmap.height), pixmap.samples
            )
            return (
                image,
                gr.update(value=index + 1, maximum=document.page_count),
                f"Page {index + 1} / {document.page_count}",
            )
    except (OSError, ValueError, RuntimeError) as exc:
        raise gr.Error("Unable to preview this PDF") from exc


def first_page(path):
    return render_page(path, 1)


def previous_page(path, page):
    return render_page(path, (page or 1) - 1)


def next_page(path, page):
    return render_page(path, (page or 1) + 1)


def create_pdf_preview():
    """Keep the existing file-output contract and bind a native page viewer."""
    source = gr.File(visible=False, file_types=[".pdf"], interactive=False)
    image = gr.Image(
        label="PDF preview",
        show_label=False,
        interactive=False,
        height=700,
        elem_classes=["pdf-preview-fixed"],
        buttons=["fullscreen"],
    )
    with gr.Row():
        previous = gr.Button("Previous page")
        page = gr.Number(value=1, minimum=1, precision=0, label="Page")
        following = gr.Button("Next page")
    status = gr.Markdown("")
    outputs = [image, page, status]
    source.change(first_page, source, outputs, api_visibility="private")
    page.input(render_page, [source, page], outputs, api_visibility="private")
    previous.click(previous_page, [source, page], outputs, api_visibility="private")
    following.click(next_page, [source, page], outputs, api_visibility="private")
    return source, image
