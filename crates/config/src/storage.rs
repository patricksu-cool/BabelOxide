use crate::{ConfigError, ConfigErrorKind, Result, Settings, parse_cli_overrides, parse_env_vars};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use tempfile::NamedTempFile;

pub const CONFIG_FILENAME: &str = "config.v3.toml";

/// Paths are application-owned and absolute. Construction performs no I/O.
#[derive(Clone)]
pub struct ConfigStore {
    app_root: PathBuf,
    path: PathBuf,
    legacy_home: Option<PathBuf>,
}

impl fmt::Debug for ConfigStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfigStore { [REDACTED PATHS] }")
    }
}

impl ConfigStore {
    pub fn new(app_root: PathBuf) -> Result<Self> {
        if !app_root.is_absolute() {
            return Err(ConfigError::new(ConfigErrorKind::InvalidPath));
        }
        let path = app_root.join("config").join(CONFIG_FILENAME);
        Ok(Self {
            app_root,
            path,
            legacy_home: None,
        })
    }
    pub fn for_current_executable() -> Result<Self> {
        let exe = std::env::current_exe().map_err(io_error)?;
        Self::new(
            exe.parent()
                .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?
                .into(),
        )
    }
    /// All environment access is explicit and injectable. No std::env::vars or
    /// SDK credential discovery occurs here.
    pub fn from_environment(env: &BTreeMap<String, String>) -> Result<Self> {
        let home = supplied_env(env, "USERPROFILE")
            .or_else(|| supplied_env(env, "HOME"))
            .map(PathBuf::from);
        let mut store = if let Some(root) = supplied_env(env, "PDF2ZH_APP_DIR") {
            let root = if root == "~" {
                home.clone()
                    .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?
            } else if let Some(rest) = root.strip_prefix("~/").or_else(|| root.strip_prefix("~\\"))
            {
                home.clone()
                    .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?
                    .join(rest)
            } else {
                PathBuf::from(root)
            };
            Self::new(root)?
        } else {
            Self::for_current_executable()?
        };
        if let Some(home) = home {
            store = store.with_legacy_home(home)?;
        }
        Ok(store)
    }
    pub fn with_legacy_home(mut self, home: PathBuf) -> Result<Self> {
        if !home.is_absolute() {
            return Err(ConfigError::new(ConfigErrorKind::InvalidPath));
        }
        self.legacy_home = Some(home);
        Ok(self)
    }
    pub fn app_root(&self) -> &Path {
        &self.app_root
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads defaults and the application file only. Never writes a default
    /// document over an existing file, including malformed or future formats.
    pub fn load(&self) -> Result<Settings> {
        self.load_with_overrides(&BTreeMap::new(), &json!({}))
    }

    pub fn load_with_overrides(
        &self,
        env: &BTreeMap<String, String>,
        cli: &Value,
    ) -> Result<Settings> {
        if let Some(home) = &self.legacy_home {
            self.migrate_legacy_once(home)?;
        }
        let cli = parse_cli_overrides(cli)?;
        let env_overrides = parse_env_vars(env);
        let default = read(&self.path)?;
        let explicit_path = cli
            .get("config_file")
            .or_else(|| env_overrides.get("config_file"))
            .and_then(Value::as_str);
        let explicit = explicit_path
            .map(Path::new)
            .map(read)
            .transpose()?
            .flatten();
        let layers = vec![
            cli,
            env_overrides,
            explicit
                .as_ref()
                .map(|v| v.0.clone())
                .unwrap_or_else(|| json!({})),
            default
                .as_ref()
                .map(|v| v.0.clone())
                .unwrap_or_else(|| json!({})),
        ];
        let mut settings = Settings::from_layers(&layers)?;
        let mut original = default.map(|v| v.1);
        if let Some((_, higher)) = explicit {
            if let Some(lower) = &mut original {
                merge_original(lower, &higher);
            } else {
                original = Some(higher);
            }
        }
        settings.original_toml = original;
        apply_sdk_url_fallbacks(&mut settings, env);
        Ok(settings)
    }

    /// Byte-preserving, copy-once migration. A racing writer/other migrator wins
    /// atomically: persist_noclobber never replaces its complete destination.
    pub fn migrate_legacy_once(&self, home: &Path) -> Result<bool> {
        if !home.is_absolute() {
            return Err(ConfigError::new(ConfigErrorKind::InvalidPath));
        }
        if self.path.try_exists().map_err(io_error)? {
            return Ok(false);
        }
        let old = home.join(".config").join("pdf2zh").join(CONFIG_FILENAME);
        match fs::metadata(&old) {
            Ok(meta) if meta.is_file() => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error(error)),
        }
        let bytes = fs::read(old).map_err(io_error)?;
        let temporary = prepare(&self.path, &bytes)?;
        match temporary.persist_noclobber(&self.path) {
            Ok(_) => {
                sync_parent(&self.path)?;
                Ok(true)
            }
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(io_error(error.error)),
        }
    }

    /// Explicit full-document persistence, including unknown fields. The caller
    /// is not mutated. Loading a file never calls this method automatically.
    pub fn save(&self, settings: &Settings) -> Result<()> {
        let encoded = encode_toml_content(settings.as_value().clone());
        let mut toml = toml::Value::try_from(encoded)
            .map_err(|_| ConfigError::new(ConfigErrorKind::Serialize))?;
        if let Some(original) = &settings.original_toml {
            restore_datetime(&mut toml, original);
        }
        let bytes = toml::to_string_pretty(&toml)
            .map_err(|_| ConfigError::new(ConfigErrorKind::Serialize))?
            .into_bytes();
        // Avoid unnecessary replacement, and preserve the original spelling and
        // comments when the effective document has not changed.
        if let Some((current, original)) = read(&self.path)?
            && current == *settings.as_value()
            && toml == original
        {
            return Ok(());
        }
        replace_atomic(&self.path, &bytes)
    }

    /// Python write_user_default_config_file clears transient input files.
    /// Keep this separate from save so ordinary round trips remain lossless.
    pub fn save_user_defaults(&self, settings: &Settings) -> Result<()> {
        let mut saved = settings.clone();
        saved.set("basic.input_files", json!([]))?;
        self.save(&saved)
    }
}

fn apply_sdk_url_fallbacks(settings: &mut Settings, env: &BTreeMap<String, String>) {
    for term in [false, true] {
        let engine = if term {
            settings.selected_engine(true)
        } else {
            Some(settings.main_engine())
        };
        let Some(engine) = engine else {
            continue;
        };
        let (field, variable) = match engine.name.as_str() {
            "OpenAI" => ("openai_base_url", "OPENAI_BASE_URL"),
            "SiliconFlow" => ("siliconflow_base_url", "OPENAI_BASE_URL"),
            "QwenMt" => ("qwenmt_base_url", "OPENAI_BASE_URL"),
            "AzureOpenAI" => ("azure_openai_base_url", "AZURE_OPENAI_ENDPOINT"),
            _ => continue,
        };
        let detail = engine
            .detail_key
            .as_ref()
            .expect("URL-bearing engines have detail tables");
        let path = if term {
            format!("term_{detail}.term_{field}")
        } else {
            format!("{detail}.{field}")
        };
        if settings.get(&path).is_some_and(Value::is_null)
            && let Some(value) = supplied_env(env, variable)
        {
            let scope = if term {
                format!("term_{}", engine.flag)
            } else {
                engine.flag.clone()
            };
            settings
                .sdk_url_fallbacks
                .insert(scope, (field.into(), value.clone()));
        }
    }
}

fn supplied_env<'a>(env: &'a BTreeMap<String, String>, name: &str) -> Option<&'a String> {
    env.get(name).or_else(|| {
        env.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}

fn io_error(_: io::Error) -> ConfigError {
    ConfigError::new(ConfigErrorKind::Io)
}

fn read(path: &Path) -> Result<Option<(Value, toml::Value)>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    let text =
        std::str::from_utf8(&bytes).map_err(|_| ConfigError::new(ConfigErrorKind::InvalidToml))?;
    let original: toml::Value =
        toml::from_str(text).map_err(|_| ConfigError::new(ConfigErrorKind::InvalidToml))?;
    let value = toml_to_json(&original);
    Ok(Some((decode_toml_content(value), original)))
}

/// The old format converts dictionary values only. A string "null" inside an
/// array remains a string; this deliberate distinction matches storage.py.
pub fn decode_toml_content(mut value: Value) -> Value {
    if let Value::Object(fields) = &mut value {
        for child in fields.values_mut() {
            if child.as_str() == Some("null") {
                *child = Value::Null;
            } else if child.is_object() {
                *child = decode_toml_content(std::mem::take(child));
            }
        }
    }
    value
}
pub fn encode_toml_content(mut value: Value) -> Value {
    if let Value::Object(fields) = &mut value {
        for child in fields.values_mut() {
            if child.is_null() {
                *child = json!("null");
            } else if child.is_object() {
                *child = encode_toml_content(std::mem::take(child));
            }
        }
    }
    value
}

fn prepare(path: &Path, bytes: &[u8]) -> Result<NamedTempFile> {
    let directory = path
        .parent()
        .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?;
    fs::create_dir_all(directory).map_err(io_error)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".config.v3.")
        .suffix(".temp.toml")
        .tempfile_in(directory)
        .map_err(io_error)?;
    temporary.write_all(bytes).map_err(io_error)?;
    temporary.flush().map_err(io_error)?;
    temporary.as_file().sync_all().map_err(io_error)?;
    Ok(temporary)
}
fn replace_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary = prepare(path, bytes)?;
    for attempt in 0..5 {
        match temporary.persist(path) {
            Ok(_) => return sync_parent(path),
            Err(error) => {
                let retry = cfg!(windows)
                    && matches!(error.error.raw_os_error(), Some(5 | 32 | 33))
                    && attempt < 4;
                if !retry {
                    return Err(io_error(error.error));
                }
                temporary = error.file;
                thread::sleep(Duration::from_millis(10 * (1 << attempt)));
            }
        }
    }
    unreachable!("final persistence attempt always returns")
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(
        path.parent()
            .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?,
    )
    .map_err(io_error)?
    .sync_all()
    .map_err(io_error)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn merge_original(target: &mut toml::Value, source: &toml::Value) {
    match (target, source) {
        (toml::Value::Table(target), toml::Value::Table(source)) => {
            for (key, value) in source {
                if let Some(old) = target.get_mut(key) {
                    merge_original(old, value);
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        (toml::Value::Array(target), toml::Value::Array(source)) => {
            target.extend(source.iter().cloned())
        }
        (target, source) => *target = source.clone(),
    }
}
fn toml_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(value) => json!(value),
        toml::Value::Integer(value) => json!(value),
        toml::Value::Float(value) if value.is_finite() => json!(value),
        toml::Value::Float(value) => json!(value.to_string()),
        toml::Value::Boolean(value) => json!(value),
        toml::Value::Datetime(value) => json!(value.to_string()),
        toml::Value::Array(values) => Value::Array(values.iter().map(toml_to_json).collect()),
        toml::Value::Table(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), toml_to_json(value)))
                .collect(),
        ),
    }
}
fn restore_datetime(target: &mut toml::Value, original: &toml::Value) {
    match (target, original) {
        (target @ toml::Value::String(_), toml::Value::Datetime(date)) => {
            if target.as_str() == Some(date.to_string().as_str()) {
                *target = original.clone();
            }
        }
        (target @ toml::Value::String(_), toml::Value::Float(value)) if !value.is_finite() => {
            if target.as_str() == Some(value.to_string().as_str()) {
                *target = original.clone();
            }
        }
        (toml::Value::Table(target), toml::Value::Table(original)) => {
            for (key, value) in target {
                if let Some(old) = original.get(key) {
                    restore_datetime(value, old);
                }
            }
        }
        (toml::Value::Array(target), toml::Value::Array(original)) => {
            for (value, old) in target.iter_mut().zip(original) {
                restore_datetime(value, old);
            }
        }
        _ => {}
    }
}
