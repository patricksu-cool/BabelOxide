import asyncio
import csv
import io
import logging
import tempfile
import typing
import uuid
import zipfile
from pathlib import Path

import chardet
import gradio as gr
import yaml
from babeldoc import __version__ as babeldoc_version
from gradio_i18n import Translate

from pdf2zh_next import __version__
from pdf2zh_next.config import ConfigManager
from pdf2zh_next.config.cli_env_model import CLIEnvSettingsModel
from pdf2zh_next.config.model import SettingsModel
from pdf2zh_next.config.translate_engine_model import GUI_PASSWORD_FIELDS
from pdf2zh_next.config.translate_engine_model import GUI_SENSITIVE_FIELDS
from pdf2zh_next.config.translate_engine_model import TERM_EXTRACTION_ENGINE_METADATA
from pdf2zh_next.config.translate_engine_model import TRANSLATION_ENGINE_METADATA
from pdf2zh_next.config.translate_engine_model import TRANSLATION_ENGINE_METADATA_MAP
from pdf2zh_next.const import DEFAULT_CONFIG_DIR
from pdf2zh_next.const import DEFAULT_CONFIG_FILE
from pdf2zh_next.high_level import TranslationError
from pdf2zh_next.high_level import do_translate_async_stream
from pdf2zh_next.i18n import LANGUAGES
from pdf2zh_next.i18n import gettext as _
from pdf2zh_next.i18n import update_current_languages
from pdf2zh_next.security_logging import install_redacting_filters
from pdf2zh_next.ui import settings as gui_settings
from pdf2zh_next.ui.fields import _field_gui_extra as _field_gui_extra
from pdf2zh_next.ui.fields import (
    _gui_dependency_field_name as _gui_dependency_field_name,
)
from pdf2zh_next.ui.fields import _gui_field_update as _gui_field_update
from pdf2zh_next.ui.fields import _gui_field_value as _gui_field_value
from pdf2zh_next.ui.fields import _gui_field_visible as _gui_field_visible
from pdf2zh_next.ui.fields import _type_hint_allows_none as _type_hint_allows_none
from pdf2zh_next.ui.files import on_file_clear
from pdf2zh_next.ui.files import on_file_input_change
from pdf2zh_next.ui.files import on_file_upload
from pdf2zh_next.ui.files import update_preview
from pdf2zh_next.ui.inputs import _get_unique_dest_path as _get_unique_dest_path
from pdf2zh_next.ui.inputs import _prepare_input_file as _prepare_input_file
from pdf2zh_next.ui.inputs import download_with_limit as download_with_limit
from pdf2zh_next.ui.languages import lang_map as lang_map
from pdf2zh_next.ui.languages import page_map as page_map
from pdf2zh_next.ui.languages import rev_lang_map as rev_lang_map
from pdf2zh_next.ui.preview import create_pdf_preview
from pdf2zh_next.ui.rate_options import (
    _calculate_rate_limit_params as _calculate_rate_limit_params,
)
from pdf2zh_next.ui.rate_options import (
    _validate_rate_limit_inputs as _validate_rate_limit_inputs,
)
from pdf2zh_next.ui.resources import custom_css
from pdf2zh_next.ui.resources import translation_file_path as translation_file_path
from pdf2zh_next.ui.session import new_session_state
from pdf2zh_next.ui.settings import SaveMode

logger = logging.getLogger(__name__)


def get_translation_dic(file_path: Path):
    with file_path.open(encoding="utf-8", newline="\n") as f:
        return yaml.safe_load(f)


__gui_service_arg_names = []
__gui_term_service_arg_names = []
LLM_support_index_map = {}


# The following variables associate strings with specific languages


# The following variable associate strings with page ranges
# Page map with fixed internal keys


def get_page_choices():
    """Get page range choices with translated labels"""
    return [
        (_("All"), "All"),
        (_("First"), "First"),
        (_("First 5 pages"), "First 5 pages"),
        (_("Range"), "Range"),
    ]


# Load configuration
config_manager = ConfigManager()
try:
    # Load configuration from files and environment variables
    settings = config_manager.initialize_cli_config()
    # Check if sensitive inputs should be disabled in GUI
    disable_sensitive_input = settings.gui_settings.disable_gui_sensitive_input
except Exception as e:
    logger.warning(f"Could not load initial config: {e}")
    settings = CLIEnvSettingsModel()
    disable_sensitive_input = False

# Define default values
default_lang_from = rev_lang_map.get(settings.translation.lang_in, "English")

default_lang_to = settings.translation.lang_out
for display_name, code in lang_map.items():
    if code == default_lang_to:
        default_lang_to = display_name
        break
else:
    default_lang_to = "Simplified Chinese"  # Fallback

# Available translation services
# This will eventually be dynamically determined based on available translators
available_services = [x.translate_engine_type for x in TRANSLATION_ENGINE_METADATA]

if settings.gui_settings.enabled_services:
    enabled_services = {
        x.lower() for x in settings.gui_settings.enabled_services.split(",")
    }
    available_services = [
        x for x in available_services if x.lower() in enabled_services
    ]

assert available_services, "No translation service is enabled"


disable_gui_sensitive_input = settings.gui_settings.disable_gui_sensitive_input


def _settings_saved(saved_settings):
    global settings
    settings = saved_settings


def _build_translate_settings(
    base_settings: CLIEnvSettingsModel,
    file_path: Path,
    output_dir: Path,
    save_mode: SaveMode,
    ui_inputs: dict,
) -> SettingsModel:
    return gui_settings.build_translate_settings(
        base_settings,
        file_path,
        output_dir,
        save_mode,
        ui_inputs,
        context=gui_settings.SettingsContext(
            config_manager=config_manager,
            lang_map=lang_map,
            page_map=page_map,
            disable_sensitive_input=disable_gui_sensitive_input,
            on_saved=_settings_saved,
        ),
    )


def _build_glossary_list(glossary_file, service_name=None):
    if not LLM_support_index_map.get(service_name, False):
        return None
    glossary_list = []
    if glossary_file is None:
        return None
    for file in glossary_file:
        try:
            f = io.StringIO(file.decode(chardet.detect(file)["encoding"]))
            with tempfile.NamedTemporaryFile(
                mode="w", delete=False, suffix=".csv"
            ) as temp_file:
                temp_file.write(f.getvalue())
                f.close()
            glossary_list.append(temp_file.name)
        except (UnicodeDecodeError, csv.Error, KeyError) as e:
            logger.error(f"Error processing glossary file: {e}")
            gr.Error(f"Failed to process glossary file: {e}")
    return ",".join(glossary_list)


def build_ui_inputs(*args):
    return gui_settings.build_ui_inputs(
        *args,
        service_arg_names=__gui_service_arg_names,
        term_service_arg_names=__gui_term_service_arg_names,
        build_glossary_list=_build_glossary_list,
    )


async def _run_translation_task(
    settings: SettingsModel,
    file_path: Path,
    state: dict,
    progress: gr.Progress,
    task_prefix: str = "",
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
        async for event in do_translate_async_stream(settings, file_path):
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
    except TranslationError as e:
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
):
    """
    This function translates PDF files and yields updates incrementally.
    """
    # Ensure there is a progress object
    if progress is None:
        progress = gr.Progress()

    # Build ui_inputs from *args
    ui_inputs = build_ui_inputs(*ui_args)
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
        file_paths = _prepare_input_file(
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
            translate_settings = _build_translate_settings(
                settings.clone(),
                file_path,
                output_dir,
                SaveMode.follow_settings,
                ui_inputs,
            )

            # Create task
            task = asyncio.create_task(
                _run_translation_task(
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


def swap_languages(lang_from_value, lang_to_value):
    """
    交换源语言和目标语言的选择。
    """
    return lang_to_value, lang_from_value


def save_config(
    *ui_args,
    progress=None,
):
    """
    This function saves the translation configuration.

    Inputs:
        - *ui_args: UI setting controls (see build_ui_inputs for details)
        - progress: The progress bar
    """
    # Setup progress tracking
    if progress is None:
        progress = gr.Progress()

    # Build ui_inputs from *args
    ui_inputs = build_ui_inputs(*ui_args)

    # Track progress
    progress(0, desc=_("Saving configuration..."))

    # Prepare output directory
    output_dir = Path("pdf2zh_files")

    _build_translate_settings(
        settings.clone(), config_fake_pdf_path, output_dir, SaveMode.always, ui_inputs
    )

    # Show success message
    gr.Info(_("Configuration saved to: {path}").format(path=DEFAULT_CONFIG_FILE))


# Custom theme definition
custom_blue = gr.themes.Color(
    c50="#E8F3FF",
    c100="#BEDAFF",
    c200="#94BFFF",
    c300="#6AA1FF",
    c400="#4080FF",
    c500="#165DFF",  # Primary color
    c600="#0E42D2",
    c700="#0A2BA6",
    c800="#061D79",
    c900="#03114D",
    c950="#020B33",
)


# Build paths to resources
current_dir = Path(__file__).parent
assets_dir = current_dir / "assets"
logo_path = assets_dir / "powered_by_siliconflow_light.png"

config_fake_pdf_path = DEFAULT_CONFIG_DIR / "config.fake.pdf"
pdf_preview_allowed_paths = [
    logo_path,
    Path("pdf2zh_files").resolve(),  # translation outputs
]

if not config_fake_pdf_path.exists():
    with config_fake_pdf_path.open("w") as f:
        f.write("This is a fake PDF file for configuration saving.")
        f.flush()


tech_details_string = f"""
                    <summary>Technical details</summary>
                    - ⭐ Star at GitHub: <a href="https://github.com/PDFMathTranslate-next/PDFMathTranslate-next">PDFMathTranslate-next/PDFMathTranslate-next</a><br>
                    - BabelDOC: <a href="https://github.com/funstory-ai/BabelDOC">funstory-ai/BabelDOC</a><br>
                    - GUI by: <a href="https://github.com/reycn">Rongxin</a> & <a href="https://github.com/hellofinch">hellofinch</a> & <a href="https://github.com/awwaawwa">awwaawwa</a> & <a href="https://github.com/zfb132">zfb132</a><br>
                    - pdf2zh Version: {__version__} <br>
                    - BabelDOC Version: {babeldoc_version}<br>
                    - Free translation service provided by <a href="https://siliconflow.cn/" target="_blank" style="text-decoration: none;">SiliconFlow</a><br>
                    <a href="https://siliconflow.cn/" target="_blank" style="text-decoration: none;">
                        <img src="/gradio_api/file={logo_path}" alt="Powered By SiliconFlow" style="height: 40px; margin-top: 10px; background-color: #ffffff; padding: 4px; border-radius: 4px;">
                    </a>
                    <br>
                """
update_current_languages(settings.gui_settings.ui_lang)
# The following code creates the GUI
with gr.Blocks(
    title="PDFMathTranslate - PDF Translation with preserved formats",
) as demo:
    lang_selector = gr.Dropdown(
        choices=LANGUAGES,
        label=_("UI Language"),
        value=settings.gui_settings.ui_lang,
        render=False,
    )
    with Translate(get_translation_dic(translation_file_path), lang_selector):
        gr.Markdown("# [PDFMathTranslate Next](https://pdf2zh-next.com)")

        translation_engine_arg_inputs = []
        detail_text_inputs = []
        detail_text_input_field_names = []
        detail_text_input_fields = []
        detail_visibility_dependency_inputs = {}
        detail_visibility_dependency_events = []
        require_llm_translator_inputs = []
        detail_text_input_index_map = {}
        term_detail_text_inputs = []
        term_detail_text_input_field_names = []
        term_detail_text_input_fields = []
        term_detail_visibility_dependency_inputs = {}
        term_detail_visibility_dependency_events = []
        term_detail_text_input_index_map = {}
        LLM_support_index_map.clear()
        with gr.Row(elem_classes=["tab-main-row"], equal_height=True):
            # 左侧侧边栏
            with gr.Column(scale=0, min_width=70, elem_classes=["sidebar-nav"]):
                btn_main_tab = gr.Button(
                    "🚀", variant="primary", elem_classes=["sidebar-btn"]
                )
                btn_settings_tab = gr.Button(
                    "⚙️", variant="secondary", elem_classes=["sidebar-btn"]
                )

            # 右侧主内容区域：再拆成“主页”和“设置”两个分组
            with gr.Column(scale=1):
                with gr.Group(visible=True) as tab_main:
                    # 主页内部使用左右两列布局：左侧基础设置/翻译结果，右侧预览
                    with gr.Row():
                        with gr.Column(scale=1):
                            gr.Markdown(_("## File(s)"), elem_classes=["tab-title"])
                            file_type = gr.Radio(
                                choices=[("File(s)", "File"), ("Link", "Link")],
                                label="Type",
                                value="File",
                            )
                            file_input = gr.File(
                                label=_("File(s)"),
                                file_count="multiple",
                                file_types=[".pdf", ".PDF"],
                                type="filepath",
                                elem_classes=["input-file"],
                            )
                            link_input = gr.Textbox(
                                label=_("Link"),
                                visible=False,
                                interactive=True,
                            )
                            uploaded_files_view = gr.Markdown(
                                label=_("Uploaded files (this session)"),
                                value="",
                                visible=False,
                                elem_classes=["uploaded-files-list"],
                            )

                            gr.Markdown(
                                _("## Translation Options"), elem_classes=["tab-title"]
                            )

                            # 语言选择与交换按钮所在的一行
                            with gr.Row(elem_classes=["lang-row"]):
                                lang_from = gr.Dropdown(
                                    label=_("Translate from"),
                                    choices=list(lang_map.keys()),
                                    value=default_lang_from,
                                )
                                # 交换语言按钮：放在两个下拉框之间
                                swap_lang_btn = gr.Button(
                                    "⇄",
                                    elem_classes=["lang-swap-btn"],
                                    scale=0,
                                    min_width=40,
                                )
                                lang_to = gr.Dropdown(
                                    label=_("Translate to"),
                                    choices=list(lang_map.keys()),
                                    value=default_lang_to,
                                )

                            # 主界面左侧保留翻译按钮和已翻译下载区
                            output_title = gr.Markdown(
                                _("## Translated"), visible=False
                            )
                            output_file_mono = gr.File(
                                label=_("Download Translation (Mono)"), visible=False
                            )
                            output_file_dual = gr.File(
                                label=_("Download Translation (Dual)"), visible=False
                            )
                            output_file_glossary = gr.File(
                                label=_("Download automatically extracted glossary"),
                                visible=False,
                            )
                            output_file_zip = gr.File(
                                label=_("Download All (ZIP)"), visible=False
                            )
                            output_file_zip_mono = gr.File(
                                label=_("Download All Mono (ZIP)"), visible=False
                            )
                            output_file_zip_dual = gr.File(
                                label=_("Download All Dual (ZIP)"), visible=False
                            )
                            output_file_zip_glossary = gr.File(
                                label=_("Download All Glossaries (ZIP)"), visible=False
                            )
                            # 操作按钮一行展示：翻译 / 取消 两个圆角按钮
                            with gr.Row(elem_classes=["action-row"]):
                                translate_btn = gr.Button(
                                    _("Translate"),
                                    variant="primary",
                                    elem_classes=["action-btn", "action-btn-primary"],
                                )
                                cancel_btn = gr.Button(
                                    _("Cancel"),
                                    variant="secondary",
                                    elem_classes=["action-btn", "action-btn-secondary"],
                                )

                        with gr.Column(scale=2):
                            gr.Markdown(_("## Preview"), elem_classes=["tab-title"])
                            # 结果选择 + 预览
                            result_file_selector = gr.Dropdown(
                                label=_("Select File to Preview/Download"),
                                choices=[],
                                value=None,
                                visible=True,
                                interactive=True,
                            )
                            # 预览区域上方已经有“## Preview”标题，这里关闭组件自带 label，
                            # 避免 Gradio 的标签卡片在布局调整后漂到左侧中间
                            preview, preview_image = create_pdf_preview()

                # 其余高级配置都移动到设置页
                with gr.Group(
                    visible=False, elem_classes=["settings-container"]
                ) as tab_settings:
                    # 界面语言切换只在“设置”页展示
                    lang_selector.render()
                    siliconflow_free_acknowledgement = gr.Markdown(
                        _(
                            "Free translation service provided by [SiliconFlow](https://siliconflow.cn)"
                        ),
                        visible=True,
                    )

                    detail_index = 0
                    term_detail_index = 0
                    with gr.Group() as translation_engine_settings:
                        service = gr.Dropdown(
                            label=_("Service"),
                            choices=available_services,
                            value=available_services[0],
                        )

                        __gui_service_arg_names = []
                        for service_name in available_services:
                            metadata = TRANSLATION_ENGINE_METADATA_MAP[service_name]
                            LLM_support_index_map[metadata.translate_engine_type] = (
                                metadata.support_llm
                            )
                            if not metadata.cli_detail_field_name:
                                # no detail field, no need to show
                                continue
                            detail_settings = getattr(
                                settings, metadata.cli_detail_field_name
                            )
                            visible = service.value == metadata.translate_engine_type

                            # OpenAI specific settings (initially visible if OpenAI is default)
                            with gr.Group(visible=True) as service_detail:
                                detail_text_input_index_map[
                                    metadata.translate_engine_type
                                ] = []
                                for (
                                    field_name,
                                    field,
                                ) in metadata.setting_model_type.model_fields.items():
                                    if disable_gui_sensitive_input:
                                        if field_name in GUI_SENSITIVE_FIELDS:
                                            continue
                                        if field_name in GUI_PASSWORD_FIELDS:
                                            continue
                                    if field.default_factory:
                                        continue

                                    if field_name == "translate_engine_type":
                                        continue
                                    if field_name == "support_llm":
                                        continue
                                    type_hint = field.annotation
                                    original_type = typing.get_origin(type_hint)
                                    type_args = typing.get_args(type_hint)
                                    value = getattr(detail_settings, field_name)
                                    gui_extra = _field_gui_extra(field)
                                    field_values = {
                                        name: getattr(detail_settings, name)
                                        for name in metadata.setting_model_type.model_fields
                                    }
                                    field_visible = _gui_field_visible(
                                        field_name,
                                        field,
                                        visible,
                                        field_values,
                                    )
                                    if gui_extra.get("widget") == "dropdown":
                                        field_input = gr.Dropdown(
                                            label=field.description,
                                            choices=gui_extra["choices"],
                                            value=_gui_field_value(field, value),
                                            interactive=True,
                                            visible=field_visible,
                                        )
                                    elif (
                                        type_hint is str
                                        or str in type_args
                                        or type_hint is int
                                        or int in type_args
                                    ):
                                        if field_name in GUI_PASSWORD_FIELDS:
                                            field_input = gr.Textbox(
                                                label=field.description,
                                                value=value,
                                                interactive=True,
                                                type="password",
                                                visible=field_visible,
                                            )
                                        else:
                                            field_input = gr.Textbox(
                                                label=field.description,
                                                value=value,
                                                interactive=True,
                                                visible=field_visible,
                                            )
                                    elif type_hint is bool or bool in type_args:
                                        field_input = gr.Checkbox(
                                            label=field.description,
                                            value=value,
                                            interactive=True,
                                            visible=field_visible,
                                        )
                                    else:
                                        raise Exception(
                                            f"Unsupported type {type_hint} for field {field_name} in gui translation engine settings"
                                        )
                                    detail_text_input_index_map[
                                        metadata.translate_engine_type
                                    ].append(detail_index)
                                    detail_index += 1
                                    detail_text_inputs.append(field_input)
                                    detail_text_input_field_names.append(field_name)
                                    detail_text_input_fields.append(field)
                                    visible_when = gui_extra.get("visible_when")
                                    if visible_when:
                                        dependency_field_name = (
                                            _gui_dependency_field_name(
                                                field_name, visible_when["field"]
                                            )
                                        )
                                        dependency_input = (
                                            detail_visibility_dependency_inputs.get(
                                                dependency_field_name
                                            )
                                        )
                                        if dependency_input is not None:
                                            detail_visibility_dependency_events.append(
                                                (
                                                    metadata.translate_engine_type,
                                                    field_name,
                                                    field,
                                                    dependency_input,
                                                    field_input,
                                                )
                                            )
                                    detail_visibility_dependency_inputs[field_name] = (
                                        field_input
                                    )
                                    __gui_service_arg_names.append(field_name)
                                    translation_engine_arg_inputs.append(field_input)
                    with gr.Group() as rate_limit_settings:
                        rate_limit_mode = gr.Radio(
                            choices=[
                                ("RPM (Requests Per Minute)", "RPM"),
                                ("Concurrent Requests", "Concurrent Threads"),
                                ("Custom", "Custom"),
                            ],
                            label="Rate Limit Mode",
                            value="Custom",
                            interactive=True,
                            visible=False,
                            info="Select the rate limit mode that best suits your API provider, system will automatically convert the rate limiting values of RPM or Concurrent Requests to QPS and Pool Max Workers when you click the Translate button",
                        )

                        rpm_input = gr.Number(
                            label=_("RPM (Requests Per Minute)"),
                            value=240,  # More conservative default value
                            precision=0,
                            minimum=1,
                            maximum=60000,
                            interactive=True,
                            visible=False,
                            info=_(
                                "Most API providers provide this parameter, such as OpenAI GPT-4: 500 RPM"
                            ),
                        )

                        concurrent_threads_input = gr.Number(
                            label=_("Concurrent Threads"),
                            value=20,  # More conservative default value
                            precision=0,
                            minimum=1,
                            maximum=1000,
                            interactive=True,
                            visible=False,
                            info=_(
                                "Maximum number of requests processed simultaneously"
                            ),
                        )

                        custom_qps_input = gr.Number(
                            label=_("QPS (Queries Per Second)"),
                            value=settings.translation.qps or 4,
                            precision=0,
                            minimum=1,
                            maximum=1000,
                            interactive=True,
                            visible=False,
                            info=_("Number of requests sent per second"),
                        )

                        custom_pool_max_workers_input = gr.Number(
                            label=_("Pool Max Workers"),
                            value=settings.translation.pool_max_workers,
                            precision=0,
                            minimum=0,
                            maximum=1000,
                            interactive=True,
                            visible=False,
                            info=_(
                                "If not set or set to 0, QPS will be used as the number of workers"
                            ),
                        )

                    # Term extraction options (engine + rate limit + detail settings)
                    with gr.Accordion(_("Auto Term Extraction"), open=True):
                        enable_auto_term_extraction = gr.Checkbox(
                            label=_("Enable auto term extraction"),
                            value=not settings.translation.no_auto_extract_glossary,
                            interactive=True,
                        )

                        term_disabled_info = gr.Markdown(
                            _(
                                "Auto term extraction is disabled. Term extraction settings below will not take effect until it is enabled."
                            ),
                            visible=settings.translation.no_auto_extract_glossary,
                        )

                        with gr.Group(visible=True) as term_settings_group:
                            term_service = gr.Dropdown(
                                label=_("Term extraction engine"),
                                choices=[
                                    (
                                        _("Follow main translation engine"),
                                        "Follow main translation engine",
                                    )
                                ]
                                + [
                                    metadata.translate_engine_type
                                    for metadata in TERM_EXTRACTION_ENGINE_METADATA
                                ],
                                value="Follow main translation engine",
                            )

                            # Term engine detail settings
                            __gui_term_service_arg_names = []
                            for term_metadata in TERM_EXTRACTION_ENGINE_METADATA:
                                if not term_metadata.cli_detail_field_name:
                                    continue
                                term_detail_field_name = (
                                    f"term_{term_metadata.cli_detail_field_name}"
                                )
                                term_detail_settings = getattr(
                                    settings, term_detail_field_name
                                )

                                # Term engine settings group should stay visible;
                                # visibility is controlled by each field input.
                                with gr.Group() as term_service_detail:
                                    term_detail_text_input_index_map[
                                        term_metadata.translate_engine_type
                                    ] = []
                                    for (
                                        field_name,
                                        field,
                                    ) in term_metadata.term_setting_model_type.model_fields.items():
                                        if field_name in (
                                            "translate_engine_type",
                                            "support_llm",
                                        ):
                                            continue
                                        if field.default_factory:
                                            continue

                                        base_field_name = field_name
                                        if base_field_name.startswith("term_"):
                                            base_name = base_field_name[len("term_") :]
                                        else:
                                            base_name = base_field_name

                                        if disable_gui_sensitive_input:
                                            if base_name in GUI_SENSITIVE_FIELDS:
                                                continue
                                            if base_name in GUI_PASSWORD_FIELDS:
                                                continue

                                        type_hint = field.annotation
                                        original_type = typing.get_origin(type_hint)
                                        type_args = typing.get_args(type_hint)
                                        value = getattr(
                                            term_detail_settings, field_name
                                        )
                                        gui_extra = _field_gui_extra(field)
                                        term_field_values = {
                                            name: getattr(term_detail_settings, name)
                                            for name in term_metadata.term_setting_model_type.model_fields
                                        }
                                        field_visible = _gui_field_visible(
                                            field_name,
                                            field,
                                            False,
                                            term_field_values,
                                        )

                                        if gui_extra.get("widget") == "dropdown":
                                            field_input = gr.Dropdown(
                                                label=field.description,
                                                choices=gui_extra["choices"],
                                                value=_gui_field_value(field, value),
                                                interactive=True,
                                                visible=field_visible,
                                            )
                                        elif (
                                            type_hint is str
                                            or str in type_args
                                            or type_hint is int
                                            or int in type_args
                                        ):
                                            if base_name in GUI_PASSWORD_FIELDS:
                                                field_input = gr.Textbox(
                                                    label=field.description,
                                                    value=value,
                                                    interactive=True,
                                                    type="password",
                                                    visible=field_visible,
                                                )
                                            else:
                                                field_input = gr.Textbox(
                                                    label=field.description,
                                                    value=value,
                                                    interactive=True,
                                                    visible=field_visible,
                                                )
                                        elif type_hint is bool or bool in type_args:
                                            field_input = gr.Checkbox(
                                                label=field.description,
                                                value=value,
                                                interactive=True,
                                                visible=field_visible,
                                            )
                                        else:
                                            raise Exception(
                                                f"Unsupported type {type_hint} for field {field_name} in gui term extraction engine settings"
                                            )

                                        term_detail_text_input_index_map[
                                            term_metadata.translate_engine_type
                                        ].append(term_detail_index)
                                        term_detail_index += 1
                                        term_detail_text_inputs.append(field_input)
                                        term_detail_text_input_field_names.append(
                                            field_name
                                        )
                                        term_detail_text_input_fields.append(field)
                                        visible_when = gui_extra.get("visible_when")
                                        if visible_when:
                                            dependency_field_name = (
                                                _gui_dependency_field_name(
                                                    field_name, visible_when["field"]
                                                )
                                            )
                                            dependency_input = term_detail_visibility_dependency_inputs.get(
                                                dependency_field_name
                                            )
                                            if dependency_input is not None:
                                                term_detail_visibility_dependency_events.append(
                                                    (
                                                        term_metadata.translate_engine_type,
                                                        field_name,
                                                        field,
                                                        dependency_input,
                                                        field_input,
                                                    )
                                                )
                                        term_detail_visibility_dependency_inputs[
                                            field_name
                                        ] = field_input
                                        __gui_term_service_arg_names.append(field_name)
                                        translation_engine_arg_inputs.append(
                                            field_input
                                        )

                            term_rate_limit_mode = gr.Radio(
                                choices=[
                                    ("RPM (Requests Per Minute)", "RPM"),
                                    ("Concurrent Requests", "Concurrent Threads"),
                                    ("Custom", "Custom"),
                                ],
                                label="Term rate limit mode",
                                value="Custom",
                                interactive=True,
                            )

                            term_rpm_input = gr.Number(
                                label=_("Term RPM (Requests Per Minute)"),
                                value=240,
                                precision=0,
                                minimum=1,
                                maximum=60000,
                                interactive=True,
                                visible=False,
                            )

                            term_concurrent_threads_input = gr.Number(
                                label=_("Term concurrent threads"),
                                value=20,
                                precision=0,
                                minimum=1,
                                maximum=1000,
                                interactive=True,
                                visible=False,
                            )

                            term_custom_qps_input = gr.Number(
                                label=_("Term QPS (Queries Per Second)"),
                                value=(
                                    settings.translation.term_qps
                                    or settings.translation.qps
                                    or 4
                                ),
                                precision=0,
                                minimum=1,
                                maximum=1000,
                                interactive=True,
                                visible=True,
                            )

                            term_custom_pool_max_workers_input = gr.Number(
                                label=_("Term pool max workers"),
                                value=settings.translation.term_pool_max_workers,
                                precision=0,
                                minimum=0,
                                maximum=1000,
                                interactive=True,
                                visible=True,
                            )

                    page_range = gr.Radio(
                        choices=[
                            ("All", "All"),
                            ("First", "First"),
                            ("First 5 pages", "First 5 pages"),
                            ("Range", "Range"),
                        ],
                        label="Pages",
                        value="All",
                    )

                    page_input = gr.Textbox(
                        label=_("Page range (e.g., 1,3,5-10,-5)"),
                        visible=False,
                        interactive=True,
                        placeholder=_("e.g., 1,3,5-10"),
                    )

                    only_include_translated_page = gr.Checkbox(
                        label=_("Only include translated pages in the output PDF."),
                        info=_("Effective only when a page range is specified."),
                        value=settings.pdf.only_include_translated_page,
                        interactive=True,
                    )

                    # PDF Output Options
                    gr.Markdown(_("## PDF Output Options"))
                    with gr.Row():
                        no_mono = gr.Checkbox(
                            label=_("Disable monolingual output"),
                            value=settings.pdf.no_mono,
                            interactive=True,
                        )
                        no_dual = gr.Checkbox(
                            label=_("Disable bilingual output"),
                            value=settings.pdf.no_dual,
                            interactive=True,
                        )

                    with gr.Row():
                        dual_translate_first = gr.Checkbox(
                            label=_("Put translated pages first in dual mode"),
                            value=settings.pdf.dual_translate_first,
                            interactive=True,
                        )
                        use_alternating_pages_dual = gr.Checkbox(
                            label=_("Use alternating pages for dual PDF"),
                            value=settings.pdf.use_alternating_pages_dual,
                            interactive=True,
                        )

                    watermark_output_mode = gr.Radio(
                        choices=[
                            ("Watermarked", "Watermarked"),
                            ("No Watermark", "No Watermark"),
                        ],
                        label="Watermark mode",
                        value="Watermarked"
                        if settings.pdf.watermark_output_mode == "watermarked"
                        else "No Watermark",
                    )

                    # Additional translation options
                    with gr.Accordion(_("Advanced Options"), open=False):
                        prompt = gr.Textbox(
                            label=_("Custom prompt for translation"),
                            value="",
                            visible=False,
                            interactive=True,
                            placeholder=_("Custom prompt for the translator"),
                        )

                        # New Textbox for custom_system_prompt
                        custom_system_prompt_input = gr.Textbox(
                            label=_("Custom System Prompt"),
                            value=settings.translation.custom_system_prompt or "",
                            interactive=True,
                            placeholder=_(
                                "e.g. /no_think You are a professional zh-CN native translator who needs to fluently translate text into zh-CN."
                            ),
                        )

                        min_text_length = gr.Number(
                            label=_("Minimum text length to translate"),
                            value=settings.translation.min_text_length,
                            precision=0,
                            minimum=0,
                            interactive=True,
                        )

                        rpc_doclayout = gr.Textbox(
                            label=_(
                                "RPC service for document layout analysis (optional)"
                            ),
                            value=settings.translation.rpc_doclayout or "",
                            visible=False,
                            interactive=True,
                            placeholder="http://host:port",
                        )

                        save_auto_extracted_glossary = gr.Checkbox(
                            label=_("save automatically extracted glossary"),
                            value=settings.translation.save_auto_extracted_glossary,
                            interactive=True,
                        )

                        primary_font_family = gr.Dropdown(
                            label=_("Primary font family for translated text"),
                            choices=["Auto", "serif", "sans-serif", "script"],
                            value="Auto"
                            if not settings.translation.primary_font_family
                            else settings.translation.primary_font_family,
                            interactive=True,
                        )

                        glossary_file = gr.File(
                            label=_("Glossary File"),
                            file_count="multiple",
                            file_types=[".csv"],
                            type="binary",
                            visible=True,
                        )
                        require_llm_translator_inputs.append(glossary_file)

                        glossary_table = gr.Dataframe(
                            headers=["source", "target"],
                            datatype=["str", "str"],
                            interactive=False,
                            column_count=2,
                            visible=False,
                        )
                        require_llm_translator_inputs.append(glossary_table)

                        # PDF options section
                        gr.Markdown(_("### PDF Options"))

                        skip_clean = gr.Checkbox(
                            label=_("Skip clean (maybe improve compatibility)"),
                            value=settings.pdf.skip_clean,
                            interactive=True,
                        )

                        disable_rich_text_translate = gr.Checkbox(
                            label=_(
                                "Disable rich text translation (maybe improve compatibility)"
                            ),
                            value=settings.pdf.disable_rich_text_translate,
                            interactive=True,
                        )

                        enhance_compatibility = gr.Checkbox(
                            label=_(
                                "Enhance compatibility (auto-enables skip_clean and disable_rich_text)"
                            ),
                            value=settings.pdf.enhance_compatibility,
                            interactive=True,
                        )

                        split_short_lines = gr.Checkbox(
                            label=_(
                                "Force split short lines into different paragraphs"
                            ),
                            value=settings.pdf.split_short_lines,
                            interactive=True,
                        )

                        short_line_split_factor = gr.Slider(
                            label=_("Split threshold factor for short lines"),
                            value=settings.pdf.short_line_split_factor,
                            minimum=0.1,
                            maximum=1.0,
                            step=0.1,
                            interactive=True,
                            visible=settings.pdf.split_short_lines,
                        )

                        translate_table_text = gr.Checkbox(
                            label=_("Translate table text (experimental)"),
                            value=settings.pdf.translate_table_text,
                            interactive=True,
                        )

                        skip_scanned_detection = gr.Checkbox(
                            label=_("Skip scanned detection"),
                            value=settings.pdf.skip_scanned_detection,
                            interactive=True,
                        )

                        ocr_workaround = gr.Checkbox(
                            label=_(
                                "OCR workaround (experimental, will auto enable Skip scanned detection in backend)"
                            ),
                            value=settings.pdf.ocr_workaround,
                            interactive=True,
                        )

                        auto_enable_ocr_workaround = gr.Checkbox(
                            label=_(
                                "Auto enable OCR workaround (enable automatic OCR workaround for heavily scanned documents)"
                            ),
                            value=settings.pdf.auto_enable_ocr_workaround,
                            interactive=True,
                        )

                        max_pages_per_part = gr.Number(
                            label=_(
                                "Maximum pages per part (for auto-split translation, 0 means no limit)"
                            ),
                            value=settings.pdf.max_pages_per_part,
                            precision=0,
                            minimum=0,
                            interactive=True,
                        )

                        formular_font_pattern = gr.Textbox(
                            label=_(
                                "Font pattern to identify formula text (regex, not recommended to change)"
                            ),
                            value=settings.pdf.formular_font_pattern or "",
                            interactive=True,
                            placeholder="e.g., CMMI|CMR",
                        )

                        formular_char_pattern = gr.Textbox(
                            label=_(
                                "Character pattern to identify formula text (regex, not recommended to change)"
                            ),
                            value=settings.pdf.formular_char_pattern or "",
                            interactive=True,
                            placeholder="e.g., [∫∬∭∮∯∰∇∆]",
                        )

                        ignore_cache = gr.Checkbox(
                            label=_("Ignore cache"),
                            value=settings.translation.ignore_cache,
                            interactive=True,
                        )

                        # BabelDOC v0.5.1 new options
                        gr.Markdown(_("#### BabelDOC Advanced Options"))

                        merge_alternating_line_numbers = gr.Checkbox(
                            label=_("Merge alternating line numbers"),
                            info=_(
                                "Handle alternating line numbers and text paragraphs in documents with line numbers"
                            ),
                            value=not settings.pdf.no_merge_alternating_line_numbers,
                            interactive=True,
                        )

                        remove_non_formula_lines = gr.Checkbox(
                            label=_("Remove non-formula lines"),
                            info=_("Remove non-formula lines within paragraph areas"),
                            value=not settings.pdf.no_remove_non_formula_lines,
                            interactive=True,
                        )

                        non_formula_line_iou_threshold = gr.Slider(
                            label=_("Non-formula line IoU threshold"),
                            info=_("IoU threshold for identifying non-formula lines"),
                            value=settings.pdf.non_formula_line_iou_threshold,
                            minimum=0.0,
                            maximum=1.0,
                            step=0.05,
                            interactive=True,
                        )

                        figure_table_protection_threshold = gr.Slider(
                            label=_("Figure/table protection threshold"),
                            info=_(
                                "Protection threshold for figures and tables (lines within figures/tables will not be processed)"
                            ),
                            value=settings.pdf.figure_table_protection_threshold,
                            minimum=0.0,
                            maximum=1.0,
                            step=0.05,
                            interactive=True,
                        )

                        skip_formula_offset_calculation = gr.Checkbox(
                            label=_("Skip formula offset calculation"),
                            info=_("Skip formula offset calculation during processing"),
                            value=settings.pdf.skip_formula_offset_calculation,
                            interactive=True,
                        )

                    # （已移动到 tab_main 中）这里保留设置页底部的“保存设置”和技术说明
                    save_btn = gr.Button(
                        _("Save Settings"),
                        variant="secondary",
                        elem_classes=["save-settings-btn"],
                    )

                    tech_details = gr.Markdown(
                        tech_details_string,
                        elem_classes=["secondary-text"],
                    )

        # Sidebar tab switching: 主界面 / 设置界面
        def _show_main_tab():
            return (
                gr.update(variant="primary"),
                gr.update(variant="secondary"),
                gr.update(visible=True),
                gr.update(visible=False),
            )

        def _show_settings_tab():
            return (
                gr.update(variant="secondary"),
                gr.update(variant="primary"),
                gr.update(visible=False),
                gr.update(visible=True),
            )

        btn_main_tab.click(
            _show_main_tab,
            outputs=[btn_main_tab, btn_settings_tab, tab_main, tab_settings],
        )
        btn_settings_tab.click(
            _show_settings_tab,
            outputs=[btn_main_tab, btn_settings_tab, tab_main, tab_settings],
        )

        # Event handlers
        def on_select_filetype(file_type):
            """Update visibility based on selected file type"""
            return (
                gr.update(visible=file_type == "File"),
                gr.update(visible=file_type == "Link"),
            )

        def on_select_page(choice):
            """Update page input visibility based on selection"""
            return gr.update(visible=choice == "Range")

        def on_select_service(service_name, *detail_values):
            """Update service-specific settings visibility"""
            if not detail_text_inputs:
                return
            detail_group_index = detail_text_input_index_map.get(service_name, [])
            detail_field_values = dict(
                zip(detail_text_input_field_names, detail_values, strict=False)
            )
            llm_support = LLM_support_index_map.get(service_name, False)
            siliconflow_free_acknowledgement_visible = service_name == "SiliconFlowFree"
            siliconflow_update = [
                gr.update(visible=siliconflow_free_acknowledgement_visible)
            ]
            return_list = []
            glossary_updates = [
                gr.update(visible=llm_support)
                for i in range(len(require_llm_translator_inputs))
            ]
            if len(detail_text_inputs) == 1:
                return_list = (
                    siliconflow_update
                    + glossary_updates
                    + [gr.update(visible=(0 in detail_group_index))]
                )
            else:
                return_list = (
                    siliconflow_update
                    + glossary_updates
                    + [
                        _gui_field_update(
                            detail_text_input_fields[i],
                            _gui_field_visible(
                                detail_text_input_field_names[i],
                                detail_text_input_fields[i],
                                i in detail_group_index,
                                detail_field_values,
                            ),
                            current_value=detail_field_values.get(
                                detail_text_input_field_names[i]
                            ),
                        )
                        for i in range(len(detail_text_inputs))
                    ]
                )
            return return_list

        def make_detail_dependency_change_handler(service_type, field_name, field):
            def on_dependency_change(dependency_value, service_name, current_value):
                return _gui_field_update(
                    field,
                    _gui_field_visible(
                        field_name,
                        field,
                        service_name == service_type,
                        {
                            _gui_dependency_field_name(
                                field_name,
                                _field_gui_extra(field)["visible_when"]["field"],
                            ): dependency_value
                        },
                    ),
                    current_value=current_value,
                )

            return on_dependency_change

        def on_enhance_compatibility_change(enhance_value):
            """Update skip_clean and disable_rich_text_translate when enhance_compatibility changes"""
            if enhance_value:
                # When enhanced compatibility is enabled, both options are auto-enabled and disabled for user modification
                return (
                    gr.update(value=True, interactive=False),
                    gr.update(value=True, interactive=False),
                )
            else:
                # When disabled, allow user to modify these settings
                return (
                    gr.update(interactive=True),
                    gr.update(interactive=True),
                )

        def on_split_short_lines_change(split_value):
            """Update short_line_split_factor visibility based on split_short_lines value"""
            return gr.update(visible=split_value)

        def on_glossary_file_change(glossary_file):
            if glossary_file is None:
                return gr.update(visible=False)

            glossary_list = []
            for file in glossary_file:
                file_encoding = chardet.detect(file)["encoding"]
                content = file.decode(file_encoding).replace("\r\n", "\n").strip()
                with io.StringIO(content) as f:
                    csvreader = csv.reader(f, delimiter=",", doublequote=True)
                    next(csvreader)  # Skip header
                    for line in csvreader:
                        if line:
                            glossary_list.append(line)
            logger.warning(f"on_glossary_file_delete glossary_list {glossary_list}")
            if not glossary_list:
                glossary_list = ["", "", ""]
            return gr.update(visible=True, value=glossary_list)

        def on_rate_limit_mode_change(mode, service_name):
            """Update rate-limit-specific-settings visibility based on rate_limit_mode value"""
            if service_name == "SiliconFlowFree":
                return [gr.update(visible=False)] * 4  # Hide all options

            rpm_visible = mode == "RPM"
            threads_visible = mode == "Concurrent Threads"
            custom_visible = mode == "Custom"

            return [
                gr.update(visible=rpm_visible),
                gr.update(visible=threads_visible),
                gr.update(visible=custom_visible),
                gr.update(visible=custom_visible),
            ]

        def on_enable_auto_term_extraction_change(enabled: bool):
            """Update term disabled info visibility based on auto term extraction toggle"""
            return gr.update(visible=not enabled)

        def on_term_rate_limit_mode_change(mode: str):
            """Update term rate-limit controls visibility based on mode"""
            rpm_visible = mode == "RPM"
            threads_visible = mode == "Concurrent Threads"
            custom_visible = mode == "Custom"
            return [
                gr.update(visible=rpm_visible),
                gr.update(visible=threads_visible),
                gr.update(visible=custom_visible),
                gr.update(visible=custom_visible),
            ]

        def on_term_service_change(term_service_name: str, *term_detail_values):
            """Update term engine-specific settings visibility"""
            if not term_detail_text_inputs:
                return
            detail_group_index = term_detail_text_input_index_map.get(
                term_service_name, []
            )
            term_detail_field_values = dict(
                zip(
                    term_detail_text_input_field_names,
                    term_detail_values,
                    strict=False,
                )
            )
            if len(term_detail_text_inputs) == 1:
                return [gr.update(visible=(0 in detail_group_index))]
            return [
                _gui_field_update(
                    term_detail_text_input_fields[i],
                    _gui_field_visible(
                        term_detail_text_input_field_names[i],
                        term_detail_text_input_fields[i],
                        i in detail_group_index,
                        term_detail_field_values,
                    ),
                    current_value=term_detail_field_values.get(
                        term_detail_text_input_field_names[i]
                    ),
                )
                for i in range(len(term_detail_text_inputs))
            ]

        def on_service_change_with_rate_limit(mode, service_name, *detail_values):
            """Expand original on_select_service with rate-limit-UI updated"""
            original_updates = on_select_service(service_name, *detail_values)

            rate_limit_visible = service_name != "SiliconFlowFree"

            detailed_visible = [gr.update(visible=False)] * 4

            if rate_limit_visible:
                detailed_visible = on_rate_limit_mode_change(mode, service_name)

            # Add updates of rate-limit-UI
            rate_limit_updates = [
                gr.update(visible=rate_limit_visible),
            ]

            return original_updates + rate_limit_updates + detailed_visible

        def on_lang_selector_change(lang):
            settings.gui_settings.ui_lang = lang
            update_current_languages(lang)
            config_manager.write_user_default_config_file(settings=settings.clone())
            return

        # UI language change handler

        lang_selector.change(on_lang_selector_change, lang_selector)

        # State for managing translation tasks
        state = gr.State(new_session_state())

        file_input.upload(
            on_file_upload,
            inputs=[file_input, state],
            outputs=[result_file_selector, state, uploaded_files_view],
        )

        # Handle per-file removal (the small X on each row in the File(s) list)
        file_input.change(
            on_file_input_change,
            inputs=[file_input, state, result_file_selector],
            outputs=[
                result_file_selector,
                state,
                uploaded_files_view,
                preview,
                output_file_mono,
                output_file_dual,
                output_file_glossary,
                output_file_mono,  # visibility
                output_file_dual,  # visibility
                output_file_glossary,  # visibility
            ],
        )

        # Handle file clear/delete event
        file_input.clear(
            on_file_clear,
            inputs=[file_input, state],
            outputs=[
                result_file_selector,
                state,
                uploaded_files_view,
                preview,  # Clear preview
                output_file_mono,  # Clear mono download
                output_file_dual,  # Clear dual download
                output_file_glossary,  # Clear glossary download
                output_file_mono,  # Hide mono button visibility
                output_file_dual,  # Hide dual button visibility
                output_file_glossary,  # Hide glossary button visibility
            ],
        )

        # Event bindings
        file_type.select(
            on_select_filetype,
            file_type,
            [file_input, link_input],
        )

        page_range.select(
            on_select_page,
            page_range,
            page_input,
        )

        on_select_service_outputs = (
            [siliconflow_free_acknowledgement]
            + require_llm_translator_inputs
            + detail_text_inputs
        )

        service.select(
            on_service_change_with_rate_limit,
            [rate_limit_mode, service] + detail_text_inputs,
            outputs=(
                on_select_service_outputs
                if len(on_select_service_outputs) > 0
                else None
            )
            + [
                rate_limit_mode,
                rpm_input,
                concurrent_threads_input,
                custom_qps_input,
                custom_pool_max_workers_input,
            ],
        )

        rate_limit_mode.change(
            on_rate_limit_mode_change,
            inputs=[rate_limit_mode, service],
            outputs=[
                rpm_input,
                concurrent_threads_input,
                custom_qps_input,
                custom_pool_max_workers_input,
            ],
        )

        glossary_file.change(
            on_glossary_file_change,
            glossary_file,
            outputs=glossary_table,
        )

        # Add event handler for enhance_compatibility
        enhance_compatibility.change(
            on_enhance_compatibility_change,
            enhance_compatibility,
            [skip_clean, disable_rich_text_translate],
        )

        # Add event handler for split_short_lines
        split_short_lines.change(
            on_split_short_lines_change,
            split_short_lines,
            short_line_split_factor,
        )

        # Auto term extraction toggle handlers
        enable_auto_term_extraction.change(
            on_enable_auto_term_extraction_change,
            enable_auto_term_extraction,
            term_disabled_info,
        )

        # Term rate limit handlers
        term_rate_limit_mode.change(
            on_term_rate_limit_mode_change,
            term_rate_limit_mode,
            [
                term_rpm_input,
                term_concurrent_threads_input,
                term_custom_qps_input,
                term_custom_pool_max_workers_input,
            ],
        )

        # Term service change handler
        term_service.change(
            on_term_service_change,
            [term_service] + term_detail_text_inputs,
            outputs=(
                term_detail_text_inputs if len(term_detail_text_inputs) > 0 else None
            ),
        )

        for (
            service_type,
            field_name,
            field,
            dependency_input,
            dependent_input,
        ) in detail_visibility_dependency_events:
            dependency_input.change(
                make_detail_dependency_change_handler(service_type, field_name, field),
                [
                    dependency_input,
                    service,
                    dependent_input,
                ],
                dependent_input,
            )

        for (
            service_type,
            field_name,
            field,
            dependency_input,
            dependent_input,
        ) in term_detail_visibility_dependency_events:
            dependency_input.change(
                make_detail_dependency_change_handler(service_type, field_name, field),
                [
                    dependency_input,
                    term_service,
                    dependent_input,
                ],
                dependent_input,
            )

        # UI setting controls list (shared by translate_btn and save_btn)
        ui_setting_controls = [
            service,
            lang_from,
            lang_to,
            page_range,
            page_input,
            # PDF Output Options
            no_mono,
            no_dual,
            dual_translate_first,
            use_alternating_pages_dual,
            watermark_output_mode,
            # Rate Limit Options
            rate_limit_mode,
            rpm_input,
            concurrent_threads_input,
            custom_qps_input,
            custom_pool_max_workers_input,
            # Advanced Options
            prompt,
            min_text_length,
            rpc_doclayout,
            custom_system_prompt_input,
            glossary_file,
            save_auto_extracted_glossary,
            # New advanced translation options
            enable_auto_term_extraction,
            primary_font_family,
            skip_clean,
            disable_rich_text_translate,
            enhance_compatibility,
            split_short_lines,
            short_line_split_factor,
            translate_table_text,
            skip_scanned_detection,
            max_pages_per_part,
            formular_font_pattern,
            formular_char_pattern,
            ignore_cache,
            state,
            ocr_workaround,
            auto_enable_ocr_workaround,
            only_include_translated_page,
            # BabelDOC v0.5.1 new options
            merge_alternating_line_numbers,
            remove_non_formula_lines,
            non_formula_line_iou_threshold,
            figure_table_protection_threshold,
            skip_formula_offset_calculation,
            # Term extraction engine options
            term_service,
            term_rate_limit_mode,
            term_rpm_input,
            term_concurrent_threads_input,
            term_custom_qps_input,
            term_custom_pool_max_workers_input,
            *translation_engine_arg_inputs,
            # any UI components that are used by translate/save should be listed above!
            # Extra UI components to be updated on load (not used by translate/save)
            siliconflow_free_acknowledgement,
            glossary_table,
            term_disabled_info,
        ]

        # Language swap button click handler
        swap_lang_btn.click(
            swap_languages,
            inputs=[lang_from, lang_to],
            outputs=[lang_from, lang_to],
        )

        # Translation button click handler
        translate_btn.click(
            translate_files,  # MODIFIED function name
            inputs=[
                file_type,
                file_input,
                link_input,
                *ui_setting_controls,
            ],
            outputs=[
                output_file_mono,  # Mono PDF file
                preview,  # Preview
                output_file_dual,  # Dual PDF file
                output_file_glossary,
                output_file_mono,  # Visibility of mono output
                output_file_dual,  # Visibility of dual output
                output_file_glossary,
                output_title,  # Visibility of output title
                result_file_selector,  # Result selector
                result_file_selector,  # Visibility
                output_file_zip,  # Visibility
                output_file_zip,  # Zip File
                output_file_zip_mono,  # Visibility
                output_file_zip_mono,  # File
                output_file_zip_dual,  # Visibility
                output_file_zip_dual,  # File
                output_file_zip_glossary,  # Visibility
                output_file_zip_glossary,  # File
                uploaded_files_view,  # Uploaded files view
            ],
            show_progress_on=[preview_image],
        )

        # ADDED: Handle result selector change
        def safe_update_preview(selected_label, state):
            """
            Wrapper for update_preview that ensures selected_label is valid before processing.
            Also returns an update for result_file_selector if the value needs to be corrected.
            """
            # Validate selected_label is in choices
            if not state or "display_map" not in state:
                choices = []
            else:
                choices = list(state.get("display_map", {}).keys())

            # If selected_label is not in choices, reset it
            if selected_label and selected_label not in choices:
                # Reset to first available choice or None
                corrected_label = choices[0] if choices else None
                # Return preview update + selector update
                preview_results = (
                    update_preview(corrected_label, state)
                    if corrected_label
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
                    *preview_results,
                    gr.update(
                        choices=choices, value=corrected_label, visible=bool(choices)
                    ),  # Fix selector
                )
            else:
                # Normal case: selected_label is valid
                preview_results = update_preview(selected_label, state)
                return (
                    *preview_results,
                    gr.update(),  # No change to selector
                )

        result_file_selector.change(
            safe_update_preview,
            inputs=[result_file_selector, state],
            outputs=[
                output_file_mono,  # Mono PDF file
                preview,  # Preview
                output_file_dual,  # Dual PDF file
                output_file_glossary,
                output_file_mono,  # Visibility of mono output
                output_file_dual,  # Visibility of dual output
                output_file_glossary,
                result_file_selector,  # Fix selector if value is invalid
            ],
        )

        # Cancel button click handler
        cancel_btn.click(
            stop_translate_file,
            inputs=[state],
        )

        # Save button click handler
        save_btn.click(
            save_config,
            inputs=ui_setting_controls,
        )

        def load_saved_config_to_ui(state):
            """Reload all settings from config and update UI components."""
            try:
                fresh_settings = settings
                update_current_languages(settings.gui_settings.ui_lang)

                updates: list = []

                # Determine selected service by cli flag
                selected_service = None
                for metadata in TRANSLATION_ENGINE_METADATA:
                    if getattr(fresh_settings, metadata.cli_flag_name, False):
                        selected_service = metadata.translate_engine_type
                        break
                if not selected_service:
                    selected_service = available_services[0]
                llm_support = LLM_support_index_map.get(selected_service, False)

                # Follow the EXACT order of ui_setting_controls
                # service
                updates.append(gr.update(value=selected_service))
                # lang_from, lang_to
                loaded_lang_from = rev_lang_map.get(
                    fresh_settings.translation.lang_in, "English"
                )
                loaded_lang_to_code = fresh_settings.translation.lang_out
                loaded_lang_to = next(
                    (k for k, v in lang_map.items() if v == loaded_lang_to_code),
                    "Simplified Chinese",
                )
                updates.append(gr.update(value=loaded_lang_from))
                updates.append(gr.update(value=loaded_lang_to))
                # page_range, page_input
                pages_setting = fresh_settings.pdf.pages
                if pages_setting is None or pages_setting == "":
                    updates.append(gr.update(value="All"))
                    updates.append(gr.update(value="", visible=False))
                else:
                    updates.append(gr.update(value="Range"))
                    updates.append(gr.update(value=str(pages_setting), visible=True))
                # PDF Output Options
                updates.append(gr.update(value=fresh_settings.pdf.no_mono))
                updates.append(gr.update(value=fresh_settings.pdf.no_dual))
                updates.append(gr.update(value=fresh_settings.pdf.dual_translate_first))
                updates.append(
                    gr.update(value=fresh_settings.pdf.use_alternating_pages_dual)
                )
                watermark_value = (
                    "Watermarked"
                    if fresh_settings.pdf.watermark_output_mode == "watermarked"
                    else "No Watermark"
                )
                updates.append(gr.update(value=watermark_value))
                # Rate Limit Options
                rate_limit_visible = selected_service != "SiliconFlowFree"
                updates.append(gr.update(value="Custom", visible=rate_limit_visible))
                updates.append(gr.update(visible=False))  # rpm_input
                updates.append(gr.update(visible=False))  # concurrent_threads_input
                updates.append(
                    gr.update(
                        value=fresh_settings.translation.qps or 4,
                        visible=rate_limit_visible,
                    )
                )
                updates.append(
                    gr.update(
                        value=fresh_settings.translation.pool_max_workers,
                        visible=rate_limit_visible,
                    )
                )
                # Advanced Options
                updates.append(gr.update(value=""))  # prompt
                updates.append(
                    gr.update(value=fresh_settings.translation.min_text_length)
                )
                updates.append(
                    gr.update(value=fresh_settings.translation.rpc_doclayout or "")
                )
                updates.append(
                    gr.update(
                        value=fresh_settings.translation.custom_system_prompt or ""
                    )
                )
                updates.append(
                    gr.update(visible=llm_support)
                )  # glossary_file visibility
                updates.append(
                    gr.update(
                        value=fresh_settings.translation.save_auto_extracted_glossary
                    )
                )
                # enable_auto_term_extraction is the inverse of no_auto_extract_glossary
                updates.append(
                    gr.update(
                        value=not fresh_settings.translation.no_auto_extract_glossary
                    )
                )
                primary_font_display = (
                    "Auto"
                    if not fresh_settings.translation.primary_font_family
                    else fresh_settings.translation.primary_font_family
                )
                updates.append(gr.update(value=primary_font_display))
                updates.append(gr.update(value=fresh_settings.pdf.skip_clean))
                updates.append(
                    gr.update(value=fresh_settings.pdf.disable_rich_text_translate)
                )
                updates.append(
                    gr.update(value=fresh_settings.pdf.enhance_compatibility)
                )
                updates.append(gr.update(value=fresh_settings.pdf.split_short_lines))
                updates.append(
                    gr.update(
                        value=fresh_settings.pdf.short_line_split_factor,
                        visible=fresh_settings.pdf.split_short_lines,
                    )
                )
                updates.append(gr.update(value=fresh_settings.pdf.translate_table_text))
                updates.append(
                    gr.update(value=fresh_settings.pdf.skip_scanned_detection)
                )
                updates.append(gr.update(value=fresh_settings.pdf.max_pages_per_part))
                updates.append(
                    gr.update(value=fresh_settings.pdf.formular_font_pattern or "")
                )
                updates.append(
                    gr.update(value=fresh_settings.pdf.formular_char_pattern or "")
                )
                updates.append(gr.update(value=fresh_settings.translation.ignore_cache))
                updates.append(state)  # state, keep unchanged
                updates.append(gr.update(value=fresh_settings.pdf.ocr_workaround))
                updates.append(
                    gr.update(value=fresh_settings.pdf.auto_enable_ocr_workaround)
                )
                updates.append(
                    gr.update(value=fresh_settings.pdf.only_include_translated_page)
                )
                # BabelDOC
                updates.append(
                    gr.update(
                        value=not fresh_settings.pdf.no_merge_alternating_line_numbers
                    )
                )
                updates.append(
                    gr.update(value=not fresh_settings.pdf.no_remove_non_formula_lines)
                )
                updates.append(
                    gr.update(value=fresh_settings.pdf.non_formula_line_iou_threshold)
                )
                updates.append(
                    gr.update(
                        value=fresh_settings.pdf.figure_table_protection_threshold
                    )
                )
                updates.append(
                    gr.update(value=fresh_settings.pdf.skip_formula_offset_calculation)
                )
                # Term extraction engine basic settings
                term_engine_enabled = (
                    not fresh_settings.translation.no_auto_extract_glossary
                )
                selected_term_service = "Follow main translation engine"
                for term_metadata in TERM_EXTRACTION_ENGINE_METADATA:
                    term_flag_name = f"term_{term_metadata.cli_flag_name}"
                    if getattr(fresh_settings, term_flag_name, False):
                        selected_term_service = term_metadata.translate_engine_type
                        break
                updates.append(gr.update(value=selected_term_service))
                # Term rate limit: use Custom mode by default
                updates.append(gr.update(value="Custom"))
                updates.append(gr.update(visible=False))  # term_rpm_input
                updates.append(
                    gr.update(visible=False)
                )  # term_concurrent_threads_input
                updates.append(
                    gr.update(
                        value=(
                            fresh_settings.translation.term_qps
                            or fresh_settings.translation.qps
                            or 4
                        ),
                        visible=True,
                    )
                )
                updates.append(
                    gr.update(
                        value=fresh_settings.translation.term_pool_max_workers,
                        visible=True,
                    )
                )
                # Translation engine detail fields (ordered)
                disable_sensitive_gui = (
                    fresh_settings.gui_settings.disable_gui_sensitive_input
                )
                for service_name in available_services:
                    metadata = TRANSLATION_ENGINE_METADATA_MAP[service_name]
                    if not metadata.cli_detail_field_name:
                        continue
                    detail_settings = getattr(
                        fresh_settings, metadata.cli_detail_field_name
                    )
                    for (
                        field_name,
                        field,
                    ) in metadata.setting_model_type.model_fields.items():
                        if disable_sensitive_gui:
                            if field_name in GUI_SENSITIVE_FIELDS:
                                continue
                            if field_name in GUI_PASSWORD_FIELDS:
                                continue
                        if field.default_factory:
                            continue
                        if (
                            field_name == "translate_engine_type"
                            or field_name == "support_llm"
                        ):
                            continue
                        value = getattr(detail_settings, field_name)
                        field_values = {
                            name: getattr(detail_settings, name)
                            for name in metadata.setting_model_type.model_fields
                        }
                        visible = _gui_field_visible(
                            field_name,
                            field,
                            metadata.translate_engine_type == selected_service,
                            field_values,
                        )
                        value = _gui_field_value(field, value)
                        updates.append(gr.update(value=value, visible=visible))

                # Term extraction engine detail fields (ordered)
                for term_metadata in TERM_EXTRACTION_ENGINE_METADATA:
                    if not term_metadata.cli_detail_field_name:
                        continue
                    term_detail_field_name = (
                        f"term_{term_metadata.cli_detail_field_name}"
                    )
                    term_detail_settings = getattr(
                        fresh_settings, term_detail_field_name
                    )
                    for (
                        field_name,
                        field,
                    ) in term_metadata.term_setting_model_type.model_fields.items():
                        if field.default_factory:
                            continue
                        if field_name in ("translate_engine_type", "support_llm"):
                            continue
                        base_field_name = field_name
                        if base_field_name.startswith("term_"):
                            base_name = base_field_name[len("term_") :]
                        else:
                            base_name = base_field_name
                        if disable_sensitive_gui:
                            if base_name in GUI_SENSITIVE_FIELDS:
                                continue
                            if base_name in GUI_PASSWORD_FIELDS:
                                continue
                        value = getattr(term_detail_settings, field_name)
                        term_field_values = {
                            name: getattr(term_detail_settings, name)
                            for name in term_metadata.term_setting_model_type.model_fields
                        }
                        visible = _gui_field_visible(
                            field_name,
                            field,
                            term_metadata.translate_engine_type
                            == selected_term_service,
                            term_field_values,
                        )
                        value = _gui_field_value(field, value)
                        updates.append(gr.update(value=value, visible=visible))

                # Extra UI components at the end of ui_setting_controls
                siliconflow_free_ack_visible = selected_service == "SiliconFlowFree"
                updates.append(gr.update(visible=siliconflow_free_ack_visible))
                updates.append(
                    gr.update(visible=llm_support)
                )  # glossary_table visibility
                updates.append(
                    gr.update(
                        visible=fresh_settings.translation.no_auto_extract_glossary
                    )
                )  # term_disabled_info visibility

                return updates
            except Exception as e:
                logger.warning(f"Could not reload config on page load: {e}")
                return [None] * len(ui_setting_controls)

        # Use ui_setting_controls as outputs for page load
        demo.load(load_saved_config_to_ui, inputs=[state], outputs=ui_setting_controls)

        # Initialize result_file_selector on page load to ensure choices and value are consistent
        def init_result_file_selector(state):
            """Initialize result_file_selector with empty choices and None value on page load."""
            # Always start with empty state to avoid Gradio validation errors
            # The actual choices will be populated when files are uploaded
            try:
                if not state or not state.get("display_map"):
                    return gr.update(choices=[], value=None, visible=False)
                choices = list(state.get("display_map", {}).keys())
                if choices:
                    # Ensure the value is in choices
                    current_value = None
                    # Try to preserve current selection if valid
                    if state.get("display_map"):
                        # Use first choice as default
                        current_value = choices[0]
                    return gr.update(choices=choices, value=current_value, visible=True)
                else:
                    return gr.update(choices=[], value=None, visible=False)
            except Exception as e:
                logger.warning(f"Error initializing result_file_selector: {e}")
                # Fallback: always return safe empty state
                return gr.update(choices=[], value=None, visible=False)

        # Initialize result_file_selector FIRST on page load (before any other load handlers)
        # This ensures it's always in a valid state
        demo.load(
            init_result_file_selector,
            inputs=[state],
            outputs=[result_file_selector],
        )

        # JavaScript: 动态调整 PDF canvas 缩放，确保完全适配容器高度
        # 使用兼容性更好的语法，避免 ES6 特性导致解析错误


def parse_user_passwd(file_path: str, welcome_page: str) -> tuple[list, str]:
    """
    This function parses a user password file.

    Inputs:
        - file_path: The path to the file

    Returns:
        - A tuple containing the user list and HTML
    """
    content = ""
    tuple_list = None
    if welcome_page:
        try:
            path = Path(welcome_page)
            content = path.read_text(encoding="utf-8")
        except FileNotFoundError:
            print(f"Error: File '{welcome_page}' not found.")
    if file_path:
        try:
            path = Path(file_path)
            tuple_list = [
                tuple(line.strip().split(","))
                for line in path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
        except FileNotFoundError:
            tuple_list = None
    return tuple_list, content


def setup_gui(
    share: bool = False,
    auth_file: str | None = None,
    welcome_page: str | None = None,
    server_port=7860,
    inbrowser: bool = True,
    server_name: str = "127.0.0.1",
) -> None:
    """Launch locally by default; network access and sharing require opt-in."""
    install_redacting_filters()
    user_list, html = parse_user_passwd(auth_file, welcome_page)
    if auth_file and not user_list:
        raise ValueError("The authentication file contains no valid users")
    if server_name not in {"127.0.0.1", "localhost", "::1"}:
        logger.warning(
            "Security warning: GUI listening on %s may be accessible to other "
            "devices. Use authentication and a trusted network.",
            server_name,
        )
    if share:
        logger.warning("Security warning: public GUI sharing was explicitly enabled")
    # Never retry with a wider address or a public tunnel after launch failure.
    demo.launch(
        theme=gr.themes.Default(
            primary_hue=custom_blue, spacing_size="md", radius_size="lg"
        ),
        css=custom_css,
        server_name=server_name,
        debug=True,
        inbrowser=inbrowser,
        share=share,
        auth=user_list or None,
        auth_message=html,
        server_port=server_port,
        allowed_paths=[str(p) for p in pdf_preview_allowed_paths],
        blocked_paths=[str(DEFAULT_CONFIG_DIR)],
    )


# For auto-reloading while developing
if __name__ == "__main__":
    from rich.logging import RichHandler

    # disable httpx, openai, httpcore, http11 logs
    logging.getLogger("httpx").setLevel("CRITICAL")
    logging.getLogger("httpx").propagate = False
    logging.getLogger("openai").setLevel("CRITICAL")
    logging.getLogger("openai").propagate = False
    logging.getLogger("httpcore").setLevel("CRITICAL")
    logging.getLogger("httpcore").propagate = False
    logging.getLogger("http11").setLevel("CRITICAL")
    logging.getLogger("http11").propagate = False
    logging.basicConfig(level=logging.INFO, handlers=[RichHandler()])
    install_redacting_filters()
    setup_gui(inbrowser=False)
