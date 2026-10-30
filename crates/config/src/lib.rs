//! Application-owned settings. No process environment, home directory, network,
//! or credential store is read implicitly while constructing Settings.
mod error;
mod overrides;
mod providers;
mod schema;
mod sections;
mod storage;

pub use error::{ConfigError, ConfigErrorKind, Result};
pub use overrides::{parse_cli_overrides, parse_env_vars};
use pdf2zh_core::{PdfJob, TranslatorConfig};
pub use schema::{EngineSpec, FieldSpec, FieldType, engine_specs, field_specs};
pub use sections::{BasicSettings, GuiSettings, PdfSettings, TranslationSettings};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt, path::PathBuf};
pub use storage::{ConfigStore, decode_toml_content, encode_toml_content};

/// Complete CLIEnv-layout document, including fields unknown to this version.
/// Explicit as_value/serialization is intended for persistence, never logging.
#[derive(Clone)]
pub struct Settings {
    document: Value,
    pub(crate) original_toml: Option<toml::Value>,
    pub(crate) sdk_url_fallbacks: BTreeMap<String, (String, String)>,
}

impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Settings { [REDACTED] }")
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            document: schema::defaults(),
            original_toml: None,
            sdk_url_fallbacks: BTreeMap::new(),
        }
    }
}

impl Serialize for Settings {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.document.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Self::from_value(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Settings {
    pub fn from_value(value: Value) -> Result<Self> {
        Self::from_layers(&[value])
    }

    /// Highest-priority layer first. Lists extend in low-to-high order, matching
    /// ConfigManager; engine selection is independently chosen before merging.
    pub fn from_layers(layers: &[Value]) -> Result<Self> {
        if layers.iter().any(|v| !v.is_object()) {
            return Err(ConfigError::new(ConfigErrorKind::InvalidDocument));
        }
        let main = layers.iter().find_map(|v| overrides::select_flag(v, false));
        let term = layers.iter().find_map(|v| overrides::select_flag(v, true));
        let mut result = Self::default();
        for layer in layers.iter().rev() {
            overrides::deep_merge(&mut result.document, layer);
        }
        for (chosen, is_term) in [(main, false), (term, true)] {
            if let Some(chosen) = chosen {
                for engine in engine_specs().iter().filter(|e| !is_term || e.supports_llm) {
                    let flag = if is_term {
                        format!("term_{}", engine.flag)
                    } else {
                        engine.flag.clone()
                    };
                    result.document[&flag] = json!(flag == chosen);
                }
            }
        }
        overrides::normalize_known_types(&mut result.document)?;
        result.basic()?;
        result.translation()?;
        result.pdf()?;
        result.gui()?;
        for engine in engine_specs() {
            if let Some(detail) = &engine.detail_key {
                if !result.document.get(detail).is_some_and(Value::is_object) {
                    return Err(ConfigError::field(detail));
                }
                if engine.supports_llm
                    && !result
                        .document
                        .get(format!("term_{detail}"))
                        .is_some_and(Value::is_object)
                {
                    return Err(ConfigError::field(&format!("term_{detail}")));
                }
                for key in [
                    Some(detail.clone()),
                    engine.supports_llm.then(|| format!("term_{detail}")),
                ]
                .into_iter()
                .flatten()
                {
                    let object = &result.document[&key];
                    if object.get("translate_engine_type").and_then(Value::as_str)
                        != Some(engine.name.as_str())
                    {
                        return Err(ConfigError::field(&format!("{key}.translate_engine_type")));
                    }
                    if object
                        .get("support_llm")
                        .is_some_and(|value| !matches!(value.as_str(), Some("yes" | "no")))
                    {
                        return Err(ConfigError::field(&format!("{key}.support_llm")));
                    }
                }
            }
        }
        Ok(result)
    }

    pub fn as_value(&self) -> &Value {
        &self.document
    }
    pub fn into_value(self) -> Value {
        self.document
    }
    pub fn get(&self, dotted_path: &str) -> Option<&Value> {
        schema::lookup(&self.document, dotted_path)
    }

    /// A true engine flag selects that engine and clears peer flags in memory.
    /// This never writes the configuration file.
    pub fn set(&mut self, dotted_path: &str, value: Value) -> Result<()> {
        let value = if let Some(spec) = field_specs().iter().find(|s| s.path == dotted_path) {
            overrides::coerce(spec, &value, false)?
        } else {
            value
        };
        let mut candidate = self.document.clone();
        if !schema::put(&mut candidate, dotted_path, value.clone()) {
            return Err(ConfigError::new(ConfigErrorKind::InvalidDocument));
        }
        if value == Value::Bool(true) {
            for term in [false, true] {
                let is_flag = engine_specs().iter().any(|e| {
                    (!term || e.supports_llm)
                        && if term {
                            format!("term_{}", e.flag) == dotted_path
                        } else {
                            e.flag == dotted_path
                        }
                });
                if is_flag {
                    for engine in engine_specs().iter().filter(|e| !term || e.supports_llm) {
                        let flag = if term {
                            format!("term_{}", engine.flag)
                        } else {
                            engine.flag.clone()
                        };
                        candidate[&flag] = json!(flag == dotted_path);
                    }
                }
            }
        }
        let mut validated = Self::from_value(candidate)?;
        validated.original_toml = self.original_toml.clone();
        validated.sdk_url_fallbacks = self.sdk_url_fallbacks.clone();
        *self = validated;
        Ok(())
    }

    fn typed<T: serde::de::DeserializeOwned>(&self, section: &str) -> Result<T> {
        serde_json::from_value(self.document[section].clone())
            .map_err(|_| ConfigError::field(section))
    }
    pub fn basic(&self) -> Result<BasicSettings> {
        self.typed("basic")
    }
    pub fn translation(&self) -> Result<TranslationSettings> {
        self.typed("translation")
    }
    pub fn pdf(&self) -> Result<PdfSettings> {
        self.typed("pdf")
    }
    pub fn gui(&self) -> Result<GuiSettings> {
        self.typed("gui_settings")
    }

    pub fn main_engine(&self) -> &'static EngineSpec {
        self.selected_engine(false).unwrap_or(&engine_specs()[0])
    }
    pub fn term_engine(&self) -> Option<&'static EngineSpec> {
        self.selected_engine(true).or_else(|| {
            self.main_engine()
                .supports_llm
                .then_some(self.main_engine())
        })
    }
    /// Compare engine settings before deriving QPS or constructing any service.
    pub fn term_engine_settings_match(&self) -> bool {
        self.term_engine().is_some_and(|term| {
            let main = self.main_engine();
            main.name == term.name
                && self.engine_detail(main, false) == self.engine_detail(term, true)
        })
    }
    fn selected_engine(&self, term: bool) -> Option<&'static EngineSpec> {
        engine_specs()
            .iter()
            .filter(|e| !term || e.supports_llm)
            .find(|engine| {
                let flag = if term {
                    format!("term_{}", engine.flag)
                } else {
                    engine.flag.clone()
                };
                self.document.get(&flag).and_then(Value::as_bool) == Some(true)
            })
    }
    pub(crate) fn engine_detail(&self, engine: &EngineSpec, term: bool) -> Value {
        let Some(key) = &engine.detail_key else {
            return json!({});
        };
        if term && self.selected_engine(true).is_some() {
            let mut object = serde_json::Map::new();
            if let Some(fields) = self.document[format!("term_{key}")].as_object() {
                for (key, value) in fields {
                    object.insert(
                        key.strip_prefix("term_").unwrap_or(key).into(),
                        value.clone(),
                    );
                }
            }
            Value::Object(object)
        } else {
            self.document[key].clone()
        }
    }

    /// Validation is pure: it does not touch input files, credentials, or APIs.
    pub fn validate(&self) -> Result<()> {
        let basic = self.basic()?;
        if basic.warmup {
            return Ok(());
        }
        if basic.generate_offline_assets.is_some() && basic.restore_offline_assets.is_some() {
            return Err(ConfigError::field("basic.generate_offline_assets"));
        }
        if basic.generate_offline_assets.is_some() {
            return Ok(());
        }
        let t = self.translation()?;
        let p = self.pdf()?;
        positive_u32(t.qps, "translation.qps")?;
        if let Some(value) = t.term_qps {
            positive_u32(value, "translation.term_qps")?;
        }
        if t.pool_max_workers
            .is_some_and(|v| v < 1 || v > u32::MAX as i64)
        {
            return Err(ConfigError::field("translation.pool_max_workers"));
        }
        if t.term_pool_max_workers
            .is_some_and(|v| v < 0 || v > u32::MAX as i64)
        {
            return Err(ConfigError::field("translation.term_pool_max_workers"));
        }
        if t.min_text_length < 0 {
            return Err(ConfigError::field("translation.min_text_length"));
        }
        if self.document["report_interval"]
            .as_f64()
            .is_none_or(|v| v < 0.05 || !v.is_finite())
        {
            return Err(ConfigError::field("report_interval"));
        }
        if p.no_mono && p.no_dual {
            return Err(ConfigError::field("pdf.no_mono"));
        }
        if p.split_short_lines && p.short_line_split_factor < 0.1 {
            return Err(ConfigError::field("pdf.short_line_split_factor"));
        }
        if p.max_pages_per_part.is_some_and(|v| v != 0 && v < 50) {
            return Err(ConfigError::field("pdf.max_pages_per_part"));
        }
        watermark(&p.watermark_output_mode)?;
        for (name, value) in [
            (
                "pdf.non_formula_line_iou_threshold",
                p.non_formula_line_iou_threshold,
            ),
            (
                "pdf.figure_table_protection_threshold",
                p.figure_table_protection_threshold,
            ),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(ConfigError::field(name));
            }
        }
        if t.primary_font_family
            .as_deref()
            .is_some_and(|v| !matches!(v, "" | "serif" | "sans-serif" | "script"))
        {
            return Err(ConfigError::field("translation.primary_font_family"));
        }
        if t.glossaries.as_deref().is_some_and(|v| !v.is_empty())
            && !self.main_engine().supports_llm
        {
            return Err(ConfigError::field("translation.glossaries"));
        }
        self.parse_pages()?;
        Ok(())
    }

    pub fn parse_pages(&self) -> Result<Option<Vec<(i64, i64)>>> {
        let pdf = self.pdf()?;
        let Some(pages) = pdf.pages.filter(|p| !p.is_empty()) else {
            return Ok(None);
        };
        let invalid = || ConfigError::field("pdf.pages");
        let mut ranges = Vec::new();
        for part in pages.split(',').map(str::trim) {
            let pieces: Vec<_> = part.split('-').collect();
            let range = match pieces.as_slice() {
                [page] => {
                    let value = page.parse::<i64>().map_err(|_| invalid())?;
                    if value < 1 {
                        return Err(invalid());
                    }
                    (value, value)
                }
                [start, end] => {
                    let start = if start.is_empty() {
                        1
                    } else {
                        start.trim().parse::<i64>().map_err(|_| invalid())?
                    };
                    let end = if end.is_empty() {
                        -1
                    } else {
                        end.trim().parse::<i64>().map_err(|_| invalid())?
                    };
                    if start < 1 || end < -1 || (end != -1 && start > end) {
                        return Err(invalid());
                    }
                    (start, end)
                }
                _ => return Err(invalid()),
            };
            ranges.push(range);
        }
        Ok(Some(ranges))
    }

    pub fn main_translator_config(&self) -> Result<TranslatorConfig> {
        self.validate()?;
        providers::build(self, self.main_engine(), false)
    }
    pub fn term_translator_config(&self) -> Result<Option<TranslatorConfig>> {
        self.validate()?;
        self.term_engine()
            .map(|e| providers::build(self, e, true))
            .transpose()
    }

    pub fn pdf_job(&self, input: PathBuf) -> Result<PdfJob> {
        self.validate()?;
        let basic = self.basic()?;
        let t = self.translation()?;
        let p = self.pdf()?;
        let mut extra = BTreeMap::new();
        let pdf_value = serde_json::to_value(&p).map_err(|_| ConfigError::field("pdf"))?;
        const DIRECT: &[&str] = &[
            "no_dual",
            "no_mono",
            "formular_font_pattern",
            "formular_char_pattern",
            "split_short_lines",
            "short_line_split_factor",
            "disable_rich_text_translate",
            "dual_translate_first",
            "enhance_compatibility",
            "use_alternating_pages_dual",
            "skip_clean",
            "skip_scanned_detection",
            "ocr_workaround",
            "auto_enable_ocr_workaround",
            "only_include_translated_page",
            "non_formula_line_iou_threshold",
            "figure_table_protection_threshold",
            "skip_formula_offset_calculation",
        ];
        for key in DIRECT {
            extra.insert((*key).into(), pdf_value[*key].clone());
        }
        extra.insert("debug".into(), json!(basic.debug));
        extra.insert("min_text_length".into(), json!(t.min_text_length));
        extra.insert(
            "report_interval".into(),
            self.document["report_interval"].clone(),
        );
        extra.insert(
            "merge_alternating_line_numbers".into(),
            json!(!p.no_merge_alternating_line_numbers),
        );
        extra.insert(
            "remove_non_formula_lines".into(),
            json!(!p.no_remove_non_formula_lines),
        );
        extra.insert("custom_system_prompt".into(), json!(t.custom_system_prompt));
        extra.insert("primary_font_family".into(), json!(t.primary_font_family));
        extra.insert(
            "term_pool_max_workers".into(),
            json!(t.term_pool_max_workers),
        );
        extra.insert(
            "save_auto_extracted_glossary".into(),
            json!(t.save_auto_extracted_glossary),
        );
        if let Some(qps) = t.term_qps {
            extra.insert("term_qps".into(), json!(qps));
        }
        if let Some(glossaries) = t.glossaries.filter(|v| !v.is_empty()) {
            extra.insert(
                "glossary_files".into(),
                json!(glossaries.split(',').collect::<Vec<_>>()),
            );
        }
        if let Some(pages) = p.max_pages_per_part.filter(|v| *v != 0) {
            extra.insert("max_pages_per_part".into(), json!(pages));
        }
        if p.enhance_compatibility {
            extra.insert("skip_clean".into(), json!(true));
            extra.insert("disable_rich_text_translate".into(), json!(true));
        }
        if p.auto_enable_ocr_workaround {
            extra.insert("ocr_workaround".into(), json!(false));
            extra.insert("skip_scanned_detection".into(), json!(false));
        }
        let qps = if self.main_engine().name == "SiliconFlowFree" {
            20
        } else {
            positive_u32(t.qps, "translation.qps")?
        };
        Ok(PdfJob {
            input_file: input,
            output_dir: t
                .output
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or(
                    std::env::current_dir().map_err(|_| ConfigError::new(ConfigErrorKind::Io))?,
                ),
            lang_in: t.lang_in,
            lang_out: t.lang_out,
            pages: p.pages,
            qps,
            pool_max_workers: t.pool_max_workers.map(|v| v as u32).unwrap_or(qps),
            no_auto_extract_glossary: t.no_auto_extract_glossary || self.term_engine().is_none(),
            watermark_output_mode: watermark(&p.watermark_output_mode)?.into(),
            extra,
        })
    }
}

pub(crate) fn positive_u32(value: i64, field: &str) -> Result<u32> {
    u32::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| ConfigError::field(field))
}
fn watermark(value: &str) -> Result<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "nowatermark" | "no_watermark" => Ok("no_watermark"),
        "watermarked" => Ok("watermarked"),
        "both" => Ok("both"),
        _ => Err(ConfigError::field("pdf.watermark_output_mode")),
    }
}
