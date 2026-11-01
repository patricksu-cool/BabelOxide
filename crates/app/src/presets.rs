//! Credential-free workflow presets, persisted by the existing atomic config store.
use anyhow::{Result, bail, ensure};
use babeloxide_config::{Settings, field_specs};
use serde_json::{Value, json};
use std::collections::BTreeMap;

// Keep this extension as a JSON string: legacy TOML intentionally treats nulls
// in arrays differently, and merges arrays when preserving unknown settings.
const PATH: &str = "babeloxide.presets_json";
const FIELDS: &[&str] = &[
    "translation.lang_in",
    "translation.lang_out",
    "translation.no_auto_extract_glossary",
    "pdf.no_mono",
    "pdf.no_dual",
    "pdf.use_alternating_pages_dual",
    "pdf.dual_translate_first",
    "pdf.watermark_output_mode",
    "pdf.primary_font_family",
    "pdf.pages",
];

#[derive(Clone)]
pub(crate) struct Preset {
    pub name: String,
    pub values: BTreeMap<String, Value>,
}

fn allowed(path: &str) -> bool {
    FIELDS.contains(&path)
        || field_specs().iter().any(|spec| {
            spec.path == path
                && !spec.secret
                && path.contains("_detail.")
                && path
                    .rsplit('.')
                    .next()
                    .is_some_and(|leaf| leaf == "model" || leaf.ends_with("_model"))
        })
}

fn check_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty()
            && name.chars().count() <= 64
            && !name.chars().any(char::is_control),
        "請使用 1–64 字且不含控制字元的組合名稱"
    );
    ensure!(
        !matches!(name, "論文雙語" | "快速單語"),
        "請為自訂組合使用其他名稱"
    );
    Ok(())
}

pub(crate) fn saved(settings: &Settings) -> Result<Vec<Preset>> {
    let Some(value) = settings.get(PATH) else {
        return Ok(Vec::new());
    };
    let value: Value = serde_json::from_str(
        value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("預設組合格式無效"))?,
    )?;
    let rows = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("預設組合格式無效"))?;
    ensure!(rows.len() <= 100, "預設組合數量超過 100");
    let mut presets: Vec<Preset> = Vec::new();
    for row in rows {
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("預設組合缺少名稱"))?;
        check_name(name)?;
        ensure!(
            !presets
                .iter()
                .any(|preset| preset.name.eq_ignore_ascii_case(name)),
            "預設組合名稱重複"
        );
        let values: BTreeMap<String, Value> = serde_json::from_value(
            row.get("values")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("預設組合缺少內容"))?,
        )?;
        ensure!(
            values.keys().all(|path| allowed(path) || path == "engine"),
            "預設組合包含不支援的欄位；設定未套用"
        );
        presets.push(Preset {
            name: name.into(),
            values,
        });
    }
    Ok(presets)
}

pub(crate) fn capture(settings: &Settings, name: &str) -> Result<Preset> {
    check_name(name)?;
    let mut values = BTreeMap::new();
    let detail = settings
        .main_engine()
        .detail_key
        .as_deref()
        .map(|key| format!("{key}."));
    for spec in field_specs().iter().filter(|spec| {
        allowed(&spec.path)
            && (!spec.path.contains("_detail.")
                || detail
                    .as_ref()
                    .is_some_and(|prefix| spec.path.starts_with(prefix)))
    }) {
        if let Some(value) = settings.get(&spec.path) {
            values.insert(spec.path.clone(), value.clone());
        }
    }
    values.insert("engine".into(), json!(settings.main_engine().flag));
    Ok(Preset {
        name: name.into(),
        values,
    })
}

pub(crate) fn apply(settings: &Settings, preset: &Preset) -> Result<Settings> {
    let mut candidate = settings.clone();
    for (path, value) in &preset.values {
        if path == "engine" {
            let flag = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("預設組合的服務無效"))?;
            ensure!(
                babeloxide_config::engine_specs()
                    .iter()
                    .any(|engine| engine.flag == flag),
                "預設組合的服務無效"
            );
            candidate.set(flag, json!(true))?;
        } else if allowed(path) {
            candidate.set(path, value.clone())?;
        } else {
            bail!("預設組合包含不支援的欄位；設定未套用");
        }
    }
    candidate.validate()?;
    Ok(candidate)
}

pub(crate) fn builtins() -> Vec<Preset> {
    [("論文雙語", false, false), ("快速單語", true, true)]
        .into_iter()
        .map(|(name, no_dual, no_glossary)| Preset {
            name: name.into(),
            values: BTreeMap::from([
                ("pdf.no_mono".into(), json!(false)),
                ("pdf.no_dual".into(), json!(no_dual)),
                (
                    "translation.no_auto_extract_glossary".into(),
                    json!(no_glossary),
                ),
            ]),
        })
        .collect()
}

pub(crate) fn store(settings: &mut Settings, presets: &[Preset]) -> Result<()> {
    let value = json!(
        presets
            .iter()
            .map(|preset| json!({"name":preset.name,"values":preset.values}))
            .collect::<Vec<_>>()
    );
    let mut candidate = settings.clone();
    candidate.set(PATH, json!(serde_json::to_string(&value)?))?;
    saved(&candidate)?;
    *settings = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_round_trip_keeps_credentials_limits_and_provider_profiles() -> Result<()> {
        let mut settings = Settings::default();
        settings.set("openai", json!(true))?;
        settings.set("openai_detail.openai_api_key", json!("synthetic-secret"))?;
        settings.set("translation.qps", json!(7))?;
        let preset = capture(&settings, "我的論文")?;
        assert!(!serde_json::to_string(&preset.values)?.contains("synthetic-secret"));
        store(&mut settings, &[preset])?;
        let directory = tempfile::tempdir()?;
        let config = babeloxide_config::ConfigStore::new(directory.path().to_path_buf())?;
        config.save_user_defaults(&settings)?;
        let decoded = config.load()?;
        let presets = saved(&decoded)?;
        let next = apply(&decoded, &presets[0])?;
        assert_eq!(
            next.get("openai_detail.openai_api_key"),
            Some(&json!("synthetic-secret"))
        );
        assert_eq!(next.get("translation.qps"), Some(&json!(7)));
        assert_eq!(next.main_engine().flag, "openai");
        let mut renamed = next;
        let mut presets = saved(&renamed)?;
        presets[0].name = "重新命名".into();
        store(&mut renamed, &presets)?;
        config.save_user_defaults(&renamed)?;
        let reloaded = config.load()?;
        assert_eq!(saved(&reloaded)?.len(), 1);
        assert_eq!(saved(&reloaded)?[0].name, "重新命名");
        Ok(())
    }

    #[test]
    fn untrusted_presets_cannot_write_keys_or_half_apply() -> Result<()> {
        let settings = Settings::default();
        let preset = Preset {
            name: "bad".into(),
            values: BTreeMap::from([
                ("pdf.no_dual".into(), json!(true)),
                ("openai_detail.openai_api_key".into(), json!("untrusted")),
            ]),
        };
        assert!(apply(&settings, &preset).is_err());
        assert_eq!(settings.get("pdf.no_dual"), Some(&json!(false)));
        Ok(())
    }
}
