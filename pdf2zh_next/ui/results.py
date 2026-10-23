"""Build GUI result updates and download archives without managing tasks."""

import zipfile
from pathlib import Path

import gradio as gr


def build_progress_update(state, preview_file_key=None):
    """Helper to generate the massive return tuple based on current state"""
    # Determine choices from display_map
    choices = list(state["display_map"].keys())

    # Determine values based on the preview_file_key (default to first processed file)
    current_res = {"mono": None, "dual": None, "glossary": None}
    preview_path = None

    if preview_file_key:
        preview_path = state["display_map"].get(preview_file_key)
        parent_key = state["parent_map"].get(preview_file_key)
        if parent_key and parent_key in state["results"]:
            current_res = state["results"][parent_key]

    # Build uploaded files markdown view
    uploaded_files = state.get("uploaded_files") or []
    if uploaded_files:
        uploaded_md = "\n".join(
            f"{idx + 1}. {name}" for idx, name in enumerate(uploaded_files)
        )
        uploaded_view_update = gr.update(value=uploaded_md, visible=True)
    else:
        uploaded_view_update = gr.update(value="", visible=False)

    return (
        current_res["mono"],
        preview_path,
        current_res["dual"],
        current_res["glossary"],
        gr.update(visible=bool(current_res["mono"])),
        gr.update(visible=bool(current_res["dual"])),
        gr.update(visible=bool(current_res["glossary"])),
        gr.update(visible=True, value="## Processing..."),  # Title
        gr.update(
            choices=choices, value=preview_file_key, visible=True
        ),  # Result Selector
        gr.update(visible=True),  # Result Selector Container
        gr.update(visible=False),  # Zip
        None,
        gr.update(visible=False),
        None,
        gr.update(visible=False),
        None,
        gr.update(visible=False),
        None,
        uploaded_view_update,
    )


def write_download_archives(results, all_path, mono_path, dual_path, glossary_path):
    """Preserve result order, archive names and empty per-kind ZIP creation."""
    fields = ("mono", "dual", "glossary")
    outputs = (
        (all_path, fields),
        (mono_path, ("mono",)),
        (dual_path, ("dual",)),
        (glossary_path, ("glossary",)),
    )
    present = dict.fromkeys(fields, False)
    for path, included in outputs:
        with zipfile.ZipFile(path, "w") as archive:
            for result in results.values():
                for field in included:
                    value = result[field]
                    if value:
                        archive.write(value, arcname=f"{field}_{Path(value).name}")
                        present[field] = True
    return present["mono"], present["dual"], present["glossary"]
