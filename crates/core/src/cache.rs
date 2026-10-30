//! Optional persistent translation cache, opened only at an explicit path.
//!
//! Python's `_translationcache` table remains untouched and can be read using
//! its recursively sorted, ASCII JSON namespace. Those rows lack endpoint,
//! method and role information, so they are never implicitly reused by a Rust
//! service. `LegacyBinding` is an explicit caller assertion of that provenance.
//! New rows use a distinct table and a digest of the complete Rust namespace.
//!
//! Store operations expose safe typed errors. TranslationService treats cache
//! failures as best effort, matching Python's get wrapper and TranslationCache
//! .set exception handler. Cancellation remains a separate control event.

use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

use crate::{TranslationRequest, TranslatorConfig};

pub type CacheResult<T> = Result<T, CacheError>;
pub type CacheFuture<'a, T> = Pin<Box<dyn Future<Output = CacheResult<T>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheErrorKind {
    Cancelled,
    Busy,
    ReadOnly,
    Corrupt,
    Unavailable,
    Database,
    InvalidNamespace,
    Runtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheError {
    pub kind: CacheErrorKind,
}
impl CacheError {
    pub fn new(kind: CacheErrorKind) -> Self {
        Self { kind }
    }
    pub fn cancelled() -> Self {
        Self::new(CacheErrorKind::Cancelled)
    }
    pub fn is_cancelled(&self) -> bool {
        self.kind == CacheErrorKind::Cancelled
    }
}
impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            CacheErrorKind::Cancelled => "cache operation cancelled",
            CacheErrorKind::Busy => "translation cache is busy",
            CacheErrorKind::ReadOnly => "translation cache is read only",
            CacheErrorKind::Corrupt => "translation cache is not a valid SQLite database",
            CacheErrorKind::Unavailable => "translation cache is unavailable",
            CacheErrorKind::Database => "translation cache database operation failed",
            CacheErrorKind::InvalidNamespace => "legacy translation cache namespace is invalid",
            CacheErrorKind::Runtime => "translation cache worker failed",
        })
    }
}
impl std::error::Error for CacheError {}

fn database_error(error: rusqlite::Error) -> CacheError {
    let kind = match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => CacheErrorKind::Busy,
        Some(ErrorCode::ReadOnly) => CacheErrorKind::ReadOnly,
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => CacheErrorKind::Corrupt,
        Some(ErrorCode::CannotOpen | ErrorCode::PermissionDenied) => CacheErrorKind::Unavailable,
        Some(ErrorCode::OperationInterrupted) => CacheErrorKind::Cancelled,
        _ => CacheErrorKind::Database,
    };
    CacheError::new(kind)
}

/// Independent injection point. Implementations must honor cancellation and
/// complete cleanup before returning; callers do not abort in-progress writes.
pub trait CacheBackend: Send + Sync {
    fn get<'a>(
        &'a self,
        key: &'a CacheKey,
        cancel: &'a CancellationToken,
    ) -> CacheFuture<'a, Option<String>>;
    fn set<'a>(
        &'a self,
        key: &'a CacheKey,
        translation: &'a str,
        cancel: &'a CancellationToken,
    ) -> CacheFuture<'a, ()>;
    fn get_legacy<'a>(
        &'a self,
        _namespace: &'a LegacyNamespace,
        _text: &'a str,
        cancel: &'a CancellationToken,
    ) -> CacheFuture<'a, Option<String>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                Err(CacheError::cancelled())
            } else {
                Ok(None)
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheMethod {
    Translate,
    Llm,
}
impl CacheMethod {
    pub fn for_request(request: &TranslationRequest) -> Option<Self> {
        match request.method.as_str() {
            "translate" | "do_translate" => Some(Self::Translate),
            "llm_translate" | "do_llm_translate" => Some(Self::Llm),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Translate => "translate",
            Self::Llm => "llm_translate",
        }
    }
}

/// The original text is intentionally absent from Debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct CacheKey {
    namespace: String,
    original_text: String,
}
impl fmt::Debug for CacheKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CacheKey")
            .field("namespace", &self.namespace)
            .field("original_text", &"[REDACTED]")
            .finish()
    }
}
impl CacheKey {
    pub fn from_request(config: &TranslatorConfig, request: &TranslationRequest) -> Self {
        let method = CacheMethod::for_request(request)
            .map(CacheMethod::as_str)
            .unwrap_or(&request.method);
        let identity = json!({
            "schema": "pdf2zh-rust-cache-v2",
            "provider_contract": 1,
            "provider": config.provider,
            "endpoint": config.endpoint,
            "model": config.model,
            "lang_in": config.options.get("lang_in").and_then(Value::as_str).unwrap_or("en"),
            "lang_out": config.lang_out,
            "method": method,
            "role": request.translator,
            "prompt_template": crate::legacy_prompt("", &config.lang_out),
            "options": output_options(&config.options, true),
            "request_options": output_options(&request.rate_limit_params, false),
        });
        let digest = Sha256::digest(python_json(&identity).as_bytes());
        let namespace = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        Self {
            namespace,
            original_text: request.text.clone(),
        }
    }
    pub fn namespace_digest(&self) -> &str {
        &self.namespace
    }
}

fn output_options(value: &Value, top_level: bool) -> Value {
    match value {
        Value::Object(object) => {
            let mut result = serde_json::Map::new();
            for (key, value) in object {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                let credential = matches!(
                    normalized.as_str(),
                    "api_key"
                        | "apikey"
                        | "authorization"
                        | "proxy_authorization"
                        | "password"
                        | "credential"
                        | "secret"
                        | "secret_id"
                        | "secret_key"
                        | "access_token"
                        | "refresh_token"
                        | "auth_token"
                        | "api_token"
                        | "bearer_token"
                        | "tencentcloud_token"
                        | "tencentcloud_secret_id"
                        | "tencentcloud_secret_key"
                ) || normalized.ends_with("_api_key")
                    || normalized.ends_with("_apikey")
                    || normalized.ends_with("_password")
                    || normalized.ends_with("_secret_key");
                let operational = top_level
                    && matches!(
                        normalized.as_str(),
                        "qps"
                            | "max_concurrency"
                            | "ignore_cache"
                            | "timeout_explicit"
                            | "timeout_seconds"
                            | "clitranslator_timeout"
                            | "cache_path"
                            | "cache_enabled"
                            | "cache_namespace"
                    );
                if !credential && !operational {
                    result.insert(key.clone(), output_options(value, false));
                }
            }
            Value::Object(result)
        }
        Value::Array(array) => Value::Array(
            array
                .iter()
                .map(|value| output_options(value, false))
                .collect(),
        ),
        _ => value.clone(),
    }
}

#[derive(Clone)]
pub struct LegacyNamespace {
    engine: String,
    params_json: String,
}
impl fmt::Debug for LegacyNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacyNamespace")
            .field("engine", &self.engine)
            .field("params", &"[REDACTED]")
            .finish()
    }
}
impl LegacyNamespace {
    pub fn new(engine: &str, params: &Value) -> CacheResult<Self> {
        // This is Python's <20 assertion, not SQLite's unenforced VARCHAR size.
        if engine.chars().count() >= 20 {
            return Err(CacheError::new(CacheErrorKind::InvalidNamespace));
        }
        Ok(Self {
            engine: engine.into(),
            params_json: if params.is_null() {
                "{}".into()
            } else {
                python_json(params)
            },
        })
    }
}

/// Explicitly scopes a legacy namespace to one method and role in the service
/// receiving this binding. The caller must verify the old row's endpoint and
/// provider configuration: that provenance is absent from the Python database.
#[derive(Clone, Debug)]
pub struct LegacyBinding {
    pub namespace: LegacyNamespace,
    method: CacheMethod,
    role: String,
}
impl LegacyBinding {
    pub fn new(namespace: LegacyNamespace, method: CacheMethod, role: impl Into<String>) -> Self {
        Self {
            namespace,
            method,
            role: role.into(),
        }
    }
    pub(crate) fn matches(&self, request: &TranslationRequest) -> bool {
        CacheMethod::for_request(request) == Some(self.method) && request.translator == self.role
    }
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rust_translation_cache_v2 (
    namespace TEXT NOT NULL,
    original_text TEXT NOT NULL,
    translation TEXT NOT NULL,
    PRIMARY KEY (namespace, original_text)
) WITHOUT ROWID";

struct CacheInner {
    connection: Mutex<Connection>,
    interrupt: rusqlite::InterruptHandle,
    gate: tokio::sync::Mutex<()>,
    legacy_present: bool,
}

#[derive(Clone)]
pub struct CacheStore {
    inner: Arc<CacheInner>,
}
impl fmt::Debug for CacheStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CacheStore")
            .field("legacy_present", &self.legacy_present())
            .finish_non_exhaustive()
    }
}
impl CacheStore {
    pub fn open(path: impl AsRef<Path>) -> CacheResult<Self> {
        Self::open_inner(path.as_ref(), false)
    }
    pub fn open_read_only(path: impl AsRef<Path>) -> CacheResult<Self> {
        Self::open_inner(path.as_ref(), true)
    }
    fn open_inner(path: &Path, read_only: bool) -> CacheResult<Self> {
        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        };
        let connection = Connection::open_with_flags(path, flags).map_err(database_error)?;
        // SQLite's busy timeout blocks its calling thread. Async operations
        // instead retry Busy for up to Python's one-second budget, with
        // cancellable waits and no detached background writer.
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(database_error)?;
        if !read_only {
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .map_err(database_error)?;
            connection.execute_batch(SCHEMA).map_err(database_error)?;
        }
        let legacy_present = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='_translationcache')", [], |row| row.get(0),
        ).map_err(database_error)?;
        let interrupt = connection.get_interrupt_handle();
        Ok(Self {
            inner: Arc::new(CacheInner {
                connection: Mutex::new(connection),
                interrupt,
                gate: tokio::sync::Mutex::new(()),
                legacy_present,
            }),
        })
    }
    pub fn legacy_present(&self) -> bool {
        self.inner.legacy_present
    }

    async fn run<T, F>(&self, cancel: &CancellationToken, operation: F) -> CacheResult<T>
    where
        T: Send + 'static,
        F: Fn(&mut Connection, &CancellationToken) -> CacheResult<T> + Send + Sync + 'static,
    {
        let _gate = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(CacheError::cancelled()),
            gate = self.inner.gate.lock() => gate,
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        let operation = Arc::new(operation);
        loop {
            if cancel.is_cancelled() {
                return Err(CacheError::cancelled());
            }
            let inner = self.inner.clone();
            let operation = operation.clone();
            let task_cancel = cancel.clone();
            let mut task = tokio::task::spawn_blocking(move || {
                let mut connection = inner
                    .connection
                    .lock()
                    .map_err(|_| CacheError::new(CacheErrorKind::Runtime))?;
                if task_cancel.is_cancelled() {
                    return Err(CacheError::cancelled());
                }
                operation(&mut connection, &task_cancel)
            });
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    self.inner.interrupt.interrupt();
                    // A write owns its transaction until commit/rollback. Do
                    // not expose Cancelled while a worker could still write.
                    let _ = task.await;
                    return Err(CacheError::cancelled());
                }
                result = &mut task => result.map_err(|_| CacheError::new(CacheErrorKind::Runtime))?,
            };
            if !matches!(&result, Err(error) if error.kind == CacheErrorKind::Busy)
                || tokio::time::Instant::now() >= deadline
            {
                return result;
            }
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(CacheError::cancelled()),
                _ = tokio::time::sleep(Duration::from_millis(5).min(deadline.saturating_duration_since(tokio::time::Instant::now()))) => {},
            }
        }
    }

    pub async fn get(
        &self,
        key: &CacheKey,
        cancel: &CancellationToken,
    ) -> CacheResult<Option<String>> {
        let key = key.clone();
        self.run(cancel, move |connection,_| connection.query_row(
            "SELECT translation FROM rust_translation_cache_v2 WHERE namespace=?1 AND original_text=?2",
            params![key.namespace, key.original_text], |row| row.get(0),
        ).optional().map_err(database_error)).await
    }
    pub async fn set(
        &self,
        key: &CacheKey,
        translation: &str,
        cancel: &CancellationToken,
    ) -> CacheResult<()> {
        let key = key.clone();
        let translation = translation.to_owned();
        self.run(cancel, move |connection,cancel| {
            let transaction = connection.transaction().map_err(database_error)?;
            transaction.execute(
                "INSERT INTO rust_translation_cache_v2 (namespace, original_text, translation) VALUES (?1,?2,?3)
                 ON CONFLICT(namespace,original_text) DO UPDATE SET translation=excluded.translation",
                params![key.namespace,key.original_text,translation],
            ).map_err(database_error)?;
            if cancel.is_cancelled() { return Err(CacheError::cancelled()); }
            transaction.commit().map_err(database_error)
        }).await
    }
    pub async fn read_legacy(
        &self,
        namespace: &LegacyNamespace,
        text: &str,
        cancel: &CancellationToken,
    ) -> CacheResult<Option<String>> {
        if cancel.is_cancelled() {
            return Err(CacheError::cancelled());
        }
        if !self.legacy_present() {
            return Ok(None);
        }
        let namespace = namespace.clone();
        let text = text.to_owned();
        self.run(cancel, move |connection,_| connection.query_row(
            "SELECT translation FROM _translationcache WHERE translate_engine=?1 AND translate_engine_params=?2 AND original_text=?3",
            params![namespace.engine,namespace.params_json,text], |row| row.get(0),
        ).optional().map_err(database_error)).await
    }
}
impl CacheBackend for CacheStore {
    fn get<'a>(
        &'a self,
        key: &'a CacheKey,
        cancel: &'a CancellationToken,
    ) -> CacheFuture<'a, Option<String>> {
        Box::pin(CacheStore::get(self, key, cancel))
    }
    fn set<'a>(
        &'a self,
        key: &'a CacheKey,
        translation: &'a str,
        cancel: &'a CancellationToken,
    ) -> CacheFuture<'a, ()> {
        Box::pin(CacheStore::set(self, key, translation, cancel))
    }
    fn get_legacy<'a>(
        &'a self,
        namespace: &'a LegacyNamespace,
        text: &'a str,
        cancel: &'a CancellationToken,
    ) -> CacheFuture<'a, Option<String>> {
        Box::pin(self.read_legacy(namespace, text, cancel))
    }
}

/// Python json.dumps defaults: sorted object keys recursively, ensure_ascii,
/// and spaces after ':' and ','. Array order and numeric types are retained.
pub fn python_json(value: &Value) -> String {
    let mut result = String::new();
    append_json(value, &mut result);
    result
}
fn append_json(value: &Value, output: &mut String) {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => {
            if number.is_f64() {
                output.push_str(&python_float(number.as_f64().expect("finite JSON float")));
            } else {
                output.push_str(&number.to_string());
            }
        }
        Value::String(value) => append_string(value, output),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                append_json(value, output);
            }
            output.push(']');
        }
        Value::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
            output.push('{');
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                append_string(key, output);
                output.push_str(": ");
                append_json(value, output);
            }
            output.push('}');
        }
    }
}
fn append_string(value: &str, output: &mut String) {
    use std::fmt::Write;
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            character if (' '..='~').contains(&character) => output.push(character),
            character => {
                let mut utf16 = [0; 2];
                for unit in character.encode_utf16(&mut utf16) {
                    write!(output, "\\u{unit:04x}").expect("write to String");
                }
            }
        }
    }
    output.push('"');
}
fn python_float(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into();
    }
    let raw = value.abs().to_string();
    let (mantissa, explicit_exponent) = raw
        .split_once(['e', 'E'])
        .map(|(mantissa, exponent)| {
            (
                mantissa,
                exponent.parse::<i32>().expect("Rust float exponent"),
            )
        })
        .unwrap_or((&raw, 0));
    let decimal = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits: String = mantissa
        .chars()
        .filter(|character| *character != '.')
        .collect();
    let leading = digits.bytes().take_while(|byte| *byte == b'0').count();
    let exponent = explicit_exponent + decimal - leading as i32 - 1;
    let digits = digits[leading..].trim_end_matches('0');
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if !(-4..16).contains(&exponent) {
        let tail = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!(
            "{sign}{}{tail}e{}{:02}",
            &digits[..1],
            if exponent < 0 { "-" } else { "+" },
            exponent.abs()
        )
    } else if exponent < 0 {
        format!("{sign}0.{}{}", "0".repeat((-exponent - 1) as usize), digits)
    } else {
        let decimal = exponent as usize + 1;
        if digits.len() <= decimal {
            format!("{sign}{}{}.0", digits, "0".repeat(decimal - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..decimal], &digits[decimal..])
        }
    }
}
