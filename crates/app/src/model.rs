//! Pure document and presentation state. Construction never reads settings,
//! credentials, PDFs, environment variables or a Python runtime.

use anyhow::{Result, anyhow, bail, ensure};
use babeloxide_config::{EngineSpec, FieldSpec, FieldType, Settings, engine_specs, field_specs};
use babeloxide_tasks::{TaskSnapshot, TaskStatus};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    Original,
    Mono,
    Dual,
}

pub struct Editor {
    pub settings: Settings,
    // Intentionally no Debug implementation: drafts may contain API keys.
    pub drafts: BTreeMap<String, String>,
    pub files: Vec<PathBuf>,
    pub dirty: bool,
    pub revision: u64,
}
impl Editor {
    pub fn new(settings: Settings) -> Self {
        let files = settings
            .basic()
            .map(|basic| basic.input_files.into_iter().map(PathBuf::from).collect())
            .unwrap_or_default();
        Self {
            settings,
            drafts: BTreeMap::new(),
            files,
            dirty: false,
            revision: 0,
        }
    }
    pub fn add_files(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Result<()> {
        let paths: Vec<_> = paths.into_iter().collect();
        for path in &paths {
            ensure!(
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")),
                "Choose a PDF file"
            );
            ensure!(
                path.to_str().is_some(),
                "This filename cannot be represented in the settings file"
            );
        }
        let mut changed = false;
        for path in paths {
            if !self.files.contains(&path) {
                self.files.push(path);
                changed = true;
            }
        }
        if changed {
            self.mark_dirty();
        }
        Ok(())
    }
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn invalid_fields(&self) -> Vec<&'static FieldSpec> {
        field_specs()
            .iter()
            .filter(|spec| {
                let stored = match self.settings.get(&spec.path) {
                    Some(Value::String(value)) => value.clone(),
                    None | Some(Value::Null) => String::new(),
                    Some(value) => value.to_string(),
                };
                let text = self.drafts.get(&spec.path).unwrap_or(&stored);
                crate::settings_ui::validate_draft(spec, text).is_err()
            })
            .collect()
    }
    pub fn remove_file(&mut self, index: usize) {
        if index < self.files.len() {
            self.files.remove(index);
            self.mark_dirty();
        }
    }
    pub fn set(&mut self, path: &str, value: Value) -> Result<()> {
        self.settings.set(path, value)?;
        self.drafts.remove(path);
        self.mark_dirty();
        Ok(())
    }
    pub fn select_engine(&mut self, flag: Option<&str>, term: bool) -> Result<()> {
        if let Some(flag) = flag {
            self.set(
                &if term {
                    format!("term_{flag}")
                } else {
                    flag.into()
                },
                json!(true),
            )?;
        } else if term {
            for engine in engine_specs().iter().filter(|engine| engine.supports_llm) {
                self.settings
                    .set(&format!("term_{}", engine.flag), json!(false))?;
            }
            self.mark_dirty();
        }
        Ok(())
    }
    /// Settings::term_engine resolves the legacy fallback to the main LLM.
    /// The selector needs the explicit flag so "same as document" remains an
    /// editable state rather than accidentally showing unused term fields.
    pub fn explicit_term_engine(&self) -> Option<&'static EngineSpec> {
        engine_specs()
            .iter()
            .filter(|engine| engine.supports_llm)
            .find(|engine| {
                self.settings
                    .get(&format!("term_{}", engine.flag))
                    .and_then(Value::as_bool)
                    == Some(true)
            })
    }
    pub fn draft(&mut self, spec: &FieldSpec) -> &mut String {
        self.drafts.entry(spec.path.clone()).or_insert_with(|| {
            match self.settings.get(&spec.path) {
                Some(Value::String(value)) => value.clone(),
                Some(Value::Null) | None => String::new(),
                Some(value) => value.to_string(),
            }
        })
    }
    /// Apply edits to a clone so validation errors never partially mutate the
    /// document being saved or submitted. Unknown fields stay in Settings.
    pub fn snapshot(&self, for_translation: bool) -> Result<Settings> {
        let mut settings = self.settings.clone();
        for (path, text) in &self.drafts {
            let Some(spec) = field_specs().iter().find(|spec| spec.path == *path) else {
                continue;
            };
            let original = match self.settings.get(path) {
                Some(Value::String(value)) => value.clone(),
                None | Some(Value::Null) => String::new(),
                Some(value) => value.to_string(),
            };
            if *text == original {
                continue;
            }
            let value = parse_draft(spec, text)?;
            crate::settings_ui::validate_draft(spec, text)?;
            settings.set(path, value)?;
        }
        // Validate every populated setting, including direct controls and older
        // configs; drafts remain untouched if any check fails.
        for spec in field_specs() {
            if let Some(value) = settings.get(&spec.path) {
                let text = match value {
                    Value::Null => String::new(),
                    Value::String(value) => value.clone(),
                    value => value.to_string(),
                };
                crate::settings_ui::validate_draft(spec, &text)?;
            }
        }
        settings.set(
            "basic.input_files",
            json!(
                self.files
                    .iter()
                    .map(|path| path.to_str().expect("validated UTF-8 filename"))
                    .collect::<Vec<_>>()
            ),
        )?;
        settings.validate()?;
        if for_translation {
            ensure!(!self.files.is_empty(), "Choose at least one PDF");
            let pdf = settings.pdf()?;
            ensure!(
                !(pdf.no_mono && pdf.no_dual),
                "Choose at least one PDF output"
            );
            ensure!(
                self.engine_enabled(settings.main_engine()),
                "The selected service is disabled"
            );
            let translation = settings.translation()?;
            for (code, source) in [(&translation.lang_in, true), (&translation.lang_out, false)] {
                ensure!(
                    crate::languages::supported(&settings.main_engine().name, code, source)
                        != Some(false),
                    "The selected service does not support this language"
                );
            }
            if let Some(engine) = self.explicit_term_engine() {
                ensure!(
                    self.engine_enabled(engine),
                    "The selected terminology service is disabled"
                );
            }
        }
        Ok(settings)
    }
    pub fn saved(&mut self, settings: Settings) {
        self.settings = settings;
        self.drafts.clear();
        self.dirty = false;
    }
    pub fn saved_at(&mut self, settings: Settings, revision: u64) -> bool {
        if self.revision != revision {
            return false;
        }
        self.saved(settings);
        true
    }
    pub fn engine_enabled(&self, engine: &EngineSpec) -> bool {
        let configured = self
            .drafts
            .get("gui_settings.enabled_services")
            .map(String::as_str)
            .or_else(|| {
                self.settings
                    .get("gui_settings.enabled_services")
                    .and_then(Value::as_str)
            })
            .unwrap_or_default();
        configured.trim().is_empty()
            || configured
                .split(',')
                .any(|name| name.trim().eq_ignore_ascii_case(&engine.name))
    }
    pub fn toggle_service(&mut self, engine: &EngineSpec, enabled: bool) -> Result<()> {
        let mut names: Vec<String> = engine_specs()
            .iter()
            .filter(|item| self.engine_enabled(item))
            .map(|item| item.name.clone())
            .collect();
        names.retain(|name| !name.eq_ignore_ascii_case(&engine.name));
        if enabled {
            names.push(engine.name.clone());
        }
        ensure!(!names.is_empty(), "Keep at least one service enabled");
        self.set("gui_settings.enabled_services", json!(names.join(",")))
    }
    pub fn swap_languages(&mut self) -> Result<()> {
        let input = "translation.lang_in";
        let output = "translation.lang_out";
        let value = |path| {
            self.drafts
                .get(path)
                .cloned()
                .or_else(|| {
                    self.settings
                        .get(path)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_default()
        };
        let mut settings = self.settings.clone();
        settings.set(input, json!(value(output)))?;
        settings.set(output, json!(value(input)))?;
        let translation = settings.translation()?;
        for (code, source) in [(&translation.lang_in, true), (&translation.lang_out, false)] {
            ensure!(
                crate::languages::supported(&settings.main_engine().name, code, source)
                    != Some(false),
                "The selected service does not support this language"
            );
        }
        self.settings = settings;
        self.drafts.remove(input);
        self.drafts.remove(output);
        self.mark_dirty();
        Ok(())
    }
}

pub fn parse_draft(spec: &FieldSpec, text: &str) -> Result<Value> {
    if text.is_empty() && spec.nullable {
        return Ok(Value::Null);
    }
    let invalid = || anyhow!("Check the value for {}", spec.flag);
    match spec.kind {
        FieldType::String => Ok(json!(text)),
        FieldType::Integer => text
            .trim()
            .parse::<i64>()
            .map(|value| json!(value))
            .map_err(|_| invalid()),
        FieldType::Float => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| json!(value))
            .ok_or_else(invalid),
        FieldType::Bool => match text {
            "true" => Ok(json!(true)),
            "false" => Ok(json!(false)),
            _ => Err(invalid()),
        },
        FieldType::StringSet => {
            let values: Vec<String> = serde_json::from_str(text).map_err(|_| invalid())?;
            Ok(json!(values))
        }
    }
}

pub fn result_path(
    snapshot: &TaskSnapshot,
    file: usize,
    kind: DocumentKind,
    inputs: &[PathBuf],
) -> Option<PathBuf> {
    match kind {
        DocumentKind::Original => inputs.get(file).cloned(),
        DocumentKind::Mono => snapshot
            .results
            .get(file)?
            .get("mono_pdf_path")?
            .as_str()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
        DocumentKind::Dual => snapshot
            .results
            .get(file)?
            .get("dual_pdf_path")?
            .as_str()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
    }
}

#[derive(Clone, Copy)]
pub enum ExportKind {
    All,
    Mono,
    Dual,
}

pub fn batch_pdf_results(snapshot: &TaskSnapshot, kind: ExportKind) -> Result<Vec<PathBuf>> {
    ensure!(
        snapshot.status == TaskStatus::Succeeded
            && snapshot.total_files > 0
            && snapshot.results.len() == snapshot.total_files,
        "The batch has no complete PDF results"
    );
    let mut paths = Vec::new();
    for result in &snapshot.results {
        let mut found = false;
        for (field, wanted) in [
            ("mono_pdf_path", !matches!(kind, ExportKind::Dual)),
            ("dual_pdf_path", !matches!(kind, ExportKind::Mono)),
        ] {
            if wanted
                && let Some(path) = result
                    .get(field)
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
            {
                paths.push(PathBuf::from(path));
                found = true;
            }
        }
        ensure!(found, "The batch has no complete PDF results");
    }
    Ok(paths)
}

pub fn progress_fraction(snapshot: &TaskSnapshot) -> Option<f32> {
    if snapshot.status == TaskStatus::Succeeded {
        return Some(1.0);
    }
    let total = snapshot.total_files.max(1) as f64;
    let file_fraction = snapshot.progress.as_ref().and_then(|event| {
        // BabelDOC's overall_progress is a weighted 0..100 percentage, even
        // below 1%. Its part offsets are already included by ProgressMonitor.
        let percentage = event
            .get("overall_progress")
            .or_else(|| event.get("percentage"))
            .and_then(Value::as_f64);
        if let Some(percentage) = percentage.filter(|number| number.is_finite()) {
            return Some(percentage / 100.0);
        }
        let done = event
            .get("current")
            .or_else(|| event.get("completed"))
            .or_else(|| event.get("stage_current"))
            .and_then(Value::as_f64)?;
        let maximum = event
            .get("total")
            .or_else(|| event.get("stage_total"))
            .and_then(Value::as_f64)?;
        (done.is_finite() && maximum.is_finite() && maximum > 0.0).then_some(done / maximum)
    });
    file_fraction.map(|fraction| {
        ((snapshot.completed_files as f64 + fraction.clamp(0.0, 0.99)) / total).clamp(0.0, 0.999)
            as f32
    })
}

pub fn validate_result_path(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "The result path is invalid");
    if !path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("pdf") || extension.eq_ignore_ascii_case("csv")
    }) {
        bail!("Choose a PDF or glossary result");
    }
    ensure!(path.is_file(), "The result is no longer available");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_preserve_unknown_fields_and_do_not_touch_disk() {
        let mut settings = Settings::default();
        settings
            .set("future_section", json!({"value":"preserved"}))
            .unwrap();
        let mut editor = Editor::new(settings);
        editor.select_engine(Some("google"), false).unwrap();
        editor.set("translation.lang_out", json!("ja")).unwrap();
        assert_eq!(
            editor.snapshot(false).unwrap().get("future_section.value"),
            Some(&json!("preserved"))
        );
        assert!(editor.dirty);
        assert_eq!(engine_specs().len(), 24);
    }
    #[test]
    fn invalid_secret_or_numeric_edits_never_echo_the_value() {
        let spec = field_specs()
            .iter()
            .find(|spec| spec.kind == FieldType::Integer)
            .unwrap();
        let error = parse_draft(spec, "private-key-content").unwrap_err();
        assert!(!error.to_string().contains("private-key-content"));
    }
    #[test]
    fn file_selection_deduplicates_and_rejects_non_pdf() {
        let mut editor = Editor::new(Settings::default());
        editor
            .add_files([PathBuf::from("原文.pdf"), PathBuf::from("原文.pdf")])
            .unwrap();
        assert_eq!(editor.files.len(), 1);
        assert!(editor.add_files([PathBuf::from("bad.exe")]).is_err());
        editor.remove_file(0);
        assert!(editor.files.is_empty());
    }
    #[test]
    fn adding_a_mixed_batch_is_atomic_and_keeps_existing_selection() {
        let mut editor = Editor::new(Settings::default());
        editor.add_files([PathBuf::from("existing.pdf")]).unwrap();
        let revision = editor.revision;
        editor.dirty = false;
        assert!(
            editor
                .add_files([PathBuf::from("valid.pdf"), PathBuf::from("invalid.txt")])
                .is_err()
        );
        assert_eq!(editor.files, [PathBuf::from("existing.pdf")]);
        assert_eq!(editor.revision, revision);
        assert!(!editor.dirty);
    }
    #[test]
    fn invalid_inactive_stored_fields_have_a_visible_repair_entry() {
        let mut editor = Editor::new(Settings::default());
        editor
            .set("deepseek_detail.deepseek_model", json!("deepseek-v4-pro"))
            .unwrap();
        let path = "deepseek_detail.deepseek_reasoning_effort";
        editor.set(path, json!("unexpected-effort")).unwrap();
        assert!(editor.drafts.is_empty());
        assert!(editor.invalid_fields().iter().any(|spec| spec.path == path));
        assert!(editor.snapshot(false).is_err());
        editor.set(path, json!("high")).unwrap();
        assert!(editor.snapshot(false).is_ok());
    }
    #[test]
    fn an_unchanged_empty_optional_string_is_not_cleared_by_a_rendered_draft() {
        let mut editor = Editor::new(Settings::default());
        let path = "openai_detail.openai_api_key";
        editor.set(path, json!("")).unwrap();
        editor.drafts.insert(path.into(), String::new());
        assert_eq!(editor.snapshot(false).unwrap().get(path), Some(&json!("")));
    }
    #[test]
    fn default_interface_is_traditional_chinese_and_explicit_english_survives() {
        let mut editor = Editor::new(Settings::default());
        assert_eq!(
            editor.settings.gui().unwrap().ui_lang.as_deref(),
            Some("zh-TW")
        );
        editor.set("gui_settings.ui_lang", json!("en")).unwrap();
        assert_eq!(
            editor
                .snapshot(false)
                .unwrap()
                .gui()
                .unwrap()
                .ui_lang
                .as_deref(),
            Some("en")
        );
    }
    #[test]
    fn invalid_save_drafts_leave_settings_and_credentials_intact() {
        let mut editor = Editor::new(Settings::default());
        editor
            .set("openai_detail.openai_api_key", json!("synthetic-preserved"))
            .unwrap();
        let before = editor.settings.as_value().clone();
        editor.drafts.insert("translation.qps".into(), "0".into());
        assert!(editor.snapshot(false).is_err());
        assert_eq!(editor.settings.as_value(), &before);
        assert_eq!(editor.drafts["translation.qps"], "0");
        editor.drafts.insert("translation.qps".into(), "6".into());
        let saved = editor.snapshot(false).unwrap();
        editor.set("translation.lang_out", json!("ja")).unwrap();
        assert!(!editor.saved_at(saved, editor.revision.wrapping_sub(1)));
        assert_eq!(
            editor.settings.get("translation.lang_out"),
            Some(&json!("ja"))
        );
        assert!(editor.dirty);
    }
    #[test]
    fn enabled_services_are_case_insensitive_and_do_not_erase_other_profiles() {
        let mut editor = Editor::new(Settings::default());
        editor
            .set("gui_settings.enabled_services", json!("gOoGle, DeepL"))
            .unwrap();
        editor
            .set("openai_detail.openai_api_key", json!("synthetic-preserved"))
            .unwrap();
        assert!(!editor.engine_enabled(editor.settings.main_engine()));
        let google = engine_specs()
            .iter()
            .find(|engine| engine.flag == "google")
            .unwrap();
        assert!(editor.engine_enabled(google));
        editor.toggle_service(google, false).unwrap();
        assert!(!editor.engine_enabled(google));
        assert_eq!(
            editor.settings.get("openai_detail.openai_api_key"),
            Some(&json!("synthetic-preserved"))
        );
    }
    #[test]
    fn language_swap_preserves_codes_and_survives_save_and_reload() {
        let mut editor = Editor::new(Settings::default());
        editor.set("translation.lang_in", json!("en")).unwrap();
        editor.set("translation.lang_out", json!("zh-TW")).unwrap();
        editor.swap_languages().unwrap();
        let settings = editor.snapshot(false).unwrap();
        let reloaded = Editor::new(Settings::from_value(settings.into_value()).unwrap());
        assert_eq!(reloaded.settings.translation().unwrap().lang_in, "zh-TW");
        assert_eq!(reloaded.settings.translation().unwrap().lang_out, "en");
    }
    #[test]
    fn term_selection_is_optional_and_uses_llm_flags() {
        let mut editor = Editor::new(Settings::default());
        editor.select_engine(Some("openai"), true).unwrap();
        assert_eq!(editor.settings.term_engine().unwrap().flag, "openai");
        editor.select_engine(None, true).unwrap();
        assert!(editor.explicit_term_engine().is_none());
        assert_eq!(
            editor.settings.term_engine().unwrap().flag,
            editor.settings.main_engine().flag
        );
    }
    fn snapshot() -> TaskSnapshot {
        TaskSnapshot {
            id: 1,
            status: TaskStatus::Running,
            total_files: 2,
            completed_files: 1,
            current_file: Some(1),
            progress: None,
            results: vec![],
            failure: None,
        }
    }
    #[test]
    fn progress_uses_engine_weighted_percent_even_when_below_one_percent() {
        let mut state = snapshot();
        state.progress = Some(
            json!({"overall_progress":0.5,"part_index":1,"total_parts":3,"stage_current":99,"stage_total":100}),
        );
        assert!((progress_fraction(&state).unwrap() - 0.5025).abs() < 0.00001);
        state.progress = Some(json!({"overall_progress":50.0}));
        assert!((progress_fraction(&state).unwrap() - 0.75).abs() < 0.00001);
        state.status = TaskStatus::Succeeded;
        assert_eq!(progress_fraction(&state), Some(1.0));
    }
    #[test]
    fn result_selection_keeps_input_and_output_documents_distinct() {
        let mut state = snapshot();
        state.results =
            vec![json!({"mono_pdf_path":"translated.pdf","dual_pdf_path":"bilingual.pdf"})];
        let inputs = vec![PathBuf::from("original.pdf")];
        assert_eq!(
            result_path(&state, 0, DocumentKind::Original, &inputs),
            Some(PathBuf::from("original.pdf"))
        );
        assert_eq!(
            result_path(&state, 0, DocumentKind::Mono, &inputs),
            Some(PathBuf::from("translated.pdf"))
        );
        assert_eq!(
            result_path(&state, 0, DocumentKind::Dual, &inputs),
            Some(PathBuf::from("bilingual.pdf"))
        );
        assert!(result_path(&state, 1, DocumentKind::Mono, &inputs).is_none());
    }
}
