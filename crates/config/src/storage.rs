use crate::{ConfigError, ConfigErrorKind, Result, Settings, parse_cli_overrides, parse_env_vars};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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
    fallback_from: Option<PathBuf>,
    default_executable_root: bool,
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
            fallback_from: None,
            default_executable_root: false,
        })
    }
    pub fn for_current_executable() -> Result<Self> {
        let exe = std::env::current_exe().map_err(io_error)?;
        let mut store = Self::new(
            exe.parent()
                .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?
                .into(),
        )?;
        store.default_executable_root = true;
        Ok(store)
    }
    /// All environment access is explicit and injectable. No std::env::vars or
    /// SDK credential discovery occurs here.
    pub fn from_environment(env: &BTreeMap<String, String>) -> Result<Self> {
        let home = supplied_env(env, "USERPROFILE")
            .or_else(|| supplied_env(env, "HOME"))
            .map(PathBuf::from);
        let mut store = if let Some(root) =
            supplied_env(env, "BABELOXIDE_APP_DIR").or_else(|| supplied_env(env, "PDF2ZH_APP_DIR"))
        {
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

    /// Original application root when a permission failure required a fallback.
    pub fn fallback_from(&self) -> Option<&Path> {
        self.fallback_from.as_deref()
    }

    /// Select writable config/cache storage after construction, without creating
    /// a default document. Explicit app directories never silently fall back.
    /// Permission failures in tests are injected; they do not modify Windows
    /// ACLs or demonstrate every read-only filesystem configuration.
    pub fn resolve_writable(&self, env: &BTreeMap<String, String>) -> Result<Self> {
        self.resolve_writable_with(env, probe_directory)
    }

    fn resolve_writable_with(
        &self,
        env: &BTreeMap<String, String>,
        probe: impl Fn(&Path) -> io::Result<()>,
    ) -> Result<Self> {
        let check = |root: &Path| -> io::Result<()> {
            probe(&root.join("config"))?;
            probe(&root.join("cache"))
        };
        match check(&self.app_root) {
            Ok(()) => return Ok(self.clone()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
                ) && self.default_executable_root
                    && supplied_env(env, "BABELOXIDE_APP_DIR").is_none()
                    && supplied_env(env, "PDF2ZH_APP_DIR").is_none() => {}
            Err(error) => return Err(io_error(error)),
        }

        let local = supplied_env(env, "LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                #[cfg(windows)]
                {
                    supplied_env(env, "USERPROFILE")
                        .filter(|value| !value.is_empty())
                        .or_else(|| supplied_env(env, "HOME").filter(|value| !value.is_empty()))
                        .map(|home| PathBuf::from(home).join("AppData").join("Local"))
                }
                #[cfg(target_os = "macos")]
                {
                    supplied_env(env, "HOME")
                        .filter(|value| !value.is_empty())
                        .map(|home| PathBuf::from(home).join("Library/Application Support"))
                }
                #[cfg(not(any(windows, target_os = "macos")))]
                {
                    None
                }
            })
            .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?;
        if !local.is_absolute() {
            return Err(ConfigError::new(ConfigErrorKind::InvalidPath));
        }
        let original = self
            .app_root
            .to_str()
            .ok_or_else(|| ConfigError::new(ConfigErrorKind::InvalidPath))?;
        let fingerprint = root_fingerprint(original);
        let current = local.join("BabelOxide").join(&fingerprint);
        let legacy = local.join("PDFMathTranslate").join(&fingerprint);
        // Keep an existing legacy fallback in place, including SQLite WALs,
        // model edits and cache handles. Copying a live database is unsafe.
        // New installations use the BabelOxide directory; neither path is reset.
        let root = if !current.exists()
            && (legacy.join("config").join(CONFIG_FILENAME).exists()
                || legacy.join("cache").exists())
        {
            legacy
        } else {
            current
        };
        check(&root).map_err(io_error)?;
        let mut fallback = Self::new(root)?;
        fallback.legacy_home = self.legacy_home.clone();
        fallback.fallback_from = Some(self.app_root.clone());
        copy_once(&self.path, &fallback.path)?;
        Ok(fallback)
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
        settings.tencent_environment = env
            .iter()
            .filter(|(key, _)| {
                matches!(
                    key.as_str(),
                    "TENCENTCLOUD_SECRET_ID"
                        | "TENCENTCLOUD_SECRET_KEY"
                        | "TKE_REGION"
                        | "TKE_PROVIDER_ID"
                        | "TKE_WEB_IDENTITY_TOKEN_FILE"
                        | "TKE_ROLE_ARN"
                )
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
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
        let (value, _) = decode_bytes(&bytes)?;
        Settings::from_value(value)?.validate()?;
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

fn root_fingerprint(root: &str) -> String {
    format!("{:x}", Sha256::digest(root.as_bytes()))[..16].to_owned()
}

fn probe_directory(path: &Path) -> io::Result<()> {
    let mut candidate = path;
    let mut missing = false;
    loop {
        match fs::metadata(candidate) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => return Err(io::ErrorKind::NotADirectory.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing = true;
                candidate = candidate.parent().ok_or(io::ErrorKind::NotFound)?;
            }
            Err(error) => return Err(error),
        }
    }
    // For a missing target, verify that its existing ancestor permits creation
    // of both a directory and a file without leaving the target tree behind.
    let temporary_directory = if missing {
        Some(tempfile::tempdir_in(candidate)?)
    } else {
        None
    };
    let probe_path = temporary_directory
        .as_ref()
        .map_or(candidate, |directory| directory.path());
    let mut temporary = NamedTempFile::new_in(probe_path)?;
    temporary.write_all(b"probe")?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    Ok(())
}

fn copy_once(source: &Path, destination: &Path) -> Result<()> {
    if destination.try_exists().map_err(io_error)? {
        return Ok(());
    }
    let bytes = match fs::read(source) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    let (value, _) = decode_bytes(&bytes)?;
    Settings::from_value(value)?.validate()?;
    let temporary = prepare(destination, &bytes)?;
    match temporary.persist_noclobber(destination) {
        Ok(_) => sync_parent(destination),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(io_error(error.error)),
    }
}

fn read(path: &Path) -> Result<Option<(Value, toml::Value)>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    decode_bytes(&bytes).map(Some)
}

fn decode_bytes(bytes: &[u8]) -> Result<(Value, toml::Value)> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| ConfigError::new(ConfigErrorKind::InvalidToml))?;
    let original: toml::Value =
        toml::from_str(text).map_err(|_| ConfigError::new(ConfigErrorKind::InvalidToml))?;
    let value = toml_to_json(&original);
    Ok((decode_toml_content(value), original))
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

#[cfg(test)]
mod writable_tests {
    use super::*;

    fn local_env(path: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([("LOCALAPPDATA".into(), path.to_str().unwrap().into())])
    }

    #[test]
    fn fingerprint_matches_known_sha256_vector() {
        assert_eq!(root_fingerprint(r"C:\PDFMathTranslate"), "4f57006f9d91214a");
    }

    fn default_store(root: PathBuf) -> ConfigStore {
        let mut store = ConfigStore::new(root).unwrap();
        store.default_executable_root = true;
        store
    }

    #[test]
    fn writable_root_is_probed_without_creating_a_default_document() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("portable")).unwrap();
        let resolved = store.resolve_writable(&BTreeMap::new()).unwrap();
        assert_eq!(resolved.app_root(), store.app_root());
        assert!(resolved.fallback_from().is_none());
        assert!(!resolved.app_root().exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn permission_failure_copies_portable_bytes_once_to_stable_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("portable");
        let home = directory.path().join("home");
        let store = default_store(root.clone())
            .with_legacy_home(home.clone())
            .unwrap();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let original = b"# portable fixture\n[future]\nunknown='preserved'\n";
        fs::write(store.path(), original).unwrap();
        let env = local_env(&directory.path().join("local"));
        let probe = |path: &Path| {
            if path == root.join("cache") {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                probe_directory(path)
            }
        };
        let fallback = store.resolve_writable_with(&env, probe).unwrap();
        assert_eq!(fallback.fallback_from(), Some(root.as_path()));
        assert_eq!(fallback.legacy_home.as_deref(), Some(home.as_path()));
        assert_eq!(
            fallback.app_root().parent().unwrap(),
            directory.path().join("local/BabelOxide")
        );
        let suffix = fallback.app_root().file_name().unwrap().to_str().unwrap();
        assert_eq!(suffix.len(), 16);
        assert!(suffix.bytes().all(|value| value.is_ascii_hexdigit()));
        assert_eq!(fs::read(fallback.path()).unwrap(), original);
        assert_eq!(fs::read(store.path()).unwrap(), original);
        assert_eq!(
            fallback.load().unwrap().get("future.unknown"),
            Some(&json!("preserved"))
        );
        let existing = b"# existing fallback\n[future]\nunknown='keep'\n";
        fs::write(fallback.path(), existing).unwrap();
        let second = store.resolve_writable_with(&env, probe).unwrap();
        assert_eq!(second.app_root(), fallback.app_root());
        assert_eq!(fs::read(second.path()).unwrap(), existing);
        assert_eq!(fs::read(store.path()).unwrap(), original);
    }

    #[test]
    fn an_existing_legacy_fallback_keeps_config_cache_and_wal_in_place() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("portable");
        let local = directory.path().join("local");
        let legacy = local
            .join("PDFMathTranslate")
            .join(root_fingerprint(install.to_str().unwrap()));
        fs::create_dir_all(legacy.join("config")).unwrap();
        fs::create_dir_all(legacy.join("cache")).unwrap();
        let original = b"[openai_detail]\nopenai_api_key='synthetic-migration-key'\n";
        fs::write(legacy.join("config").join(CONFIG_FILENAME), original).unwrap();
        fs::write(
            legacy.join("cache/translations.v2.db-wal"),
            b"untouched WAL",
        )
        .unwrap();
        let store = default_store(install.clone());
        let resolved = store
            .resolve_writable_with(&local_env(&local), |path| {
                if path.starts_with(&install) {
                    Err(io::ErrorKind::PermissionDenied.into())
                } else {
                    probe_directory(path)
                }
            })
            .unwrap();
        assert_eq!(resolved.app_root(), legacy);
        assert_eq!(
            resolved.load().unwrap().get("openai_detail.openai_api_key"),
            Some(&json!("synthetic-migration-key"))
        );
        assert_eq!(fs::read(resolved.path()).unwrap(), original);
        assert_eq!(
            fs::read(legacy.join("cache/translations.v2.db-wal")).unwrap(),
            b"untouched WAL"
        );
        assert!(!local.join("BabelOxide").exists());
    }

    #[test]
    fn explicit_root_and_unrelated_errors_never_fall_back() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("portable");
        let store = default_store(root.clone());
        let mut env = local_env(&directory.path().join("local"));
        env.insert("BABELOXIDE_APP_DIR".into(), root.to_str().unwrap().into());
        let error = store
            .resolve_writable_with(&env, |_| Err(io::ErrorKind::PermissionDenied.into()))
            .unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::Io);
        assert!(!directory.path().join("local").exists());
        env.remove("BABELOXIDE_APP_DIR");
        let custom = ConfigStore::new(root.clone()).unwrap();
        let error = custom
            .resolve_writable_with(&env, |_| Err(io::ErrorKind::PermissionDenied.into()))
            .unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::Io);
        let error = store
            .resolve_writable_with(&env, |_| Err(io::ErrorKind::InvalidData.into()))
            .unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::Io);
        assert!(!directory.path().join("local").exists());
    }

    #[test]
    fn existing_file_ancestor_is_not_treated_as_missing() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("not-a-directory");
        fs::write(&file, b"fixture").unwrap();
        let store = ConfigStore::new(file.join("app")).unwrap();
        let error = store.resolve_writable(&BTreeMap::new()).unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::Io);
        assert_eq!(fs::read(file).unwrap(), b"fixture");
    }

    #[test]
    fn corrupt_config_directory_error_does_not_trigger_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("portable");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("config"), b"not a directory").unwrap();
        let store = default_store(root);
        let env = local_env(&directory.path().join("local"));
        assert_eq!(
            store.resolve_writable(&env).unwrap_err().kind,
            ConfigErrorKind::Io
        );
        assert!(!directory.path().join("local").exists());
    }

    #[test]
    fn fallback_without_source_does_not_create_a_default_document() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("portable");
        let store = default_store(root.clone());
        let env = local_env(&directory.path().join("local"));
        let fallback = store
            .resolve_writable_with(&env, |path| {
                if path.starts_with(&root) {
                    Err(io::ErrorKind::ReadOnlyFilesystem.into())
                } else {
                    probe_directory(path)
                }
            })
            .unwrap();
        assert!(!fallback.app_root().exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn missing_localappdata_uses_macos_application_support() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("portable");
        let home = directory.path().join("home");
        let store = default_store(root.clone());
        let env = BTreeMap::from([("HOME".into(), home.to_str().unwrap().into())]);
        let fallback = store
            .resolve_writable_with(&env, |path| {
                if path.starts_with(&root) {
                    Err(io::ErrorKind::PermissionDenied.into())
                } else {
                    probe_directory(path)
                }
            })
            .unwrap();
        assert!(
            fallback
                .app_root()
                .starts_with(home.join("Library/Application Support/BabelOxide"))
        );
        assert!(!fallback.path().exists());
    }

    #[cfg(windows)]
    #[test]
    fn missing_localappdata_uses_explicit_userprofile() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("portable");
        let home = directory.path().join("home");
        let store = default_store(root.clone());
        let env = BTreeMap::from([("USERPROFILE".into(), home.to_str().unwrap().into())]);
        let fallback = store
            .resolve_writable_with(&env, |path| {
                if path.starts_with(&root) {
                    Err(io::ErrorKind::PermissionDenied.into())
                } else {
                    probe_directory(path)
                }
            })
            .unwrap();
        assert!(
            fallback
                .app_root()
                .starts_with(home.join("AppData/Local/BabelOxide"))
        );
    }
}
