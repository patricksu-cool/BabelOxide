//! Rust-owned provider execution, HTTP transport, retry policies and accounting.

pub const PRODUCT_NAME: &str = "BabelOxide";
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, OnceCell, Semaphore};
use tokio_util::sync::CancellationToken;

pub mod cache;
pub mod error;
pub mod limiter;
pub mod metrics;
pub mod process;
pub mod providers;
pub mod retry;
pub mod transport;

pub use cache::{
    CacheBackend, CacheError, CacheErrorKind, CacheFuture, CacheKey, CacheMethod, CacheResult,
    CacheStore, LegacyBinding, LegacyNamespace,
};
pub use error::{ErrorKind, ProviderError, ProviderResult};
pub use limiter::RateLimiter;
pub use metrics::{MetricsSnapshot, Usage};
pub use providers::{ProviderContext, ProviderOutput};
pub use retry::{Clock, RandomSource, RetryPolicy, RetryRuntime, RetrySelector, WaitPolicy};
pub use transport::{
    HttpBody, HttpExecutor, HttpRequest, HttpResponse, HttpTransport, MAX_RESPONSE_BYTES,
};

/// Provider configuration stays in Rust and never enters the PDF worker.
#[derive(Clone)]
pub struct TranslatorConfig {
    pub provider: String,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub timeout_seconds: f64,
    pub lang_out: String,
    pub options: Value,
}
impl fmt::Debug for TranslatorConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TranslatorConfig")
            .field("provider", &self.provider)
            .field("endpoint", &"[REDACTED]")
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("lang_out", &self.lang_out)
            .field("options", &"[REDACTED]")
            .finish()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PdfJob {
    pub input_file: PathBuf,
    pub output_dir: PathBuf,
    pub lang_in: String,
    pub lang_out: String,
    pub pages: Option<String>,
    pub qps: u32,
    pub pool_max_workers: u32,
    pub no_auto_extract_glossary: bool,
    pub watermark_output_mode: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
/// Source text deliberately has no Debug implementation.
#[derive(Clone, Deserialize)]
pub struct TranslationRequest {
    pub id: String,
    pub text: String,
    pub method: String,
    pub ignore_cache: bool,
    pub rate_limit_params: Value,
    pub translator: String,
}

pub struct TranslationService {
    config: TranslatorConfig,
    http: HttpTransport,
    runtime: RetryRuntime,
    rate_limiter: RateLimiter,
    concurrency: Option<Semaphore>,
    preflight: OnceCell<ProviderResult<()>>,
    preflight_lock: Mutex<()>,
    num_predict: AtomicU64,
    recommended_qps: AtomicU32,
    recommended_workers: AtomicU32,
    cache: Option<Arc<dyn CacheBackend>>,
    legacy_cache: Option<LegacyBinding>,
}
impl TranslationService {
    pub fn new(config: TranslatorConfig) -> Result<Self> {
        Self::new_with_runtime(config, RetryRuntime::default())
    }
    pub fn new_with_runtime(mut config: TranslatorConfig, runtime: RetryRuntime) -> Result<Self> {
        config.provider = match config.provider.to_ascii_lowercase().as_str() {
            "openai-compatible" => "openai".into(),
            "azureopenai" => "azure-openai".into(),
            "qwenmt" => "qwen-mt".into(),
            "siliconflow-free" => "siliconflowfree".into(),
            provider => provider.to_owned(),
        };
        if !providers::supports(&config.provider) {
            return Err(anyhow!(ProviderError::new(
                ErrorKind::InvalidConfiguration,
                "unsupported translation provider"
            )));
        }
        if !config.options.is_null() && !config.options.is_object() {
            return Err(anyhow!(ProviderError::new(
                ErrorKind::InvalidConfiguration,
                "translator options must be an object"
            )));
        }
        providers::openai::validate(&config)?;
        if matches!(config.provider.as_str(), "ollama" | "xinference")
            && config.model.trim().is_empty()
        {
            return Err(anyhow!(ProviderError::new(
                ErrorKind::InvalidConfiguration,
                "a model is required"
            )));
        }
        let qps = positive_option(&config.options, "qps", 4)?;
        let concurrency = config
            .options
            .get("max_concurrency")
            .map(|_| positive_option(&config.options, "max_concurrency", qps))
            .transpose()?;
        if concurrency.is_some_and(|value| value as usize > Semaphore::MAX_PERMITS) {
            return Err(anyhow!(ProviderError::new(
                ErrorKind::InvalidConfiguration,
                "max_concurrency exceeds the supported limit"
            )));
        }
        let num_predict = config
            .options
            .get("num_predict")
            .and_then(Value::as_u64)
            .unwrap_or(2000);
        let connect_timeout = if config.provider == "openai"
            && config
                .options
                .get("timeout_explicit")
                .and_then(Value::as_bool)
                != Some(false)
        {
            Some(duration(config.timeout_seconds)?)
        } else if matches!(
            config.provider.as_str(),
            "openai" | "azure-openai" | "qwen-mt" | "siliconflow"
        ) {
            Some(Duration::from_secs(5))
        } else {
            None
        };
        let redirect_limit = match config.provider.as_str() {
            "openai" | "azure-openai" | "qwen-mt" | "siliconflow" | "ollama" => 20,
            "siliconflowfree" | "azure" => 0,
            _ => 30,
        };
        let mut http = HttpTransport::new(runtime.clone(), connect_timeout)
            .with_redirect_limit(redirect_limit)
            .with_diagnostics(
                config
                    .options
                    .get("diagnostic_http")
                    .and_then(Value::as_bool)
                    == Some(true),
            );
        if matches!(config.provider.as_str(), "anythingllm" | "dify") {
            http = http.with_request_scoped_cookies();
        } else if config.provider == "deepl" {
            http = http.with_prepared_initial_request();
        }
        Ok(Self {
            rate_limiter: RateLimiter::with_now(qps, runtime.now()),
            config,
            http,
            runtime,
            concurrency: concurrency.map(|value| Semaphore::new(value as usize)),
            preflight: OnceCell::new(),
            preflight_lock: Mutex::new(()),
            num_predict: AtomicU64::new(num_predict),
            recommended_qps: AtomicU32::new(0),
            recommended_workers: AtomicU32::new(0),
            cache: None,
            legacy_cache: None,
        })
    }
    pub fn new_with_transport(
        config: TranslatorConfig,
        runtime: RetryRuntime,
        http: HttpTransport,
    ) -> Result<Self> {
        let mut service = Self::new_with_runtime(config, runtime)?;
        service.http = http;
        Ok(service)
    }
    /// Cache access remains optional and never chooses a user path implicitly.
    pub fn with_cache(mut self, cache: Arc<dyn CacheBackend>) -> Self {
        self.cache = Some(cache);
        self
    }
    /// Explicitly assigns the provenance of old Python rows to this configured
    /// service. Unbound legacy rows are preserved but never automatically used.
    pub fn with_legacy_cache(mut self, binding: LegacyBinding) -> Self {
        self.legacy_cache = Some(binding);
        self
    }
    /// A PDF gets a new SDK/session/limiter lifetime while retaining the shared
    /// application cache. Construction never sends a request or starts a child.
    pub fn fresh_for_document(&self) -> Result<Self> {
        let mut fresh = Self::new(self.config.clone())?;
        fresh.cache = self.cache.clone();
        fresh.legacy_cache = self.legacy_cache.clone();
        Ok(fresh)
    }
    pub fn name(&self) -> &str {
        &self.config.provider
    }
    /// Apply the same credential/URL redaction to diagnostics from the engine.
    pub fn redact_diagnostic(&self, message: &str) -> String {
        let request = TranslationRequest {
            id: String::new(),
            text: String::new(),
            method: String::new(),
            ignore_cache: true,
            rate_limit_params: Value::Null,
            translator: String::new(),
        };
        ProviderContext {
            config: &self.config,
            request: &request,
            http: &self.http,
            runtime: &self.runtime,
            cancel: &CancellationToken::new(),
        }
        .sanitize(message)
    }
    pub fn is_llm(&self) -> bool {
        providers::is_llm(&self.config.provider)
    }
    pub fn requires_reap(&self) -> bool {
        matches!(
            self.config
                .provider
                .to_ascii_lowercase()
                .replace(['-', '_', ' '], "")
                .as_str(),
            "clitranslator" | "claudecode"
        )
    }
    pub fn owns_process(&self) -> bool {
        self.requires_reap()
    }
    /// Cache backends can own blocking SQLite work or transaction rollback.
    /// The bridge must await their cooperative cancellation just as it awaits
    /// a child-process owner; aborting the future can detach that cleanup.
    pub fn requires_cooperative_cleanup(&self) -> bool {
        self.requires_reap() || self.cache.is_some()
    }
    pub fn lang_out(&self) -> &str {
        &self.config.lang_out
    }
    pub fn metrics(&self) -> MetricsSnapshot {
        self.runtime.metrics().snapshot()
    }
    pub fn recommendations(&self) -> (Option<u32>, Option<u32>) {
        let qps = self.recommended_qps.load(Ordering::Relaxed);
        let workers = self.recommended_workers.load(Ordering::Relaxed);
        ((qps > 0).then_some(qps), (workers > 0).then_some(workers))
    }
    pub async fn prepare(&self, cancel: &CancellationToken) -> ProviderResult<()> {
        let request = TranslationRequest {
            id: "prepare".into(),
            text: String::new(),
            method: "translate".into(),
            ignore_cache: true,
            rate_limit_params: Value::Null,
            translator: self.name().into(),
        };
        let ctx = ProviderContext {
            config: &self.config,
            request: &request,
            http: &self.http,
            runtime: &self.runtime,
            cancel,
        };
        self.ensure_prepared(&ctx).await
    }
    pub async fn set_max_qps(&self, qps: u32) -> ProviderResult<()> {
        self.rate_limiter.set_max_qps(qps).await
    }
    /// The legacy factory checks ordinary translation, bypassing both cache
    /// reads and writes. Call after prepare, outside its initialization lock.
    pub async fn health_check(&self, cancel: &CancellationToken) -> ProviderResult<()> {
        self.translate_typed(
            &TranslationRequest {
                id: "health".into(),
                text: "Hello".into(),
                method: "translate".into(),
                ignore_cache: true,
                rate_limit_params: Value::Null,
                translator: self.name().into(),
            },
            cancel,
        )
        .await
        .map(|_| ())
    }
    pub async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: &CancellationToken,
    ) -> Result<String> {
        self.translate_typed(request, cancel)
            .await
            .map(|output| output.text)
            .map_err(anyhow::Error::new)
    }
    pub async fn translate_typed(
        &self,
        request: &TranslationRequest,
        cancel: &CancellationToken,
    ) -> ProviderResult<ProviderOutput> {
        self.runtime.metrics().call();
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        if !matches!(
            request.method.as_str(),
            "translate" | "do_translate" | "llm_translate" | "do_llm_translate"
        ) {
            return Err(ProviderError::new(
                ErrorKind::Value,
                "unsupported translation request method",
            ));
        }
        let cache_key = (self.cache.is_some()
            && !request.ignore_cache
            && self
                .config
                .options
                .get("ignore_cache")
                .and_then(Value::as_bool)
                != Some(true))
        .then(|| CacheKey::from_request(&self.config, request));
        if let Some(key) = &cache_key
            && let Some(text) = self.cached_translation(key, request, cancel).await?
        {
            self.runtime.metrics().cache_hit();
            return Ok(ProviderOutput::text(text));
        }
        let ctx = ProviderContext {
            config: &self.config,
            request,
            http: &self.http,
            runtime: &self.runtime,
            cancel,
        };
        self.ensure_prepared(&ctx).await?;
        let _permit = if let Some(concurrency) = &self.concurrency {
            Some(tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(ProviderError::cancelled()),
                permit = concurrency.acquire() => permit.map_err(|_| ProviderError::new(ErrorKind::Runtime, "translation service closed"))?,
            })
        } else {
            None
        };
        self.rate_limiter
            .wait_with_runtime(&self.runtime, cancel)
            .await?;
        self.runtime.metrics().admit();
        let _inflight = self.runtime.metrics().enter();
        // Cooperative provider cancellation permits process-tree kill and reap.
        let output = providers::dispatch(&ctx, &self.num_predict).await?;
        if let Some(key) = &cache_key {
            self.store_translation(key, &output.text, cancel).await?;
        }
        Ok(output)
    }
    async fn cached_translation(
        &self,
        key: &CacheKey,
        request: &TranslationRequest,
        cancel: &CancellationToken,
    ) -> ProviderResult<Option<String>> {
        let Some(cache) = &self.cache else {
            return Ok(None);
        };
        match cache.get(key, cancel).await {
            Ok(Some(text)) => {
                if cancel.is_cancelled() {
                    return Err(ProviderError::cancelled());
                }
                return Ok(Some(text));
            }
            Ok(None) => {}
            Err(error) if error.is_cancelled() => return Err(ProviderError::cancelled()),
            Err(_) => self.runtime.metrics().cache_read_error(),
        }
        if let Some(binding) = &self.legacy_cache
            && binding.matches(request)
        {
            match cache
                .get_legacy(&binding.namespace, &request.text, cancel)
                .await
            {
                Ok(Some(text)) => {
                    if cancel.is_cancelled() {
                        return Err(ProviderError::cancelled());
                    }
                    self.store_translation(key, &text, cancel).await?;
                    self.runtime.metrics().legacy_cache_hit();
                    return Ok(Some(text));
                }
                Ok(None) => {}
                Err(error) if error.is_cancelled() => return Err(ProviderError::cancelled()),
                Err(_) => self.runtime.metrics().cache_read_error(),
            }
        }
        Ok(None)
    }
    async fn store_translation(
        &self,
        key: &CacheKey,
        text: &str,
        cancel: &CancellationToken,
    ) -> ProviderResult<()> {
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        if let Some(cache) = &self.cache {
            match cache.set(key, text, cancel).await {
                Ok(()) => {}
                Err(error) if error.is_cancelled() => return Err(ProviderError::cancelled()),
                // Python TranslationCache.set catches Exception internally.
                // Cache persistence failure must not fail a valid translation.
                Err(_) => self.runtime.metrics().cache_write_error(),
            }
        }
        Ok(())
    }
    async fn ensure_prepared(&self, ctx: &ProviderContext<'_>) -> ProviderResult<()> {
        let cancel = ctx.cancel;
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        // A waiting caller can cancel without dropping another caller's
        // cooperative process initializer.
        if self.preflight.get().is_none() {
            let _initializing = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(ProviderError::cancelled()),
                guard = self.preflight_lock.lock() => guard,
            };
            if self.preflight.get().is_none() {
                let result = providers::preflight(ctx).await;
                if result.as_ref().is_err_and(|error| error.is_cancelled()) {
                    return Err(ProviderError::cancelled());
                }
                if result.is_ok() && self.config.provider == "siliconflowfree" {
                    let (qps, workers, apply_qps) = {
                        let state = self.http.state.lock().await;
                        (
                            state
                                .get("free.qps")
                                .and_then(Value::as_u64)
                                .and_then(|value| u32::try_from(value).ok()),
                            state
                                .get("free.max_pool_size")
                                .and_then(Value::as_u64)
                                .and_then(|value| u32::try_from(value).ok()),
                            state.get("free.apply_qps").and_then(Value::as_bool) == Some(true),
                        )
                    };
                    if let Some(qps) = qps {
                        if apply_qps {
                            self.rate_limiter.set_max_qps(qps).await?;
                        }
                        self.recommended_qps.store(qps, Ordering::Relaxed);
                    }
                    if let Some(workers) = workers {
                        self.recommended_workers.store(workers, Ordering::Relaxed);
                    }
                }
                let _ = self.preflight.set(result);
            }
        }
        self.preflight
            .get()
            .expect("completed preflight initializes its outcome")
            .clone()
    }
}
fn positive_option(options: &Value, key: &str, default: u32) -> Result<u32> {
    match options.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                anyhow!(ProviderError::new(
                    ErrorKind::InvalidConfiguration,
                    format!("{key} must be a positive 32-bit integer")
                ))
            }),
    }
}
pub(crate) fn duration(seconds: f64) -> ProviderResult<Duration> {
    Duration::try_from_secs_f64(seconds)
        .ok()
        .filter(|value| !value.is_zero())
        .ok_or_else(|| {
            ProviderError::new(
                ErrorKind::InvalidConfiguration,
                "translation timeout must be finite and positive",
            )
        })
}
pub(crate) fn legacy_prompt(text: &str, lang_out: &str) -> String {
    format!(
        "You are a professional,authentic machine translation engine.\n\n;; Treat next line as plain text input and translate it into {lang_out}, output translation ONLY. If translation is unnecessary (e.g. proper nouns, codes, {{{{1}}}}, etc. ), return the original text. NO explanations. NO notes. Input:\n\n{text}"
    )
}
pub(crate) fn legacy_strip(content: &str) -> &str {
    content.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{001c}'..='\u{001f}')
    })
}
pub(crate) fn remove_think(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("<think>") {
        let mut offset = 0;
        while let Some(index) = rest[offset..].find("</think>") {
            let index = offset + index;
            if index > 0 {
                return &rest[index + "</think>".len()..];
            }
            offset = "</think>".len();
        }
    }
    content
}
#[cfg(test)]
mod poc_tests;

#[cfg(test)]
mod service_unit_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn p05_service_factory_preserves_sdk_default_connect_and_explicit_timeout() {
        for (provider, explicit, seconds) in [
            ("openai", false, 5),
            ("openai", true, 7),
            ("azure-openai", false, 5),
            ("qwen-mt", false, 5),
            ("siliconflow", false, 5),
        ] {
            let service = TranslationService::new(TranslatorConfig {
                provider: provider.into(),
                endpoint: "http://127.0.0.1:1".into(),
                api_key: "synthetic-key".into(),
                model: "qwen-mt-fixture".into(),
                timeout_seconds: 7.0,
                lang_out: "zh-TW".into(),
                options: json!({"timeout_explicit":explicit}),
            })
            .unwrap();
            assert_eq!(
                service.http.configured_connect_timeout(),
                Some(Duration::from_secs(seconds))
            );
        }
    }
}
