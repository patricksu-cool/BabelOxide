"""File upload, preview and download callbacks for Gradio sessions."""

import logging
from pathlib import Path

import gradio as gr

from pdf2zh_next.ui.session import new_session_state

logger = logging.getLogger("pdf2zh_next.gui")


def update_preview(selected_label, state):
    """
    Update preview based on selected label from dropdown.
    Modified to support previewing raw uploaded files before translation.
    """
    # 1. Basic validation
    if not selected_label or not state or "display_map" not in state:
        return (
            None,
            None,
            None,
            None,
            gr.update(visible=False),
            gr.update(visible=False),
            gr.update(visible=False),
        )

    # 1.5. Validate selected_label is in display_map (choices)
    # This prevents Gradio errors when value is not in choices
    if selected_label not in state.get("display_map", {}):
        # Reset to first available choice or None
        choices = list(state.get("display_map", {}).keys())
        selected_label = choices[0] if choices else None
        if not selected_label:
            return (
                None,
                None,
                None,
                None,
                gr.update(visible=False),
                gr.update(visible=False),
                gr.update(visible=False),
            )

    # 2. Get the file path for the PDF viewer
    # This works for both uploaded files and translated files
    preview_path = state["display_map"].get(selected_label)

    if not preview_path:
        return (
            None,
            None,
            None,
            None,
            gr.update(visible=False),
            gr.update(visible=False),
            gr.update(visible=False),
        )

    # 3. Try to get translation results for download buttons
    # If just uploaded (not translated), res will be None
    parent_key = state["parent_map"].get(selected_label)
    res = None
    if parent_key and "results" in state:
        res = state["results"].get(parent_key)

    # 4. Prepare return values
    # If res exists, show download buttons. If not, hide them.
    mono_path = res["mono"] if res else None
    dual_path = res["dual"] if res else None
    glossary_path = res["glossary"] if res else None

    return (
        mono_path,  # Download Mono Button Value
        preview_path,  # PDF Preview Value (Critical Fix: Always return this if found)
        dual_path,  # Download Dual Button Value
        glossary_path,  # Download Glossary Button Value
        gr.update(visible=bool(mono_path)),  # Mono Button Visibility
        gr.update(visible=bool(dual_path)),  # Dual Button Visibility
        gr.update(visible=bool(glossary_path)),  # Glossary Button Visibility
    )


def on_file_upload(files, state):
    """
    Handle file upload event to populate the preview dropdown immediately.
    """
    if not files:
        return (
            gr.update(choices=[], value=None, visible=False),
            state,
            gr.update(value="", visible=False),
        )

    # Initialize state if needed
    if not state:
        state = new_session_state()

    if "display_map" not in state:
        state["display_map"] = {}
    if "parent_map" not in state:
        state["parent_map"] = {}
    if "uploaded_files" not in state:
        state["uploaded_files"] = []

    # Clear previous maps if this is a fresh upload action/session logic implies reset
    # For additive behavior, we keep them. Assuming additive for multi-upload.

    new_choices = []

    # Process uploaded files
    for f in files:
        # Gradio passes NamedString or file path
        f_path = Path(f.name if hasattr(f, "name") else f)
        original_name = f_path.name

        # Add to display map (Original file)
        state["display_map"][original_name] = str(f_path)
        state["parent_map"][original_name] = original_name

        # Track uploaded files for this session
        if original_name not in state["uploaded_files"]:
            state["uploaded_files"].append(original_name)

        # Add to local list to update UI
        if original_name not in new_choices:
            new_choices.append(original_name)

    # Get all current choices from state to preserve history + new files
    all_choices = list(state["display_map"].keys())

    # Default to the first file of the new batch if available
    default_value = (
        new_choices[0] if new_choices else (all_choices[0] if all_choices else None)
    )

    # Ensure default_value is in choices or None to avoid Gradio errors
    if default_value and default_value not in all_choices:
        default_value = all_choices[0] if all_choices else None

    # Build uploaded files markdown view
    uploaded_files = state["uploaded_files"]
    if uploaded_files:
        uploaded_md = "\n".join(
            f"{idx + 1}. {name}" for idx, name in enumerate(uploaded_files)
        )
        uploaded_view_update = gr.update(value=uploaded_md, visible=True)
    else:
        uploaded_view_update = gr.update(value="", visible=False)

    return (
        gr.update(choices=all_choices, value=default_value, visible=bool(all_choices)),
        state,
        uploaded_view_update,
    )


def on_file_clear(_files, state):
    """
    Handle file clear/delete event to clean up state and UI.
    When user clicks the X button on file input, this function removes uploaded files
    from state while preserving translation results.
    Also cancels any ongoing translation task.
    """
    # Initialize state if needed
    if not state:
        state = new_session_state()

    if "display_map" not in state:
        state["display_map"] = {}
    if "parent_map" not in state:
        state["parent_map"] = {}
    if "uploaded_files" not in state:
        state["uploaded_files"] = []

    # Cancel any ongoing translation task
    if "current_task" in state and state["current_task"] is not None:
        try:
            state["current_task"].cancel()
            logger.info("Translation task cancelled due to file clear")
        except Exception as e:
            logger.error(f"Error cancelling translation task: {e}")
        finally:
            state["current_task"] = None

    # Get list of uploaded files to remove
    uploaded_files_to_remove = list(state["uploaded_files"])

    # Remove uploaded files from display_map and parent_map
    # But preserve translation results (mono/dual files)
    for original_name in uploaded_files_to_remove:
        # Remove original file entry from display_map
        if original_name in state["display_map"]:
            del state["display_map"][original_name]
        # Remove original file entry from parent_map
        if original_name in state["parent_map"]:
            del state["parent_map"][original_name]

    # Clear uploaded_files list
    state["uploaded_files"] = []

    # Build remaining choices (only translation results if any)
    remaining_choices = list(state["display_map"].keys())

    # Update UI components
    if remaining_choices:
        # If there are translation results, show them in dropdown
        default_value = remaining_choices[0]
        selector_update = gr.update(
            choices=remaining_choices, value=default_value, visible=True
        )
    else:
        # No files at all, hide dropdown
        default_value = None
        selector_update = gr.update(choices=[], value=None, visible=False)

    # Hide uploaded files view
    uploaded_view_update = gr.update(value="", visible=False)

    return (
        selector_update,
        state,
        uploaded_view_update,
        None,  # Clear preview
        None,  # Clear mono download
        None,  # Clear dual download
        None,  # Clear glossary download
        gr.update(visible=False),  # Hide mono button
        gr.update(visible=False),  # Hide dual button
        gr.update(visible=False),  # Hide glossary button
    )


def on_file_input_change(files, state, selected_label):
    """
    Handle per-file delete in the File(s) component (the small X on each row).

    Gradio triggers `change` when the list changes (add/remove). We diff against
    state["uploaded_files"] (which tracks current uploaded originals) to find removed
    items and keep state + preview selector consistent.
    """
    # Normalize files -> current names
    current_names: list[str] = []
    if files:
        for f in files:
            p = Path(f.name if hasattr(f, "name") else f)
            current_names.append(p.name)

    # Initialize state if needed
    if not state:
        state = new_session_state()
    state.setdefault("display_map", {})
    state.setdefault("parent_map", {})
    state.setdefault("uploaded_files", [])

    prev_names = list(state.get("uploaded_files") or [])
    removed = [n for n in prev_names if n not in current_names]

    # If something was removed while translating, cancel to avoid processing stale inputs.
    if removed and state.get("current_task") is not None:
        try:
            state["current_task"].cancel()
            logger.info("Translation task cancelled due to per-file removal")
        except Exception as e:
            logger.error(f"Error cancelling translation task: {e}")
        finally:
            state["current_task"] = None

    # Remove deleted originals from display_map/parent_map
    for name in removed:
        state["display_map"].pop(name, None)
        state["parent_map"].pop(name, None)

    # Update uploaded_files to match current file_input
    state["uploaded_files"] = current_names

    # Rebuild uploaded files markdown view
    if current_names:
        uploaded_md = "\n".join(
            f"{idx + 1}. {name}" for idx, name in enumerate(current_names)
        )
        uploaded_view_update = gr.update(value=uploaded_md, visible=True)
    else:
        uploaded_view_update = gr.update(value="", visible=False)

    # Rebuild selector choices from state (includes translations + remaining originals)
    choices = list(state["display_map"].keys())

    # If current preview was removed, pick a safe fallback; otherwise keep selection.
    # Ensure selector_value is always in choices or None to avoid Gradio errors
    if selected_label in removed:
        selected_label = None
    if selected_label and selected_label in choices:
        selector_value = selected_label
    else:
        selector_value = choices[0] if choices else None

    # Always ensure value is in choices or None
    if selector_value and selector_value not in choices:
        selector_value = choices[0] if choices else None

    selector_update = (
        gr.update(choices=choices, value=selector_value, visible=bool(choices))
        if choices
        else gr.update(choices=[], value=None, visible=False)
    )

    # Update preview + download buttons based on new selection
    (
        mono_path,
        preview_path,
        dual_path,
        glossary_path,
        vis_mono,
        vis_dual,
        vis_glossary,
    ) = (
        update_preview(selector_value, state)
        if selector_value
        else (
            None,
            None,
            None,
            None,
            gr.update(visible=False),
            gr.update(visible=False),
            gr.update(visible=False),
        )
    )

    return (
        selector_update,
        state,
        uploaded_view_update,
        preview_path,  # preview
        mono_path,
        dual_path,
        glossary_path,
        vis_mono,
        vis_dual,
        vis_glossary,
    )
