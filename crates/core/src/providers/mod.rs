use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    ErrorKind, HttpResponse, HttpTransport, ProviderError, ProviderResult, RetryPolicy,
    RetryRuntime, TranslationRequest, TranslatorConfig, Usage, retry,
};

pub mod legacy;
pub mod local;
pub mod openai;

pub struct ProviderOutput {
    pub text: String,
    pub usage: Usage,
}

impl ProviderOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            usage: Usage::default(),
        }
    }
}

pub struct ProviderContext<'a> {
    pub config: &'a TranslatorConfig,
    pub request: &'a TranslationRequest,
    pub http: &'a HttpTransport,
    pub runtime: &'a RetryRuntime,
    pub cancel: &'a CancellationToken,
}

impl ProviderContext<'_> {
    pub fn option(&self, key: &str) -> Option<&Value> {
        self.config.options.get(key)
    }
    pub fn prompt(&self, text: &str) -> String {
        crate::legacy_prompt(text, &self.config.lang_out)
    }
    pub fn is_llm_request(&self) -> bool {
        matches!(
            self.request.method.as_str(),
            "llm_translate" | "do_llm_translate"
        )
    }
    pub fn error(&self, kind: ErrorKind, message: impl AsRef<str>) -> ProviderError {
        ProviderError::new(kind, self.sanitize(message.as_ref()))
    }
    pub fn http_error(&self, response: &HttpResponse) -> ProviderError {
        let detail = serde_json::from_slice::<Value>(&response.body)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/error/message")
                    .or_else(|| value.get("message"))
                    .or_else(|| value.get("detail"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        let message = match detail {
            Some(message) => format!(
                "translation provider returned HTTP {}: {}",
                response.status,
                self.sanitize(&message)
            ),
            None => format!("translation provider returned HTTP {}", response.status),
        };
        ProviderError::new(ErrorKind::HttpStatus(response.status), message)
    }
    pub async fn retry<T, F, Fut>(&self, policy: RetryPolicy, operation: F) -> ProviderResult<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = ProviderResult<T>>,
    {
        retry::run(self.runtime, self.cancel, policy, operation).await
    }
    pub fn record_usage(&self, usage: Usage) {
        self.runtime.metrics().record_usage(usage);
    }
    fn sanitize(&self, message: &str) -> String {
        let mut secrets = Vec::new();
        if !self.request.text.is_empty() {
            secrets.push(self.request.text.as_str());
        }
        if !self.config.api_key.is_empty() {
            secrets.push(self.config.api_key.as_str());
        }
        collect_secrets(&self.config.options, &mut secrets);
        // Find overlapping ranges in the untouched original. Replacing one
        // secret first could otherwise destroy a source/key match that overlaps it.
        // Search the complete input, including matches crossing the output bound.
        let bound = message
            .char_indices()
            .nth(4096)
            .map(|(offset, _)| offset)
            .unwrap_or(message.len());
        let mut ranges = Vec::new();
        for secret in secrets {
            if !secret.is_empty() {
                let mut offset = 0;
                while offset < bound {
                    let Some(found) = message[offset..].find(secret) else {
                        break;
                    };
                    let start = offset + found;
                    if start >= bound {
                        break;
                    }
                    ranges.push((start, (start + secret.len()).min(bound)));
                    offset = start
                        + message[start..]
                            .chars()
                            .next()
                            .expect("matched nonempty secret")
                            .len_utf8();
                }
            }
        }
        // Provider-originated URLs can contain keys or document contents in query strings.
        let urls = regex::Regex::new(r#"https?://[^\s\"'<>]+"#).expect("static URL pattern");
        for found in urls
            .find_iter(message)
            .take_while(|found| found.start() < bound)
        {
            ranges.push((found.start(), found.end().min(bound)));
        }
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
                continue;
            }
            merged.push((start, end));
        }
        let mut safe = String::new();
        let mut cursor = 0;
        for (start, end) in merged {
            safe.push_str(&message[cursor..start]);
            safe.push_str("[REDACTED]");
            cursor = end;
        }
        safe.push_str(&message[cursor..bound]);
        safe.chars()
            .take(4096)
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect()
    }
}

fn collect_secrets<'a>(value: &'a Value, output: &mut Vec<&'a str>) {
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            let key = key.to_ascii_lowercase();
            if matches!(
                key.as_str(),
                "headers"
                    | "extra_headers"
                    | "default_headers"
                    | "http_headers"
                    | "request_headers"
            ) {
                collect_header_secrets(value, output);
            }
            if ["key", "secret", "token", "password", "authorization"]
                .iter()
                .any(|part| key.contains(part))
                && let Some(value) = value.as_str()
            {
                output.push(value);
                if let Some((_, payload)) = value.split_once(' ')
                    && !payload.trim().is_empty()
                {
                    output.push(payload.trim());
                }
            }
            collect_secrets(value, output);
        }
    } else if let Some(array) = value.as_array() {
        for value in array {
            collect_secrets(value, output);
        }
    }
}

fn collect_header_secrets<'a>(value: &'a Value, output: &mut Vec<&'a str>) {
    match value {
        Value::String(value) => {
            output.push(value);
            if let Some((_, payload)) = value.split_once(' ')
                && !payload.trim().is_empty()
            {
                output.push(payload.trim());
            }
        }
        Value::Object(value) => {
            for value in value.values() {
                collect_header_secrets(value, output);
            }
        }
        Value::Array(value) => {
            for value in value {
                collect_header_secrets(value, output);
            }
        }
        _ => {}
    }
}

pub(crate) async fn dispatch(
    ctx: &ProviderContext<'_>,
    num_predict: &AtomicU64,
) -> ProviderResult<ProviderOutput> {
    match ctx.config.provider.as_str() {
        "openai" | "azure-openai" | "qwen-mt" | "siliconflow" => openai::dispatch(ctx).await,
        "siliconflowfree" => openai::dispatch_free(ctx).await,
        "ollama" => local::ollama(ctx, num_predict).await,
        "xinference" => local::xinference(ctx).await,
        _ => legacy::dispatch(ctx).await,
    }
}

pub(crate) fn supports(provider: &str) -> bool {
    matches!(
        provider,
        "openai"
            | "azure-openai"
            | "qwen-mt"
            | "siliconflow"
            | "siliconflowfree"
            | "ollama"
            | "xinference"
    ) || legacy::supports(provider)
}
pub(crate) fn is_llm(provider: &str) -> bool {
    !matches!(provider, "qwen-mt")
        && (matches!(
            provider,
            "openai" | "azure-openai" | "siliconflow" | "siliconflowfree" | "ollama" | "xinference"
        ) || legacy::is_llm(provider))
}

pub(crate) fn update_num_predict(value: &AtomicU64, minimum: u64) -> u64 {
    value.fetch_max(minimum, Ordering::Relaxed).max(minimum)
}

pub(crate) async fn preflight(ctx: &ProviderContext<'_>) -> ProviderResult<()> {
    match ctx.config.provider.as_str() {
        "siliconflowfree" => openai::preflight_free(ctx).await,
        "xinference" => local::preflight_xinference(ctx).await,
        provider if legacy::supports(provider) => legacy::preflight(ctx).await,
        _ => Ok(()),
    }
}
