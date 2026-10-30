//! Typed snapshots of the existing Python base settings.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BasicSettings {
    pub input_files: BTreeSet<String>,
    pub debug: bool,
    pub gui: bool,
    pub warmup: bool,
    pub generate_offline_assets: Option<String>,
    pub restore_offline_assets: Option<String>,
    pub version: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for BasicSettings {
    fn default() -> Self {
        Self {
            input_files: BTreeSet::new(),
            debug: false,
            gui: false,
            warmup: false,
            generate_offline_assets: None,
            restore_offline_assets: None,
            version: false,
            extra: Map::new(),
        }
    }
}
impl std::fmt::Debug for BasicSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BasicSettings { [REDACTED] }")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiSettings {
    pub share: bool,
    pub auth_file: Option<String>,
    pub welcome_page: Option<String>,
    pub enabled_services: Option<String>,
    pub disable_gui_sensitive_input: bool,
    pub disable_config_auto_save: bool,
    pub server_port: i64,
    pub server_name: String,
    pub ui_lang: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            share: false,
            auth_file: None,
            welcome_page: None,
            enabled_services: None,
            disable_gui_sensitive_input: false,
            disable_config_auto_save: false,
            server_port: 7860,
            server_name: "127.0.0.1".into(),
            ui_lang: Some("en".into()),
            extra: Map::new(),
        }
    }
}
impl std::fmt::Debug for GuiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GuiSettings { [REDACTED] }")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationSettings {
    pub min_text_length: i64,
    pub rpc_doclayout: Option<String>,
    pub lang_in: String,
    pub lang_out: String,
    pub output: Option<String>,
    pub qps: i64,
    pub ignore_cache: bool,
    pub custom_system_prompt: Option<String>,
    pub glossaries: Option<String>,
    pub save_auto_extracted_glossary: bool,
    pub pool_max_workers: Option<i64>,
    pub term_qps: Option<i64>,
    pub term_pool_max_workers: Option<i64>,
    pub no_auto_extract_glossary: bool,
    pub primary_font_family: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            min_text_length: 5,
            rpc_doclayout: None,
            lang_in: "en".into(),
            lang_out: "zh".into(),
            output: None,
            qps: 4,
            ignore_cache: false,
            custom_system_prompt: None,
            glossaries: None,
            save_auto_extracted_glossary: false,
            pool_max_workers: None,
            term_qps: None,
            term_pool_max_workers: None,
            no_auto_extract_glossary: false,
            primary_font_family: None,
            extra: Map::new(),
        }
    }
}
impl std::fmt::Debug for TranslationSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TranslationSettings { [REDACTED] }")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PdfSettings {
    pub pages: Option<String>,
    pub no_dual: bool,
    pub no_mono: bool,
    pub formular_font_pattern: Option<String>,
    pub formular_char_pattern: Option<String>,
    pub split_short_lines: bool,
    pub short_line_split_factor: f64,
    pub skip_clean: bool,
    pub dual_translate_first: bool,
    pub disable_rich_text_translate: bool,
    pub enhance_compatibility: bool,
    pub use_alternating_pages_dual: bool,
    pub watermark_output_mode: String,
    pub max_pages_per_part: Option<i64>,
    pub translate_table_text: bool,
    pub skip_scanned_detection: bool,
    pub ocr_workaround: bool,
    pub auto_enable_ocr_workaround: bool,
    pub only_include_translated_page: bool,
    pub no_merge_alternating_line_numbers: bool,
    pub no_remove_non_formula_lines: bool,
    pub non_formula_line_iou_threshold: f64,
    pub figure_table_protection_threshold: f64,
    pub skip_formula_offset_calculation: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for PdfSettings {
    fn default() -> Self {
        Self {
            pages: None,
            no_dual: false,
            no_mono: false,
            formular_font_pattern: None,
            formular_char_pattern: None,
            split_short_lines: false,
            short_line_split_factor: 0.8,
            skip_clean: false,
            dual_translate_first: false,
            disable_rich_text_translate: false,
            enhance_compatibility: false,
            use_alternating_pages_dual: false,
            watermark_output_mode: "watermarked".into(),
            max_pages_per_part: None,
            translate_table_text: true,
            skip_scanned_detection: false,
            ocr_workaround: false,
            auto_enable_ocr_workaround: false,
            only_include_translated_page: false,
            no_merge_alternating_line_numbers: false,
            no_remove_non_formula_lines: false,
            non_formula_line_iou_threshold: 0.9,
            figure_table_protection_threshold: 0.9,
            skip_formula_offset_calculation: false,
            extra: Map::new(),
        }
    }
}
impl std::fmt::Debug for PdfSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PdfSettings { [REDACTED] }")
    }
}
