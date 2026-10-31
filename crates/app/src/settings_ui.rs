//! Schema-aware desktop controls. Drafts are retained until an atomic snapshot.
use crate::{
    model::{Editor, parse_draft},
    strings,
};
use anyhow::{Result, bail};
use eframe::egui;
use pdf2zh_config::{FieldSpec, FieldType};
use serde_json::{Value, json};

pub fn group(spec: &FieldSpec) -> &'static str {
    match spec.path.as_str() {
        p if p.starts_with("pdf.") => "PDF processing",
        p if p.starts_with("translation.") && (p.contains("qps") || p.contains("workers")) => {
            "Performance and rate limits"
        }
        p if p.starts_with("translation.") => "Basic translation settings",
        p if p.contains("_detail.") => "Service and API",
        _ => "Other advanced settings",
    }
}
pub fn advanced(spec: &FieldSpec) -> bool {
    if spec.path.contains("_detail.") {
        let p = spec.path.as_str();
        return !(p.ends_with("_model")
            || p.ends_with("_api_key")
            || p.ends_with("_apikey")
            || p.ends_with("_auth_key")
            || p.ends_with("_secret_id")
            || p.ends_with("_secret_key")
            || p.ends_with("_base_url")
            || p.ends_with("_host")
            || p.ends_with("_url")
            || p.ends_with("_endpoint"));
    }
    !matches!(
        spec.path.as_str(),
        "translation.lang_in"
            | "translation.lang_out"
            | "translation.output"
            | "translation.qps"
            | "pdf.pages"
            | "pdf.no_mono"
            | "pdf.no_dual"
    )
}

pub fn choices(spec: &FieldSpec) -> Option<&'static [&'static str]> {
    match spec.path.as_str() {
        "translation.primary_font_family" => Some(&["serif", "sans-serif", "script"]),
        "pdf.watermark_output_mode" => Some(&["no_watermark", "watermarked", "both"]),
        p if p.ends_with("deepseek_thinking_mode") => Some(&["enabled", "disabled"]),
        p if p.ends_with("deepseek_reasoning_effort") => Some(&["high", "max"]),
        _ => None,
    }
}

pub fn visible(editor: &Editor, spec: &FieldSpec) -> bool {
    if editor
        .drafts
        .get(&spec.path)
        .is_some_and(|text| validate_draft(spec, text).is_err())
    {
        return true;
    }
    if (spec.path.ends_with("deepseek_reasoning_effort")
        || spec.path.ends_with("deepseek_thinking_mode"))
        && !editor.drafts.contains_key(&spec.path)
        && editor
            .settings
            .get(&spec.path)
            .and_then(Value::as_str)
            .is_some_and(|text| validate_draft(spec, text).is_err())
    {
        return true;
    }
    if spec.path == "pdf.short_line_split_factor" {
        return editor
            .settings
            .get("pdf.split_short_lines")
            .and_then(Value::as_bool)
            == Some(true);
    }
    let p = spec.path.as_str();
    let dependency = if p.ends_with("openai_compatible_temperature")
        || p.ends_with("aliyun_dashscope_temperature")
    {
        p.strip_suffix("temperature")
            .map(|prefix| format!("{prefix}send_temperature"))
    } else if p.ends_with("openai_temperature") {
        p.strip_suffix("temperature")
            .map(|prefix| format!("{prefix}send_temprature"))
    } else if p.ends_with("openai_compatible_reasoning_effort")
        || p.ends_with("openai_reasoning_effort")
    {
        p.strip_suffix("reasoning_effort")
            .map(|prefix| format!("{prefix}send_reasoning_effort"))
    } else if p.ends_with("siliconflow_enable_thinking") {
        p.strip_suffix("enable_thinking")
            .map(|prefix| format!("{prefix}send_enable_thinking_param"))
    } else {
        None
    };
    if let Some(path) = dependency {
        return editor
            .drafts
            .get(&path)
            .map(String::as_str)
            .map(|value| value == "true")
            .unwrap_or_else(|| editor.settings.get(&path).and_then(Value::as_bool) == Some(true));
    }
    if spec.path.ends_with("deepseek_reasoning_effort")
        || spec.path.ends_with("deepseek_thinking_mode")
    {
        let (section, _) = spec.path.split_once('.').unwrap_or(("", ""));
        let prefix = if section.starts_with("term_") {
            "term_"
        } else {
            ""
        };
        let mode = format!("{section}.{prefix}deepseek_thinking_mode");
        let model = format!("{section}.{prefix}deepseek_model");
        let read = |p: &str| {
            editor
                .drafts
                .get(p)
                .map(String::as_str)
                .or_else(|| editor.settings.get(p).and_then(Value::as_str))
                .unwrap_or("")
        };
        return read(&model).starts_with("deepseek-v4-")
            && (spec.path.ends_with("deepseek_thinking_mode") || read(&mode) == "enabled");
    }
    true
}

fn url_field(spec: &FieldSpec) -> bool {
    let p = spec.path.as_str();
    p.ends_with("_base_url")
        || p.ends_with("_url")
        || p.ends_with("_endpoint")
        || p.ends_with("_host")
}
fn looks_like_http_url(text: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(text) else {
        return false;
    };
    let authority = text
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""));
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && authority.is_some_and(|authority| !authority.contains('@'))
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}
/// Validate a draft before snapshot; errors never include the user's value.
pub fn validate_draft(spec: &FieldSpec, text: &str) -> Result<()> {
    let value = parse_draft(spec, text)?;
    if value.is_null() {
        return Ok(());
    }
    if let Some(options) = choices(spec)
        && !options.contains(&text)
        && !(spec.path == "pdf.watermark_output_mode" && text.eq_ignore_ascii_case("nowatermark"))
        && !(spec.path == "pdf.watermark_output_mode"
            && options
                .iter()
                .any(|option| option.eq_ignore_ascii_case(text)))
    {
        bail!("Check the value for {}", spec.flag);
    }
    if spec.kind == FieldType::StringSet && serde_json::from_str::<Vec<String>>(text).is_err() {
        bail!("Check the value for {}", spec.flag);
    }
    if url_field(spec) && !text.is_empty() && !looks_like_http_url(text) {
        bail!("Check the value for {}", spec.flag);
    }
    let valid = match spec.path.as_str() {
        "translation.qps" | "translation.term_qps" | "translation.pool_max_workers" => value
            .as_i64()
            .is_some_and(|n| n > 0 && n <= u32::MAX as i64),
        "translation.term_pool_max_workers" => value
            .as_i64()
            .is_some_and(|n| n >= 0 && n <= u32::MAX as i64),
        "translation.min_text_length" => value.as_i64().is_some_and(|n| n >= 0),
        "clitranslator_detail.clitranslator_timeout" => {
            value.as_i64().is_some_and(|n| (1..=300).contains(&n))
        }
        "pdf.max_pages_per_part" => value.as_i64().is_some_and(|n| n == 0 || n >= 50),
        "pdf.non_formula_line_iou_threshold" | "pdf.figure_table_protection_threshold" => {
            value.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n))
        }
        "report_interval" => value.as_f64().is_some_and(|n| n >= 0.05),
        p if p.ends_with("openai_timeout")
            || p.ends_with("openai_compatible_timeout")
            || p.ends_with("aliyun_dashscope_timeout") =>
        {
            text.parse::<f64>().is_ok_and(|n| n.is_finite() && n > 0.0)
        }
        p if p.ends_with("openai_temperature")
            || p.ends_with("openai_compatible_temperature")
            || p.ends_with("aliyun_dashscope_temperature") =>
        {
            text.parse::<f64>().is_ok_and(f64::is_finite)
        }
        _ => true,
    };
    if !valid {
        bail!("Check the value for {}", spec.flag);
    }
    Ok(())
}

fn path_kind(spec: &FieldSpec) -> Option<bool> {
    match spec.path.as_str() {
        "translation.output" | "basic.generate_offline_assets" => Some(true),
        "basic.restore_offline_assets"
        | "gui_settings.auth_file"
        | "gui_settings.welcome_page"
        | "config_file"
        | "claudecode_detail.claude_code_path" => Some(false),
        _ => None,
    }
}
#[cfg(windows)]
fn pick_path(frame: &eframe::Frame, directory: bool) -> Option<std::path::PathBuf> {
    let dialog = rfd::FileDialog::new().set_parent(frame);
    if directory {
        dialog.pick_folder()
    } else {
        dialog.pick_file()
    }
}
#[cfg(not(windows))]
fn pick_path(_frame: &eframe::Frame, _directory: bool) -> Option<std::path::PathBuf> {
    None
}

pub fn field(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    spec: &FieldSpec,
    label: Option<&str>,
    chinese: bool,
    frame: &eframe::Frame,
) -> Result<()> {
    let label = label
        .map(str::to_owned)
        .unwrap_or_else(|| strings::field_label(spec, chinese));
    let help = strings::field_help(spec, chinese);
    let disabled = spec.secret
        && editor
            .settings
            .gui()
            .is_ok_and(|gui| gui.disable_gui_sensitive_input);
    let mut result = Ok(());
    ui.push_id(&spec.path, |ui| {
        ui.add_enabled_ui(!disabled, |ui| {
            if spec.kind == FieldType::Bool {
                if spec.nullable {
                    let current = editor.settings.get(&spec.path).and_then(Value::as_bool);
                    let mut value = current;
                    ui.horizontal(|ui| {
                        ui.label(&label).on_hover_text(&help);
                        let selected = match value {
                            Some(true) => "Enabled",
                            Some(false) => "Disabled",
                            None => "Not set",
                        };
                        egui::ComboBox::from_id_salt("boolean")
                            .selected_text(strings::ui(selected, chinese))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut value,
                                    None,
                                    strings::ui("Not set", chinese),
                                );
                                ui.selectable_value(
                                    &mut value,
                                    Some(true),
                                    strings::ui("Enabled", chinese),
                                );
                                ui.selectable_value(
                                    &mut value,
                                    Some(false),
                                    strings::ui("Disabled", chinese),
                                );
                            });
                    });
                    if value != current {
                        result = editor.set(&spec.path, value.map_or(Value::Null, |v| json!(v)));
                    }
                } else {
                    let mut value = editor
                        .settings
                        .get(&spec.path)
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if ui
                        .checkbox(&mut value, &label)
                        .on_hover_text(&help)
                        .changed()
                    {
                        result = editor.set(&spec.path, json!(value));
                    }
                }
            } else if let Some(options) = choices(spec) {
                let original = editor.drafts.get(&spec.path).cloned().unwrap_or_else(|| {
                    editor
                        .settings
                        .get(&spec.path)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                });
                let mut selected = original.clone();
                ui.horizontal(|ui| {
                    ui.label(&label).on_hover_text(&help);
                    egui::ComboBox::from_id_salt("choice")
                        .selected_text(if selected.is_empty() {
                            strings::ui("Not set", chinese)
                        } else {
                            options
                                .iter()
                                .find(|option| **option == selected.as_str())
                                .map_or(selected.as_str(), |option| strings::ui(option, chinese))
                        })
                        .show_ui(ui, |ui| {
                            if spec.nullable {
                                ui.selectable_value(
                                    &mut selected,
                                    String::new(),
                                    strings::ui("Not set", chinese),
                                );
                            }
                            for option in options {
                                ui.selectable_value(
                                    &mut selected,
                                    (*option).to_owned(),
                                    strings::ui(option, chinese),
                                );
                            }
                        });
                });
                if validate_draft(spec, &selected).is_err() {
                    ui.colored_label(
                        egui::Color32::from_rgb(180, 40, 40),
                        strings::ui("Invalid value", chinese),
                    );
                }
                if selected != original {
                    result = editor.set(
                        &spec.path,
                        if selected.is_empty() && spec.nullable {
                            Value::Null
                        } else {
                            json!(selected)
                        },
                    );
                }
            } else {
                ui.label(&label).on_hover_text(&help);
                let mut buffer = editor.drafts.get(&spec.path).cloned().unwrap_or_else(|| {
                    match editor.settings.get(&spec.path) {
                        Some(Value::String(value)) => value.clone(),
                        Some(Value::Null) | None => String::new(),
                        Some(value) => value.to_string(),
                    }
                });
                let mut chosen = None;
                let mut changed = ui
                    .horizontal(|ui| {
                        let multiline = spec.kind == FieldType::StringSet
                            || spec.path.ends_with("custom_system_prompt")
                            || spec.path.ends_with("postprocess_command");
                        let edit = if multiline {
                            egui::TextEdit::multiline(&mut buffer).desired_rows(2)
                        } else {
                            egui::TextEdit::singleline(&mut buffer)
                        };
                        let numeric = matches!(spec.kind, FieldType::Integer | FieldType::Float);
                        let reserve = if numeric {
                            80.0
                        } else if path_kind(spec).is_some() {
                            40.0
                        } else {
                            0.0
                        };
                        let response = ui.add(
                            edit.password(spec.secret)
                                .desired_width((ui.available_width() - reserve).max(40.0)),
                        );
                        let mut stepped = false;
                        if numeric {
                            if spec.kind == FieldType::Integer {
                                if let Ok(mut value) = buffer.parse::<i64>()
                                    && ui.add(egui::DragValue::new(&mut value)).changed()
                                {
                                    buffer = value.to_string();
                                    stepped = true;
                                }
                            } else if let Ok(mut value) = buffer.parse::<f64>()
                                && value.is_finite()
                                && ui
                                    .add(egui::DragValue::new(&mut value).speed(0.01))
                                    .changed()
                            {
                                buffer = value.to_string();
                                stepped = true;
                            }
                        }
                        if let Some(directory) = path_kind(spec)
                            && ui
                                .small_button("…")
                                .on_hover_text(strings::ui("Choose…", chinese))
                                .clicked()
                        {
                            chosen = pick_path(frame, directory);
                        }
                        response.changed() || stepped
                    })
                    .inner;
                if let Some(path) = chosen
                    && let Some(path) = path.to_str()
                {
                    buffer = path.to_owned();
                    changed = true;
                }
                if changed {
                    editor.drafts.insert(spec.path.clone(), buffer);
                    editor.mark_dirty();
                }
                if let Some(text) = editor.drafts.get(&spec.path)
                    && (validate_draft(spec, text).is_err()
                        || (spec.path == "pdf.short_line_split_factor"
                            && editor
                                .settings
                                .get("pdf.split_short_lines")
                                .and_then(Value::as_bool)
                                == Some(true)
                            && text.parse::<f64>().is_ok_and(|value| value < 0.1)))
                {
                    let key = if url_field(spec) {
                        "Enter a valid HTTP or HTTPS URL"
                    } else if spec.kind == FieldType::StringSet {
                        "JSON array of strings"
                    } else {
                        "Invalid value"
                    };
                    ui.colored_label(
                        egui::Color32::from_rgb(180, 40, 40),
                        strings::ui(key, chinese),
                    );
                }
            }
        });
        if disabled {
            ui.small(strings::ui(
                "Sensitive fields are disabled by settings",
                chinese,
            ));
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec(path: &str) -> &'static FieldSpec {
        pdf2zh_config::field_specs()
            .iter()
            .find(|s| s.path == path)
            .unwrap()
    }
    #[test]
    fn numeric_ranges_and_null() {
        assert!(validate_draft(spec("translation.pool_max_workers"), "").is_ok());
        assert!(validate_draft(spec("translation.pool_max_workers"), "0").is_err());
        assert!(validate_draft(spec("translation.term_pool_max_workers"), "0").is_ok());
        assert!(validate_draft(spec("pdf.max_pages_per_part"), "49").is_err());
        assert!(validate_draft(spec("pdf.max_pages_per_part"), "50").is_ok());
        assert!(validate_draft(spec("pdf.non_formula_line_iou_threshold"), "1.1").is_err());
    }
    #[test]
    fn only_json_string_arrays_are_valid() {
        assert!(validate_draft(spec("basic.input_files"), "[\"a.pdf\"]").is_ok());
        assert!(validate_draft(spec("basic.input_files"), "[1]").is_err());
    }
    #[test]
    fn dependent_fields_and_invalid_drafts_remain_visible() {
        let mut editor = Editor::new(pdf2zh_config::Settings::default());
        let temperature = spec("openai_detail.openai_temperature");
        editor
            .set("openai_detail.openai_send_temprature", json!(false))
            .unwrap();
        assert!(!visible(&editor, temperature));
        editor.drafts.insert(temperature.path.clone(), "bad".into());
        assert!(visible(&editor, temperature));
        editor.drafts.remove(&temperature.path);
        editor
            .set("openai_detail.openai_send_temprature", json!(true))
            .unwrap();
        assert!(visible(&editor, temperature));
        let thinking = spec("siliconflow_detail.siliconflow_enable_thinking");
        editor
            .set(
                "siliconflow_detail.siliconflow_send_enable_thinking_param",
                json!(false),
            )
            .unwrap();
        assert!(!visible(&editor, thinking));
        editor
            .set(
                "siliconflow_detail.siliconflow_send_enable_thinking_param",
                json!(true),
            )
            .unwrap();
        assert!(visible(&editor, thinking));
    }
    #[test]
    fn deepseek_thinking_controls_follow_model_and_preserve_values() {
        for prefix in ["", "term_"] {
            let section = format!("{prefix}deepseek_detail");
            let model = format!("{section}.{prefix}deepseek_model");
            let mode = format!("{section}.{prefix}deepseek_thinking_mode");
            let effort = format!("{section}.{prefix}deepseek_reasoning_effort");
            let mut editor = Editor::new(pdf2zh_config::Settings::default());
            editor.set(&model, json!("deepseek-chat")).unwrap();
            editor.set(&mode, json!("enabled")).unwrap();
            editor.set(&effort, json!("high")).unwrap();
            assert!(!visible(&editor, spec(&mode)));
            assert!(!visible(&editor, spec(&effort)));
            editor
                .drafts
                .insert(model.clone(), "deepseek-v4-pro".into());
            assert!(visible(&editor, spec(&mode)));
            assert!(visible(&editor, spec(&effort)));
            editor.set(&mode, json!("disabled")).unwrap();
            assert!(visible(&editor, spec(&mode)));
            assert!(!visible(&editor, spec(&effort)));
            editor.drafts.remove(&model);
            assert!(!visible(&editor, spec(&mode)));
            assert_eq!(editor.settings.get(&mode), Some(&json!("disabled")));
            assert_eq!(editor.settings.get(&effort), Some(&json!("high")));
            editor.drafts.insert(mode.clone(), "invalid".into());
            assert!(visible(&editor, spec(&mode)));
            editor.drafts.insert(effort.clone(), "invalid".into());
            assert!(visible(&editor, spec(&effort)));
        }
    }
    #[test]
    fn painting_unedited_nullable_values_does_not_create_drafts() {
        let mut settings = pdf2zh_config::Settings::default();
        settings
            .set("translation.custom_system_prompt", json!(""))
            .unwrap();
        let mut editor = Editor::new(settings);
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            field(
                ui,
                &mut editor,
                spec("translation.custom_system_prompt"),
                None,
                true,
                &eframe::Frame::_new_kittest(),
            )
            .unwrap();
            field(
                ui,
                &mut editor,
                spec("translation.pool_max_workers"),
                None,
                true,
                &eframe::Frame::_new_kittest(),
            )
            .unwrap();
        });
        output.drop_without_applying_deltas();
        assert!(editor.drafts.is_empty());
        assert!(!editor.dirty);
        assert_eq!(
            editor.settings.get("translation.custom_system_prompt"),
            Some(&json!(""))
        );
    }
    #[test]
    fn url_checks_do_not_include_secret() {
        let error = validate_draft(spec("openai_detail.openai_base_url"), "private token")
            .unwrap_err()
            .to_string();
        assert!(!error.contains("private token"));
        assert!(
            validate_draft(
                spec("openai_detail.openai_base_url"),
                "http://localhost:8080/v1"
            )
            .is_ok()
        );
        assert!(
            validate_draft(
                spec("openai_detail.openai_base_url"),
                "https://user:password@example.com/v1"
            )
            .is_err()
        );
        assert!(
            validate_draft(
                spec("openai_detail.openai_base_url"),
                "https://example.com/v1#fragment"
            )
            .is_err()
        );
    }
}
