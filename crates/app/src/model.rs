//! Pure document and presentation state. Construction never reads settings,
//! credentials, PDFs, environment variables or a Python runtime.

use anyhow::{Result, anyhow, bail, ensure};
use pdf2zh_config::{EngineSpec, FieldSpec, FieldType, Settings, engine_specs, field_specs};
use pdf2zh_tasks::{TaskSnapshot, TaskStatus};
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
        }
    }
    pub fn add_files(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Result<()> {
        for path in paths {
            ensure!(
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")),
                "Choose a PDF file"
            );
            ensure!(
                path.to_str().is_some(),
                "This filename cannot be represented in the settings file"
            );
            if !self.files.contains(&path) {
                self.files.push(path);
                self.dirty = true;
            }
        }
        Ok(())
    }
    pub fn remove_file(&mut self, index: usize) {
        if index < self.files.len() {
            self.files.remove(index);
            self.dirty = true;
        }
    }
    pub fn set(&mut self, path: &str, value: Value) -> Result<()> {
        self.settings.set(path, value)?;
        self.drafts.remove(path);
        self.dirty = true;
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
            self.dirty = true;
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
            let value = parse_draft(spec, text)?;
            settings.set(path, value)?;
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
        if for_translation {
            ensure!(!self.files.is_empty(), "Choose at least one PDF");
            let pdf = settings.pdf()?;
            ensure!(
                !(pdf.no_mono && pdf.no_dual),
                "Choose at least one PDF output"
            );
            settings.validate()?;
        }
        Ok(settings)
    }
    pub fn saved(&mut self, settings: Settings) {
        self.settings = settings;
        self.drafts.clear();
        self.dirty = false;
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
            .parse::<i64>()
            .map(|value| json!(value))
            .map_err(|_| invalid()),
        FieldType::Float => text
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
        FieldType::StringSet => serde_json::from_str(text).map_err(|_| invalid()),
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
