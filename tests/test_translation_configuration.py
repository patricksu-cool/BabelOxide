"""Characterize the public BabelDOC configuration boundary without API calls."""

import sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import pytest
from pdf2zh_next import high_level
from pdf2zh_next.config.model import SettingsModel


@pytest.fixture
def settings():
    value = SettingsModel(translate_engine_settings={"translate_engine_type": "Google"})
    value.term_extraction_engine_settings = value.translate_engine_settings.model_copy()
    value.translation.term_qps = value.translation.qps
    value.pdf.translate_table_text = False
    return value


@pytest.fixture
def factories(monkeypatch):
    main, term = object(), object()
    config_type = Mock(side_effect=lambda **kwargs: SimpleNamespace(**kwargs))
    split = object()
    config_type.create_max_pages_per_part_split_strategy.return_value = split
    values = SimpleNamespace(
        main=main,
        term=term,
        main_factory=Mock(return_value=main),
        term_factory=Mock(return_value=term),
        config_type=config_type,
        split=split,
    )
    monkeypatch.setattr(high_level, "get_translator", values.main_factory)
    monkeypatch.setattr(high_level, "get_term_translator", values.term_factory)
    monkeypatch.setattr(high_level, "BabelDOCConfig", config_type)
    return values


def test_equal_engines_and_qps_share_translator(settings, factories):
    result = high_level.create_babeldoc_config(settings, Path("input.pdf"))
    assert result.translator is factories.main
    assert result.term_extraction_translator is factories.main
    factories.main_factory.assert_called_once_with(settings)
    factories.term_factory.assert_not_called()


@pytest.mark.parametrize("difference", ["qps", "engine"])
def test_distinct_term_settings_create_term_translator(settings, factories, difference):
    if difference == "qps":
        settings.translation.term_qps = settings.translation.qps + 1
    else:
        settings.term_extraction_engine_settings = None
    result = high_level.create_babeldoc_config(settings, Path("input.pdf"))
    assert result.term_extraction_translator is factories.term
    factories.term_factory.assert_called_once_with(settings)


def test_shared_translator_recommendations_keep_existing_side_effects(
    settings, factories
):
    translator = SimpleNamespace(
        pdf2zh_next_recommended_qps=7, pdf2zh_next_recommended_pool_max_workers=11
    )
    factories.main_factory.return_value = translator
    result = high_level.create_babeldoc_config(settings, Path("input.pdf"))
    assert settings.translation.term_qps == 7
    assert settings.translation.term_pool_max_workers == 11
    assert result.qps == 4
    assert result.term_pool_max_workers == 11
    assert result.term_extraction_translator is translator
    factories.term_factory.assert_not_called()


@pytest.mark.parametrize(
    ("mode", "expected"),
    [
        ("no_watermark", high_level.BabelDOCWatermarkMode.NoWatermark),
        ("both", high_level.BabelDOCWatermarkMode.Both),
        ("watermarked", high_level.BabelDOCWatermarkMode.Watermarked),
        ("unknown", high_level.BabelDOCWatermarkMode.Watermarked),
    ],
)
@pytest.mark.usefixtures("factories")
def test_watermark_modes_and_original_unknown_fallback(settings, mode, expected):
    settings.pdf.watermark_output_mode = mode
    result = high_level.create_babeldoc_config(settings, Path("input.pdf"))
    assert result.watermark_output_mode == expected


def test_config_maps_pdf_translation_and_split_options(
    settings, factories, monkeypatch
):
    settings.basic.debug = True
    settings.report_interval = 0.25
    settings.translation.output = "output"
    settings.translation.lang_out = "zh-TW"
    settings.translation.qps = settings.translation.term_qps = 8
    settings.translation.pool_max_workers = 12
    settings.translation.term_pool_max_workers = 9
    settings.translation.no_auto_extract_glossary = True
    settings.translation.custom_system_prompt = "test prompt"
    settings.translation.primary_font_family = "serif"
    settings.pdf.pages = "1,3-5"
    settings.pdf.max_pages_per_part = 50
    settings.pdf.no_mono = True
    settings.pdf.no_merge_alternating_line_numbers = True
    settings.pdf.no_remove_non_formula_lines = True
    settings.pdf.non_formula_line_iou_threshold = 0.6
    settings.pdf.figure_table_protection_threshold = 0.7
    settings.pdf.skip_formula_offset_calculation = True
    glossary = [object()]
    monkeypatch.setattr(high_level, "_get_glossaries", Mock(return_value=glossary))
    source = Path("different-cwd/input.pdf")
    result = high_level.create_babeldoc_config(settings, source)
    assert result.input_file is source
    assert result.pages == "1,3-5"
    assert result.output_dir == "output"
    assert result.lang_in == "en" and result.lang_out == "zh-TW"
    assert result.debug and result.no_mono and not result.no_dual
    assert result.qps == 8
    assert result.pool_max_workers == 12 and result.term_pool_max_workers == 9
    assert result.report_interval == 0.25
    assert result.custom_system_prompt == "test prompt"
    assert result.primary_font_family == "serif"
    assert result.glossaries is glossary
    assert not result.auto_extract_glossary
    assert not result.merge_alternating_line_numbers
    assert not result.remove_non_formula_lines
    assert result.non_formula_line_iou_threshold == 0.6
    assert result.figure_table_protection_threshold == 0.7
    assert result.skip_formula_offset_calculation
    assert result.font is None and result.doc_layout_model is None
    assert result.split_strategy is factories.split and result.table_model is None
    factories.config_type.create_max_pages_per_part_split_strategy.assert_called_once_with(
        50
    )
    high_level._get_glossaries.assert_called_once_with(settings)


@pytest.mark.usefixtures("factories")
def test_retired_table_model_is_never_loaded(settings, monkeypatch, caplog):
    table_model = object()
    constructor = Mock(return_value=table_model)
    monkeypatch.setitem(
        sys.modules,
        "babeldoc.docvision.table_detection.rapidocr",
        SimpleNamespace(RapidOCRModel=constructor),
    )
    disabled = high_level.create_babeldoc_config(settings, Path("input.pdf"))
    assert disabled.table_model is None
    constructor.assert_not_called()
    settings.pdf.translate_table_text = True
    enabled = high_level.create_babeldoc_config(settings, Path("input.pdf"))
    assert enabled.table_model is None
    constructor.assert_not_called()
    assert "Table-text detection is unavailable" in caplog.text


def test_configuration_validation_fails_before_external_calls(settings, factories):
    with pytest.raises(ValueError, match="is not SettingsModel"):
        high_level.create_babeldoc_config(None, Path("input.pdf"))
    factories.main_factory.assert_not_called()
    factories.main_factory.return_value = None
    with pytest.raises(ValueError, match="No translator found"):
        high_level.create_babeldoc_config(settings, Path("input.pdf"))
    factories.term_factory.assert_not_called()
    factories.config_type.assert_not_called()


def test_glossary_loading_preserves_order_and_language(settings, monkeypatch):
    loader = Mock(side_effect=["first", "second"])
    monkeypatch.setattr(high_level, "Glossary", SimpleNamespace(from_csv=loader))
    assert high_level._get_glossaries(settings) is None
    loader.assert_not_called()
    settings.translation.glossaries = "first.csv,folder/second.csv"
    settings.translation.lang_out = "zh-TW"
    assert high_level._get_glossaries(settings) == ["first", "second"]
    assert loader.call_args_list == [
        ((Path("first.csv"),), {"target_lang_out": "zh-TW"}),
        ((Path("folder/second.csv"),), {"target_lang_out": "zh-TW"}),
    ]
