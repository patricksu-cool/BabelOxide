//! Providers that expose the original, non-LLM translation entry point.
//!
//! Retry boundaries deliberately follow the Python implementations. In
//! particular, the SDK loops in DeepL and Azure are inside the application
//! retry, whereas process startup and language-independent validation are not.

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::Method;
use serde_json::Value;

use crate::{
    ErrorKind, HttpBody, HttpRequest, HttpResponse, ProviderContext, ProviderOutput,
    ProviderResult, RetryPolicy, RetrySelector, Usage, WaitPolicy,
};

mod azure;
mod cli;
mod deepl;
mod html5;
mod tencent;
mod web;

fn canonical(provider: &str) -> String {
    provider.to_ascii_lowercase().replace(['-', '_', ' '], "")
}

pub fn supports(provider: &str) -> bool {
    matches!(
        canonical(provider).as_str(),
        "google"
            | "bing"
            | "deepl"
            | "azure"
            | "anythingllm"
            | "dify"
            | "tencent"
            | "tencentmechinetranslation"
            | "tencentmachinetranslation"
            | "clitranslator"
            | "claudecode"
    )
}

pub fn is_llm(_provider: &str) -> bool {
    // These Python classes implement do_translate, never do_llm_translate.
    false
}

/// Startup checks belong outside translation retry. The owner may cache this
/// result for the lifetime of a configured translator.
pub async fn preflight(ctx: &ProviderContext<'_>) -> ProviderResult<()> {
    match canonical(&ctx.config.provider).as_str() {
        "clitranslator" => cli::validate_cli(ctx),
        "claudecode" => cli::preflight_claude(ctx).await,
        _ => Ok(()),
    }
}

pub async fn dispatch(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    match canonical(&ctx.config.provider).as_str() {
        "google" => web::google(ctx).await,
        "bing" => web::bing(ctx).await,
        "anythingllm" => web::anythingllm(ctx).await,
        "dify" => web::dify(ctx).await,
        "deepl" => deepl::translate(ctx).await,
        "azure" => azure::translate(ctx).await,
        "tencent" | "tencentmechinetranslation" | "tencentmachinetranslation" => {
            tencent::translate(ctx).await
        }
        "clitranslator" => cli::translate(ctx).await,
        "claudecode" => cli::claude(ctx).await,
        _ => Err(ctx.error(ErrorKind::InvalidConfiguration, "Unknown legacy provider")),
    }
}

fn policy(layer: &'static str, selector: RetrySelector) -> RetryPolicy {
    RetryPolicy {
        layer,
        max_attempts: 5,
        selector,
        wait: WaitPolicy::E,
        wrap_exhaustion: true,
    }
}

fn output(text: impl Into<String>) -> ProviderOutput {
    ProviderOutput {
        text: text.into(),
        usage: Usage::default(),
    }
}

fn option_string<'a>(ctx: &'a ProviderContext<'_>, key: &str) -> Option<&'a str> {
    ctx.option(key).and_then(Value::as_str)
}

fn language_in<'a>(ctx: &'a ProviderContext<'_>) -> &'a str {
    option_string(ctx, "lang_in").unwrap_or("en")
}

fn endpoint(ctx: &ProviderContext<'_>, fallback: &str) -> String {
    if ctx.config.endpoint.is_empty() {
        fallback.to_owned()
    } else {
        ctx.config.endpoint.clone()
    }
}

fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn request(method: Method, url: String, body: HttpBody, timeout: Option<Duration>) -> HttpRequest {
    HttpRequest {
        method,
        url,
        headers: BTreeMap::new(),
        query: Vec::new(),
        body,
        timeout,
    }
}

fn response_json(ctx: &ProviderContext<'_>, response: &HttpResponse) -> ProviderResult<Value> {
    serde_json::from_slice(&response.body).map_err(|_| {
        ctx.error(
            ErrorKind::MalformedResponse,
            "Provider returned invalid JSON",
        )
    })
}

fn require_string<'a>(
    ctx: &ProviderContext<'_>,
    value: &'a Value,
    pointer: &str,
) -> ProviderResult<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ctx.error(
                ErrorKind::MalformedResponse,
                "Provider response is missing translation text",
            )
        })
}

fn check_status(ctx: &ProviderContext<'_>, response: &HttpResponse) -> ProviderResult<()> {
    // requests.raise_for_status ignores redirects and only raises for >=400.
    if response.status >= 400 {
        Err(ctx.http_error(response))
    } else {
        Ok(())
    }
}

fn required_key(ctx: &ProviderContext<'_>) -> ProviderResult<()> {
    if ctx.config.api_key.is_empty() {
        Err(ctx.error(
            ErrorKind::InvalidConfiguration,
            "Provider API key is required",
        ))
    } else {
        Ok(())
    }
}
