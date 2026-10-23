"""Convert GUI controls into validated translation and saved settings."""

import typing
from collections.abc import Callable
from collections.abc import Sequence
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from string import Template

import gradio as gr

from pdf2zh_next.config import ConfigManager
from pdf2zh_next.config.cli_env_model import CLIEnvSettingsModel
from pdf2zh_next.config.model import SettingsModel
from pdf2zh_next.config.translate_engine_model import GUI_PASSWORD_FIELDS
from pdf2zh_next.config.translate_engine_model import GUI_SENSITIVE_FIELDS
from pdf2zh_next.config.translate_engine_model import TERM_EXTRACTION_ENGINE_METADATA
from pdf2zh_next.config.translate_engine_model import (
    TERM_EXTRACTION_ENGINE_METADATA_MAP,
)
from pdf2zh_next.config.translate_engine_model import TRANSLATION_ENGINE_METADATA
from pdf2zh_next.config.translate_engine_model import TRANSLATION_ENGINE_METADATA_MAP
from pdf2zh_next.ui.fields import _type_hint_allows_none
from pdf2zh_next.ui.rate_options import _calculate_rate_limit_params


@dataclass(frozen=True)
class SettingsContext:
    config_manager: ConfigManager
    lang_map: dict[str, str]
    page_map: dict[str, list[int] | None]
    disable_sensitive_input: bool
    on_saved: Callable[[CLIEnvSettingsModel], None]


class SaveMode(Enum):
    """Enum for configuration save behavior."""

    follow_settings = "follow_settings"  # Follow disable_config_auto_save setting
    never = "never"  # Never save
    always = "always"  # Always save regardless of disable_config_auto_save


def build_translate_settings(
    base_settings: CLIEnvSettingsModel,
    file_path: Path,
    output_dir: Path,
    save_mode: SaveMode,
    ui_inputs: dict,
    *,
    context: SettingsContext,
) -> SettingsModel:
    """
    This function builds translation settings from UI inputs.

    Inputs:
        - base_settings: The base settings model to build upon
        - file_path: The path to the input file
        - output_dir: The output directory
        - save_mode: SaveMode enum indicating when to save config
        - ui_inputs: A dictionary of UI inputs

    Returns:
        - A configured SettingsModel instance
    """
    config_manager = context.config_manager
    lang_map = context.lang_map
    page_map = context.page_map
    disable_gui_sensitive_input = context.disable_sensitive_input

    # Clone base settings to avoid modifying the original
    translate_settings = base_settings.clone()
    original_output = translate_settings.translation.output
    original_pages = translate_settings.pdf.pages
    original_gui_settings = config_manager.config_cli_settings.gui_settings

    # Extract UI values
    service = ui_inputs.get("service")
    lang_from = ui_inputs.get("lang_from")
    lang_to = ui_inputs.get("lang_to")
    page_range = ui_inputs.get("page_range")
    page_input = ui_inputs.get("page_input")
    prompt = ui_inputs.get("prompt")
    ignore_cache = ui_inputs.get("ignore_cache")

    # PDF Output Options
    no_mono = ui_inputs.get("no_mono")
    no_dual = ui_inputs.get("no_dual")
    dual_translate_first = ui_inputs.get("dual_translate_first")
    use_alternating_pages_dual = ui_inputs.get("use_alternating_pages_dual")
    watermark_output_mode = ui_inputs.get("watermark_output_mode")

    # Rate Limit Options
    rate_limit_mode = ui_inputs.get("rate_limit_mode")

    # Advanced Translation Options
    min_text_length = ui_inputs.get("min_text_length")
    rpc_doclayout = ui_inputs.get("rpc_doclayout")
    enable_auto_term_extraction = ui_inputs.get("enable_auto_term_extraction")
    primary_font_family = ui_inputs.get("primary_font_family")

    # Advanced PDF Options
    skip_clean = ui_inputs.get("skip_clean")
    disable_rich_text_translate = ui_inputs.get("disable_rich_text_translate")
    enhance_compatibility = ui_inputs.get("enhance_compatibility")
    split_short_lines = ui_inputs.get("split_short_lines")
    short_line_split_factor = ui_inputs.get("short_line_split_factor")
    translate_table_text = ui_inputs.get("translate_table_text")
    skip_scanned_detection = ui_inputs.get("skip_scanned_detection")
    ocr_workaround = ui_inputs.get("ocr_workaround")
    max_pages_per_part = ui_inputs.get("max_pages_per_part")
    formular_font_pattern = ui_inputs.get("formular_font_pattern")
    formular_char_pattern = ui_inputs.get("formular_char_pattern")
    auto_enable_ocr_workaround = ui_inputs.get("auto_enable_ocr_workaround")
    only_include_translated_page = ui_inputs.get("only_include_translated_page")

    # BabelDOC v0.5.1 new options
    merge_alternating_line_numbers = ui_inputs.get("merge_alternating_line_numbers")
    remove_non_formula_lines = ui_inputs.get("remove_non_formula_lines")
    non_formula_line_iou_threshold = ui_inputs.get("non_formula_line_iou_threshold")
    figure_table_protection_threshold = ui_inputs.get(
        "figure_table_protection_threshold"
    )
    skip_formula_offset_calculation = ui_inputs.get("skip_formula_offset_calculation")

    # Term extraction options
    term_service = ui_inputs.get("term_service")
    term_rate_limit_mode = ui_inputs.get("term_rate_limit_mode")
    term_rpm_input = ui_inputs.get("term_rpm_input")
    term_concurrent_threads = ui_inputs.get("term_concurrent_threads")
    term_custom_qps = ui_inputs.get("term_custom_qps")
    term_custom_pool_workers = ui_inputs.get("term_custom_pool_workers")

    # New input for custom_system_prompt
    custom_system_prompt_input = ui_inputs.get("custom_system_prompt_input")
    glossaries = ui_inputs.get("glossaries")
    save_auto_extracted_glossary = ui_inputs.get("save_auto_extracted_glossary")

    # Map UI language selections to language codes
    source_lang = lang_map.get(lang_from, "auto")
    target_lang = lang_map.get(lang_to, "zh")

    # Set up page selection
    if page_range == "Range" and page_input:
        pages = page_input  # The backend parser handles the format
    else:
        # Use predefined ranges from page_map
        selected_pages = page_map[page_range]
        if selected_pages is None:
            pages = None  # All pages
        else:
            # Convert page indices to comma-separated string
            pages = ",".join(
                str(p + 1) for p in selected_pages
            )  # +1 because UI is 1-indexed

    # Update settings with UI values
    translate_settings.basic.input_files = {str(file_path)}
    translate_settings.report_interval = 0.2
    translate_settings.translation.lang_in = source_lang
    translate_settings.translation.lang_out = target_lang
    translate_settings.translation.output = str(output_dir)
    translate_settings.translation.ignore_cache = ignore_cache

    # Update Translation Settings
    if min_text_length is not None:
        translate_settings.translation.min_text_length = int(min_text_length)
    if rpc_doclayout:
        translate_settings.translation.rpc_doclayout = rpc_doclayout

    # UI uses positive switch, config uses negative flag, so we invert here
    if enable_auto_term_extraction is not None:
        translate_settings.translation.no_auto_extract_glossary = (
            not enable_auto_term_extraction
        )
    if primary_font_family:
        if primary_font_family == "Auto":
            translate_settings.translation.primary_font_family = None
        else:
            translate_settings.translation.primary_font_family = primary_font_family

    # Calculate and update rate limit settings
    if service != "SiliconFlowFree":
        qps, pool_workers = _calculate_rate_limit_params(
            rate_limit_mode, ui_inputs, translate_settings.translation.qps or 4
        )

        # Update translation settings
        translate_settings.translation.qps = int(qps)
        translate_settings.translation.pool_max_workers = (
            int(pool_workers) if pool_workers is not None else None
        )

    # Calculate and update term extraction rate limit settings
    if term_rate_limit_mode:
        term_rate_inputs = {
            "rpm_input": term_rpm_input,
            "concurrent_threads": term_concurrent_threads,
            "custom_qps": term_custom_qps,
            "custom_pool_workers": term_custom_pool_workers,
        }
        term_qps, term_pool_workers = _calculate_rate_limit_params(
            term_rate_limit_mode,
            term_rate_inputs,
            translate_settings.translation.term_qps
            or translate_settings.translation.qps
            or 4,
        )
        translate_settings.translation.term_qps = int(term_qps)
        translate_settings.translation.term_pool_max_workers = (
            int(term_pool_workers) if term_pool_workers is not None else None
        )

    # Reset all term extraction engine flags
    for term_metadata in TERM_EXTRACTION_ENGINE_METADATA:
        term_flag_name = f"term_{term_metadata.cli_flag_name}"
        if hasattr(translate_settings, term_flag_name):
            setattr(translate_settings, term_flag_name, False)

    # Configure term extraction engine settings from UI when not following main engine
    if (
        term_service
        and term_service != "Follow main translation engine"
        and not translate_settings.translation.no_auto_extract_glossary
        and term_service in TERM_EXTRACTION_ENGINE_METADATA_MAP
    ):
        term_metadata = TERM_EXTRACTION_ENGINE_METADATA_MAP[term_service]

        # Enable selected term extraction engine flag
        term_flag_name = f"term_{term_metadata.cli_flag_name}"
        if hasattr(translate_settings, term_flag_name):
            setattr(translate_settings, term_flag_name, True)

        # Update term extraction engine detail settings
        if term_metadata.cli_detail_field_name:
            term_detail_field_name = f"term_{term_metadata.cli_detail_field_name}"
            term_detail_settings = getattr(translate_settings, term_detail_field_name)
            term_model_type = term_metadata.term_setting_model_type

            for field_name, field in term_model_type.model_fields.items():
                if field_name in ("translate_engine_type", "support_llm"):
                    continue

                if field_name not in ui_inputs:
                    continue
                value = ui_inputs[field_name]

                type_hint = field.annotation
                type_args = typing.get_args(type_hint)

                if value is None or (value == "" and _type_hint_allows_none(type_hint)):
                    if _type_hint_allows_none(type_hint):
                        setattr(term_detail_settings, field_name, None)
                    continue

                if type_hint is str or str in type_args:
                    pass
                elif type_hint is int or int in type_args:
                    value = int(value)
                elif type_hint is bool or bool in type_args:
                    value = bool(value)
                else:
                    raise Exception(
                        f"Unsupported type {type_hint} for field {field_name} in gui term extraction engine settings"
                    )

                setattr(term_detail_settings, field_name, value)

    # Update PDF Settings
    translate_settings.pdf.pages = pages
    translate_settings.pdf.no_mono = no_mono
    translate_settings.pdf.no_dual = no_dual
    translate_settings.pdf.dual_translate_first = dual_translate_first
    translate_settings.pdf.use_alternating_pages_dual = use_alternating_pages_dual

    # Map watermark mode from UI to enum
    translate_settings.pdf.watermark_output_mode = (
        watermark_output_mode.lower().replace(" ", "_")
    )

    # Update Advanced PDF Settings
    translate_settings.pdf.skip_clean = skip_clean
    translate_settings.pdf.disable_rich_text_translate = disable_rich_text_translate
    translate_settings.pdf.enhance_compatibility = enhance_compatibility
    translate_settings.pdf.split_short_lines = split_short_lines
    translate_settings.pdf.ocr_workaround = ocr_workaround
    if short_line_split_factor is not None:
        translate_settings.pdf.short_line_split_factor = float(short_line_split_factor)

    translate_settings.pdf.translate_table_text = translate_table_text
    translate_settings.pdf.skip_scanned_detection = skip_scanned_detection
    translate_settings.pdf.auto_enable_ocr_workaround = auto_enable_ocr_workaround
    translate_settings.pdf.only_include_translated_page = only_include_translated_page

    if max_pages_per_part is not None and max_pages_per_part > 0:
        translate_settings.pdf.max_pages_per_part = int(max_pages_per_part)

    if formular_font_pattern:
        translate_settings.pdf.formular_font_pattern = formular_font_pattern

    if formular_char_pattern:
        translate_settings.pdf.formular_char_pattern = formular_char_pattern

    # Apply BabelDOC v0.5.1 new options
    translate_settings.pdf.no_merge_alternating_line_numbers = (
        not merge_alternating_line_numbers
    )
    translate_settings.pdf.no_remove_non_formula_lines = not remove_non_formula_lines
    if non_formula_line_iou_threshold is not None:
        translate_settings.pdf.non_formula_line_iou_threshold = float(
            non_formula_line_iou_threshold
        )
    if figure_table_protection_threshold is not None:
        translate_settings.pdf.figure_table_protection_threshold = float(
            figure_table_protection_threshold
        )
    translate_settings.pdf.skip_formula_offset_calculation = (
        skip_formula_offset_calculation
    )

    assert service in TRANSLATION_ENGINE_METADATA_MAP, "UNKNOW TRANSLATION ENGINE!"

    for metadata in TRANSLATION_ENGINE_METADATA:
        cli_flag = metadata.cli_flag_name
        setattr(translate_settings, cli_flag, False)

    metadata = TRANSLATION_ENGINE_METADATA_MAP[service]
    cli_flag = metadata.cli_flag_name
    setattr(translate_settings, cli_flag, True)
    if metadata.cli_detail_field_name:
        detail_setting = getattr(translate_settings, metadata.cli_detail_field_name)
        if metadata.setting_model_type:
            for field_name in metadata.setting_model_type.model_fields:
                if field_name == "translate_engine_type" or field_name == "support_llm":
                    continue
                if disable_gui_sensitive_input:
                    if field_name in GUI_PASSWORD_FIELDS:
                        continue
                    if field_name in GUI_SENSITIVE_FIELDS:
                        continue
                value = ui_inputs.get(field_name)
                type_hint = detail_setting.model_fields[field_name].annotation
                if value == "" and _type_hint_allows_none(type_hint):
                    value = None
                original_type = typing.get_origin(type_hint)
                type_args = typing.get_args(type_hint)
                if type_hint is str or str in type_args:
                    pass
                elif type_hint is int or int in type_args:
                    value = int(value)
                elif type_hint is bool or bool in type_args:
                    value = bool(value)
                else:
                    raise Exception(
                        f"Unsupported type {type_hint} for field {field_name} in gui translation engine settings"
                    )
                setattr(detail_setting, field_name, value)

    # Add custom prompt if provided
    if prompt:
        # This might need adjustment based on how prompt is handled in the new system
        translate_settings.custom_prompt = Template(prompt)

    # Add custom system prompt if provided
    if custom_system_prompt_input:
        translate_settings.translation.custom_system_prompt = custom_system_prompt_input
    else:
        translate_settings.translation.custom_system_prompt = None

    if glossaries:
        translate_settings.translation.glossaries = glossaries
    else:
        translate_settings.translation.glossaries = None

    translate_settings.translation.save_auto_extracted_glossary = (
        save_auto_extracted_glossary
    )

    # Validate settings before proceeding
    try:
        translate_settings.validate_settings()
        temp_settings = translate_settings.to_settings_model()
        translate_settings.translation.output = original_output
        translate_settings.pdf.pages = original_pages
        translate_settings.gui_settings = original_gui_settings
        translate_settings.basic.gui = False
        translate_settings.basic.debug = False
        translate_settings.translation.glossaries = None

        # Determine if config should be saved based on save_mode
        should_save = False
        if save_mode == SaveMode.always:
            should_save = True
        elif save_mode == SaveMode.follow_settings:
            should_save = not temp_settings.gui_settings.disable_config_auto_save
        # SaveMode.never: should_save remains False

        if should_save:
            config_manager.write_user_default_config_file(settings=translate_settings)
            context.on_saved(translate_settings)
        temp_settings.validate_settings()
        return temp_settings
    except ValueError as e:
        raise gr.Error(f"Invalid settings: {e}") from e


def build_ui_inputs(
    *args,
    service_arg_names: Sequence[str],
    term_service_arg_names: Sequence[str],
    build_glossary_list: Callable,
):
    """
    Build ui_inputs dictionary from *args.

    Args:
        *args: UI setting controls in the following order:
            service, lang_from, lang_to, page_range, page_input,
            no_mono, no_dual, dual_translate_first, use_alternating_pages_dual, watermark_output_mode,
            rate_limit_mode, rpm_input, concurrent_threads_input, custom_qps_input, custom_pool_max_workers_input,
            prompt, min_text_length, rpc_doclayout, custom_system_prompt_input, glossary_file,
            save_auto_extracted_glossary, enable_auto_term_extraction, primary_font_family, skip_clean,
            disable_rich_text_translate, enhance_compatibility, split_short_lines, short_line_split_factor,
            translate_table_text, skip_scanned_detection, max_pages_per_part, formular_font_pattern,
            formular_char_pattern, ignore_cache, state, ocr_workaround, auto_enable_ocr_workaround,
            only_include_translated_page, merge_alternating_line_numbers, remove_non_formula_lines,
            non_formula_line_iou_threshold, figure_table_protection_threshold, skip_formula_offset_calculation,
            term_service, term_rate_limit_mode, term_rpm_input, term_concurrent_threads_input,
            term_custom_qps_input, term_custom_pool_max_workers_input, *translation_engine_arg_inputs

    Returns:
        dict: ui_inputs dictionary with all UI settings
    """
    # Fixed parameter names in order (excluding translation_engine_arg_inputs)
    fixed_param_names = [
        "service",
        "lang_from",
        "lang_to",
        "page_range",
        "page_input",
        "no_mono",
        "no_dual",
        "dual_translate_first",
        "use_alternating_pages_dual",
        "watermark_output_mode",
        "rate_limit_mode",
        "rpm_input",
        "concurrent_threads",  # mapped from concurrent_threads_input
        "custom_qps",  # mapped from custom_qps_input
        "custom_pool_workers",  # mapped from custom_pool_max_workers_input
        "prompt",
        "min_text_length",
        "rpc_doclayout",
        "custom_system_prompt_input",
        "glossary_file",  # will be converted to glossaries
        "save_auto_extracted_glossary",
        "enable_auto_term_extraction",
        "primary_font_family",
        "skip_clean",
        "disable_rich_text_translate",
        "enhance_compatibility",
        "split_short_lines",
        "short_line_split_factor",
        "translate_table_text",
        "skip_scanned_detection",
        "max_pages_per_part",
        "formular_font_pattern",
        "formular_char_pattern",
        "ignore_cache",
        "state",
        "ocr_workaround",
        "auto_enable_ocr_workaround",
        "only_include_translated_page",
        "merge_alternating_line_numbers",
        "remove_non_formula_lines",
        "non_formula_line_iou_threshold",
        "figure_table_protection_threshold",
        "skip_formula_offset_calculation",
        "term_service",
        "term_rate_limit_mode",
        "term_rpm_input",
        "term_concurrent_threads",
        "term_custom_qps",
        "term_custom_pool_workers",
    ]

    # Split args into fixed params and translation_engine_arg_inputs
    num_fixed = len(fixed_param_names)
    fixed_args = args[:num_fixed]
    translation_engine_arg_inputs = args[num_fixed:]

    # Build ui_inputs dictionary
    ui_inputs = {}
    for param_name, arg_value in zip(fixed_param_names, fixed_args, strict=False):
        ui_inputs[param_name] = arg_value

    # Convert glossary_file to glossaries
    service = ui_inputs["service"]
    glossary_file = ui_inputs["glossary_file"]
    ui_inputs["glossaries"] = build_glossary_list(glossary_file, service)

    # Add translation engine args (main translator + term translator detail settings)
    main_detail_count = len(service_arg_names)
    term_detail_count = len(term_service_arg_names)

    main_detail_inputs = translation_engine_arg_inputs[:main_detail_count]
    term_detail_inputs = translation_engine_arg_inputs[
        main_detail_count : main_detail_count + term_detail_count
    ]

    for arg_name, arg_input in zip(service_arg_names, main_detail_inputs, strict=False):
        ui_inputs[arg_name] = arg_input

    for arg_name, arg_input in zip(
        term_service_arg_names, term_detail_inputs, strict=False
    ):
        ui_inputs[arg_name] = arg_input

    return ui_inputs
