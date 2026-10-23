"""GUI task lifecycle and result streaming, independent of component declarations."""

import asyncio
import logging
import uuid
import zipfile
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import gradio as gr

from pdf2zh_next.config.model import SettingsModel
from pdf2zh_next.i18n import gettext as _
from pdf2zh_next.ui.settings import SaveMode

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class TaskContext:
    """Callbacks resolve current settings and keep the controller out of gui.py."""

    build_inputs: Callable
    prepare_files: Callable
    build_settings: Callable
    get_settings: Callable
    run_task: Callable


async def run_translation_task(
    settings: SettingsModel,
    file_path: Path,
    state: dict,
    progress: gr.Progress,
    task_prefix: str = "",
    *,
    stream_factory,
    translation_error,
) -> tuple[Path | None, Path | None, Path | None, dict | None]:
    """
    This function runs the translation task and handles progress updates.

    Inputs:
        - settings: The translation settings
        - file_path: The path to the input file
        - state: The state dictionary for tracking the task
        - progress: The Gradio progress bar
        - task_prefix: A prefix string for progress description

    Returns:
        - A tuple of (mono_pdf_path, dual_pdf_path, glossary_path, token_usage)
    """
    mono_path = None
    dual_path = None
    glossary_path = None
    token_usage = None

    try:
        settings.basic.input_files = set()
        async for event in stream_factory(settings, file_path):
            if event["type"] in (
                "progress_start",
                "progress_update",
                "progress_end",
            ):
                # Update progress bar
                desc = event["stage"]
                progress_value = event["overall_progress"] / 100.0
                part_index = event["part_index"]
                total_parts = event["total_parts"]
                stage_current = event["stage_current"]
                stage_total = event["stage_total"]

                # Combine task prefix with current status
                full_desc = f"{task_prefix}{desc} ({part_index}/{total_parts}, {stage_current}/{stage_total})"
                logger.info(f"Progress: {progress_value}, {full_desc}")
                progress(progress_value, desc=full_desc)
            elif event["type"] == "finish":
                # Extract result paths
                result = event["translate_result"]
                mono_path = result.mono_pdf_path
                dual_path = result.dual_pdf_path
                glossary_path = result.auto_extracted_glossary_path
                token_usage = event.get("token_usage", {})
                # progress(1.0, desc=_("Translation complete!")) # Let caller handle final completion
                break
            elif event["type"] == "error":
                # Handle error event
                error_msg = event.get("error", "Unknown error")
                error_details = event.get("details", "")
                raise gr.Error(f"Translation error: {error_msg}")
    except asyncio.CancelledError:
        # Handle task cancellation - let translate_file handle the UI updates
        logger.info(
            f"Translation for session {state.get('session_id', 'unknown')} was cancelled"
        )
        raise  # Re-raise for the calling function to handle
    except translation_error as e:
        # Handle structured translation errors
        logger.error(f"Translation error: {e}")
        raise gr.Error(f"Translation error: {e.message}") from e
    except gr.Error as e:
        # Handle Gradio errors
        logger.error(f"Gradio error: {e}")
        raise
    except Exception as e:
        # Handle other exceptions
        logger.error(f"Error in _run_translation_task: {e}", exc_info=True)
        raise gr.Error(f"Translation failed: {e}") from e

    return mono_path, dual_path, glossary_path, token_usage


async def stop_translate_file(state: dict) -> None:
    """
    This function stops the translation process.

    Inputs:
        - state: The state of the translation process

    Returns:- None
    """
    if "current_task" not in state or state["current_task"] is None:
        return

    logger.info(
        f"Stopping translation for session {state.get('session_id', 'unknown')}"
    )
    # Cancel the task
    try:
        state["current_task"].cancel()
        # Wait briefly for cancellation to take effect
        await asyncio.sleep(0.1)
    except Exception as e:
        logger.error(f"Error stopping translation: {e}")
    finally:
        state["current_task"] = None


async def translate_files(
    file_type,
    file_input,
    link_input,
    *ui_args,
    progress=None,
    context: TaskContext,
):
    """
    This function translates PDF files and yields updates incrementally.
    """
    # Ensure there is a progress object
    if progress is None:
        progress = gr.Progress()

    # Build ui_inputs from *args
    ui_inputs = context.build_inputs(*ui_args)
    state = ui_inputs["state"]

    # Initialize session and output directory
    session_id = str(uuid.uuid4())
    state["session_id"] = session_id
    # Reset results for new translation run, but keep display_map for uploaded files
    state["results"] = {}
    state["file_order"] = []

    # Ensure display_map exists (it might have been populated by on_file_upload)
    if "display_map" not in state:
        state["display_map"] = {}
    if "parent_map" not in state:
        state["parent_map"] = {}

    # Prepare output directory
    output_dir = Path("pdf2zh_files") / session_id
    output_dir.mkdir(parents=True, exist_ok=True)

    zip_path = output_dir / "all_translations.zip"
    zip_mono_path = output_dir / "all_mono_translations.zip"
    zip_dual_path = output_dir / "all_dual_translations.zip"
    zip_glossary_path = output_dir / "all_glossaries.zip"

    # Default return values (Hidden/Empty)
    def get_current_ui_update(preview_file_key=None):
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

    try:
        # Step 1: Prepare input files
        file_paths = context.prepare_files(
            file_type, file_input, link_input, output_dir, state
        )
        total_files = len(file_paths)

        all_token_usage = {
            "total": 0,
            "prompt": 0,
            "cache_hit_prompt": 0,
            "completion": 0,
        }

        # Step 2: Iterate and Process each file
        for idx, file_path in enumerate(file_paths):
            filename = file_path.name
            current_file_idx = idx + 1
            task_prefix = f"[{current_file_idx}/{total_files}] {filename}: "

            logger.info(f"Processing file {current_file_idx}/{total_files}: {filename}")

            # Update display map for the original file being processed (if not already there)
            if filename not in state["display_map"]:
                state["display_map"][filename] = str(file_path)
                state["parent_map"][filename] = filename

            # Build translation settings
            translate_settings = context.build_settings(
                context.get_settings().clone(),
                file_path,
                output_dir,
                SaveMode.follow_settings,
                ui_inputs,
            )

            # Create task
            task = asyncio.create_task(
                context.run_task(
                    translate_settings,
                    file_path,
                    state,
                    progress,
                    task_prefix=task_prefix,
                )
            )
            state["current_task"] = task

            # Await result
            mono_path, dual_path, glossary_path, token_usage = await task

            # Store results
            result_entry = {
                "original_name": filename,
                "original_path": str(file_path),
                "mono": str(mono_path) if mono_path and mono_path.exists() else None,
                "dual": str(dual_path) if dual_path and dual_path.exists() else None,
                "glossary": str(glossary_path)
                if glossary_path and glossary_path.exists()
                else None,
                "token_usage": token_usage,
            }
            state["results"][filename] = result_entry
            state["file_order"].append(filename)

            # Update maps for dropdown
            if result_entry["mono"]:
                mono_label = f"{Path(filename).stem}_mono{Path(filename).suffix}"
                state["display_map"][mono_label] = result_entry["mono"]
                state["parent_map"][mono_label] = filename

            if result_entry["dual"]:
                dual_label = f"{Path(filename).stem}_dual{Path(filename).suffix}"
                state["display_map"][dual_label] = result_entry["dual"]
                state["parent_map"][dual_label] = filename

            # Accumulate tokens
            if token_usage:
                for key in ["main", "term"]:
                    if key in token_usage:
                        u = token_usage[key]
                        all_token_usage["total"] += u.get("total", 0)
                        all_token_usage["prompt"] += u.get("prompt", 0)
                        all_token_usage["cache_hit_prompt"] += u.get(
                            "cache_hit_prompt", 0
                        )
                        all_token_usage["completion"] += u.get("completion", 0)

            # YIELD UPDATE: Update the UI immediately after this file is done
            # Select the Dual version of the current file to show progress visually
            current_preview_label = filename
            if result_entry["dual"]:
                current_preview_label = (
                    f"{Path(filename).stem}_dual{Path(filename).suffix}"
                )
            elif result_entry["mono"]:
                current_preview_label = (
                    f"{Path(filename).stem}_mono{Path(filename).suffix}"
                )

            # Generate the intermediate UI update
            yield get_current_ui_update(current_preview_label)

        # Step 3: Finalize (Create Zips)
        # All files zip
        with zipfile.ZipFile(zip_path, "w") as zipf:
            for _fname, res in state["results"].items():
                if res["mono"]:
                    zipf.write(res["mono"], arcname=f"mono_{Path(res['mono']).name}")
                if res["dual"]:
                    zipf.write(res["dual"], arcname=f"dual_{Path(res['dual']).name}")
                if res["glossary"]:
                    zipf.write(
                        res["glossary"],
                        arcname=f"glossary_{Path(res['glossary']).name}",
                    )

        # Individual Zips
        has_mono, has_dual, has_glossary = False, False, False
        with zipfile.ZipFile(zip_mono_path, "w") as zipf:
            for _fname, res in state["results"].items():
                if res["mono"]:
                    zipf.write(res["mono"], arcname=f"mono_{Path(res['mono']).name}")
                    has_mono = True

        with zipfile.ZipFile(zip_dual_path, "w") as zipf:
            for _fname, res in state["results"].items():
                if res["dual"]:
                    zipf.write(res["dual"], arcname=f"dual_{Path(res['dual']).name}")
                    has_dual = True

        with zipfile.ZipFile(zip_glossary_path, "w") as zipf:
            for _fname, res in state["results"].items():
                if res["glossary"]:
                    zipf.write(
                        res["glossary"],
                        arcname=f"glossary_{Path(res['glossary']).name}",
                    )
                    has_glossary = True

        progress(1.0, desc=_("All translations complete!"))

        # Final UI State
        token_info = f"\n\n**Total Token Usage:** Total {all_token_usage['total']} (Prompt {all_token_usage['prompt']}, Cache Hit {all_token_usage['cache_hit_prompt']}, Completion {all_token_usage['completion']})"

        # Determine final preview (last file processed or first)
        last_file_key = state["file_order"][-1] if state["file_order"] else None
        final_preview_label = None

        if last_file_key:
            res = state["results"][last_file_key]
            final_preview_label = last_file_key
            if res["dual"]:
                final_preview_label = (
                    f"{Path(last_file_key).stem}_dual{Path(last_file_key).suffix}"
                )
            elif res["mono"]:
                final_preview_label = (
                    f"{Path(last_file_key).stem}_mono{Path(last_file_key).suffix}"
                )

        # Get the base tuple
        (
            final_mono,
            final_preview_path,
            final_dual,
            final_glossary,
            vis_mono,
            vis_dual,
            vis_glossary,
            _u1,
            selector_update,
            selector_vis,
            _u2,
            _u3,
            _u4,
            _u5,
            _u6,
            _u7,
            _u8,
            _u9,
            uploaded_view_update,
        ) = get_current_ui_update(final_preview_label)

        # After successful translation of all files, clear uploaded files for next batch
        if "uploaded_files" in state:
            state["uploaded_files"] = []
            uploaded_view_update = gr.update(value="", visible=False)

        # Yield Final Result with Zips visible
        yield (
            final_mono,
            final_preview_path,
            final_dual,
            final_glossary,
            vis_mono,
            vis_dual,
            vis_glossary,
            gr.update(value=f"{_('## Translated')}{token_info}"),  # Title updated
            selector_update,
            selector_vis,
            gr.update(visible=True),
            str(zip_path),  # Zip
            gr.update(visible=has_mono),
            str(zip_mono_path) if has_mono else None,
            gr.update(visible=has_dual),
            str(zip_dual_path) if has_dual else None,
            gr.update(visible=has_glossary),
            str(zip_glossary_path) if has_glossary else None,
            uploaded_view_update,
        )

    except asyncio.CancelledError:
        gr.Info(_("Translation cancelled"))
        state["uploaded_files"] = []
        yield (
            None,
            None,
            None,
            None,
            gr.update(visible=False),
            gr.update(visible=False),
            gr.update(visible=False),
            gr.update(visible=False),
            gr.update(choices=[], value=None, visible=False),
            gr.update(visible=False),
            gr.update(visible=False),
            None,
            gr.update(visible=False),
            None,
            gr.update(visible=False),
            None,
            gr.update(visible=False),
            None,
            gr.update(value="", visible=False),
        )
    except Exception as e:
        logger.exception(f"Error in translate_files: {e}")
        raise gr.Error(f"Translation failed: {e}") from e
    finally:
        state["current_task"] = None
