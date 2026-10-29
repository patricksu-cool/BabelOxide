use std::{
    collections::BTreeMap,
    future::Future,
    task::Poll,
    time::{Duration, SystemTime},
};

use reqwest::{Method, Url};
use serde_json::{Value, json};

use crate::{
    ErrorKind, HttpBody, HttpRequest, HttpResponse, ProviderContext, ProviderError, ProviderOutput,
    ProviderResult, RetryPolicy, RetrySelector, TranslatorConfig, Usage, WaitPolicy, duration,
    legacy_strip, remove_think,
};

const QWEN_DOMAINS: &str = "This sentence is extracted from a scientific paper. When translating, please pay close attention to the use of specialized troubleshooting terminologies and adhere to scientific sentence structures to maintain the technical rigor and precision of the original text.";

pub fn validate(config: &TranslatorConfig) -> ProviderResult<()> {
    if !matches!(
        config.provider.as_str(),
        "openai" | "azure-openai" | "qwen-mt" | "siliconflow"
    ) {
        return Ok(());
    }
    if config.api_key.trim().is_empty() {
        return Err(ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "an OpenAI-compatible API key is required",
        ));
    }
    if config.model.trim().is_empty() {
        return Err(ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "an OpenAI-compatible model is required",
        ));
    }
    if config.provider == "openai" {
        duration(config.timeout_seconds)?;
    }
    if config.provider == "azure-openai" && config.endpoint.is_empty() {
        return Err(ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "an Azure OpenAI endpoint is required",
        ));
    }
    if config.provider == "qwen-mt" && !config.model.contains("qwen-mt") {
        return Err(ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "Qwen-MT requires a qwen-mt model",
        ));
    }
    Ok(())
}

pub async fn dispatch(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    if ctx.config.provider == "qwen-mt" && ctx.is_llm_request() {
        return Err(ctx.error(ErrorKind::Value, "Qwen-MT does not implement llm_translate"));
    }
    let layer = match ctx.config.provider.as_str() {
        "azure-openai" => "azureopenai.application",
        "qwen-mt" => "qwenmt.application",
        "siliconflow" => "siliconflow.application",
        _ => "openai.application",
    };
    ctx.retry(
        RetryPolicy {
            layer,
            max_attempts: 100,
            selector: RetrySelector::OpenAiRateLimit,
            wait: WaitPolicy::E,
            wrap_exhaustion: true,
        },
        || async {
            let request = chat_request(ctx)?;
            let response = sdk_request(ctx, request).await?;
            parse_chat(ctx, response)
        },
    )
    .await
}

fn chat_request(ctx: &ProviderContext<'_>) -> ProviderResult<HttpRequest> {
    let provider = ctx.config.provider.as_str();
    let content = if ctx.is_llm_request() || provider == "qwen-mt" {
        ctx.request.text.clone()
    } else {
        ctx.prompt(&ctx.request.text)
    };
    let mut body = json!({"model":ctx.config.model,"messages":[{"role":"user","content":content}]});
    let mut headers = BTreeMap::new();
    let mut query = Vec::new();
    let endpoint = match provider {
        "azure-openai" => {
            headers.insert("api-key".into(), ctx.config.api_key.clone());
            query.push((
                "api-version".into(),
                ctx.option("api_version")
                    .and_then(Value::as_str)
                    .unwrap_or("2024-06-01")
                    .to_owned(),
            ));
            body["temperature"] = json!(0);
            let mut url = parse_url(&ctx.config.endpoint)?;
            {
                let mut parts = url.path_segments_mut().map_err(|_| {
                    ctx.error(
                        ErrorKind::InvalidConfiguration,
                        "invalid Azure OpenAI endpoint",
                    )
                })?;
                parts
                    .pop_if_empty()
                    .push("openai")
                    .push("deployments")
                    .push(&ctx.config.model)
                    .push("chat")
                    .push("completions");
            }
            url.to_string()
        }
        _ => {
            headers.insert(
                "authorization".into(),
                format!("Bearer {}", ctx.config.api_key),
            );
            let fallback = match provider {
                "qwen-mt" => "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "siliconflow" => "https://api.siliconflow.cn/v1",
                _ => "https://api.openai.com/v1",
            };
            append_path(
                if ctx.config.endpoint.is_empty() {
                    fallback
                } else {
                    &ctx.config.endpoint
                },
                "chat/completions",
            )?
        }
    };
    match provider {
        "qwen-mt" => {
            body["temperature"] = json!(0);
            body["translation_options"] = json!({
                "source_lang": qwen_language(ctx.option("lang_in").and_then(Value::as_str).unwrap_or("en"))?,
                "target_lang": qwen_language(&ctx.config.lang_out)?,
                "domains": ctx.option("ali_domains").or_else(|| ctx.option("domains")).cloned().unwrap_or_else(|| json!(QWEN_DOMAINS)),
            });
        }
        "siliconflow" => {
            body["temperature"] = json!(0);
            if boolean(ctx, "enable_json_mode") {
                body["response_format"] = json!({"type":"json_object"});
            }
            if boolean(ctx, "send_enable_thinking_param") {
                body["enable_thinking"] = json!(boolean(ctx, "enable_thinking"));
            }
        }
        "openai" => {
            for key in ["temperature", "reasoning_effort"] {
                if let Some(value) = ctx.option(key) {
                    body[key] = value.clone();
                }
            }
            if boolean(ctx, "enable_json_mode")
                && ctx
                    .request
                    .rate_limit_params
                    .get("request_json_mode")
                    .and_then(Value::as_bool)
                    == Some(true)
            {
                body["response_format"] = json!({"type":"json_object"});
            }
            // The SDK's extra_body merges into the serialized request last.
            if let Some(extra) = ctx.option("extra_body") {
                let extra = extra
                    .as_object()
                    .ok_or_else(|| ctx.error(ErrorKind::Value, "extra_body must be an object"))?;
                for (key, value) in extra {
                    body[key] = value.clone();
                }
            }
        }
        _ => {}
    }
    for container in ["headers", "extra_headers"] {
        if let Some(extra) = ctx.option(container).and_then(Value::as_object) {
            for (key, value) in extra {
                if let Some(value) = value.as_str() {
                    headers.insert(key.to_ascii_lowercase(), value.to_owned());
                }
            }
        }
    }
    let timeout = if provider == "openai"
        && ctx.option("timeout_explicit").and_then(Value::as_bool) != Some(false)
    {
        duration(ctx.config.timeout_seconds)?
    } else {
        Duration::from_secs(600)
    };
    Ok(HttpRequest {
        method: Method::POST,
        url: endpoint,
        headers,
        query,
        body: HttpBody::Json(body),
        timeout: Some(timeout),
    })
}

pub(crate) async fn sdk_request(
    ctx: &ProviderContext<'_>,
    request: HttpRequest,
) -> ProviderResult<HttpResponse> {
    // OpenAI 3.36.1 DEFAULT_MAX_RETRIES=2: three physical HTTP attempts.
    for used_retries in 0..=2 {
        let mut request = request.clone();
        request
            .headers
            .insert("x-stainless-retry-count".into(), used_retries.to_string());
        let result = ctx.http.send(request, ctx.cancel).await;
        let (error, headers) = match result {
            Ok(response) if (200..300).contains(&response.status) => return Ok(response),
            Ok(response) => {
                let error = ctx.http_error(&response);
                if used_retries == 2 || !should_retry(&response, ctx.runtime.wall_time()) {
                    return Err(error);
                }
                (error, Some(response))
            }
            Err(error) => {
                if error.is_cancelled() || used_retries == 2 || !error.is_transport() {
                    return Err(error);
                }
                (error, None)
            }
        };
        let _ = error;
        let delay = headers
            .as_ref()
            .and_then(|response| retry_after(response, ctx.runtime.wall_time()))
            .filter(|delay| delay.is_finite() && *delay > 0.0 && *delay <= 120.0)
            .unwrap_or_else(|| {
                (0.5_f64 * 2_f64.powi(used_retries)).min(8.0)
                    * (1.0 - 0.25 * ctx.runtime.random_unit())
            });
        ctx.runtime.record_retry_sleep();
        ctx.runtime
            .sleep(Duration::from_secs_f64(delay), ctx.cancel)
            .await?;
    }
    unreachable!("SDK retry loop returns its final result")
}

pub fn should_retry(response: &HttpResponse, now: SystemTime) -> bool {
    if retry_after(response, now).is_some_and(|delay| delay.is_finite() && delay > 120.0) {
        return false;
    }
    match response.header("x-should-retry") {
        Some("true") => true,
        Some("false") => false,
        _ => matches!(response.status, 408 | 409 | 429) || response.status >= 500,
    }
}

pub fn retry_after(response: &HttpResponse, now: SystemTime) -> Option<f64> {
    for (header, divisor) in [("retry-after-ms", 1000.0), ("retry-after", 1.0)] {
        if let Some(value) = response.header(header)
            && let Some(delay) = python_float(value)
        {
            if delay == f64::INFINITY
                && !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "inf" | "+inf" | "infinity" | "+infinity"
                )
            {
                return Some(121.0);
            }
            return Some(delay / divisor);
        }
    }
    let date = httpdate::parse_http_date(response.header("retry-after")?).ok()?;
    Some(match date.duration_since(now) {
        Ok(duration) => duration.as_secs_f64(),
        Err(error) => -error.duration().as_secs_f64(),
    })
}

fn python_float(value: &str) -> Option<f64> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => return Some(f64::INFINITY),
        "-inf" | "-infinity" => return Some(f64::NEG_INFINITY),
        "nan" | "+nan" | "-nan" => return Some(f64::NAN),
        _ => {}
    }
    let bytes = value.as_bytes();
    for (index, character) in bytes.iter().enumerate() {
        if *character == b'_'
            && (index == 0
                || index + 1 == bytes.len()
                || !bytes[index - 1].is_ascii_digit()
                || !bytes[index + 1].is_ascii_digit())
        {
            return None;
        }
    }
    value.replace('_', "").parse().ok()
}

fn parse_chat(ctx: &ProviderContext<'_>, response: HttpResponse) -> ProviderResult<ProviderOutput> {
    let value = response.json()?;
    let usage = if ctx.config.provider == "qwen-mt" {
        Usage::default()
    } else {
        usage(&value)
    };
    // Python accounts usage before indexing choices, including malformed choices.
    ctx.record_usage(usage);
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ctx.error(
                ErrorKind::MalformedResponse,
                "translation provider returned no message content",
            )
        })?;
    Ok(ProviderOutput {
        text: remove_think(legacy_strip(content)).to_owned(),
        usage,
    })
}

fn usage(value: &Value) -> Usage {
    let number = |key| value.pointer(key).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        total_tokens: number("/usage/total_tokens"),
        prompt_tokens: number("/usage/prompt_tokens"),
        completion_tokens: number("/usage/completion_tokens"),
        cache_hit_prompt_tokens: value
            .pointer("/usage/prompt_cache_hit_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| number("/usage/prompt_tokens_details/cached_tokens")),
    }
}

fn qwen_language(language: &str) -> ProviderResult<&'static str> {
    match language {
        "zh-CN" | "zh-TW" => Ok("Chinese"),
        "en" => Ok("English"),
        "fr" => Ok("French"),
        "de" => Ok("German"),
        "ja" => Ok("Japanese"),
        "ko" => Ok("Korean"),
        "ru" => Ok("Russian"),
        "es" => Ok("Spanish"),
        "it" => Ok("Italian"),
        _ => Err(ProviderError::new(
            ErrorKind::Value,
            "unsupported Qwen-MT language code",
        )),
    }
}
fn boolean(ctx: &ProviderContext<'_>, key: &str) -> bool {
    ctx.option(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn append_path(endpoint: &str, path: &str) -> ProviderResult<String> {
    let mut url = parse_url(endpoint)?;
    let current = url.path().trim_end_matches('/').to_owned();
    if !current.ends_with(&format!("/{path}")) {
        url.set_path(&format!("{current}/{path}"));
    } else {
        url.set_path(&current);
    }
    Ok(url.to_string())
}
fn parse_url(endpoint: &str) -> ProviderResult<Url> {
    Url::parse(endpoint).map_err(|_| {
        ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "invalid translation provider endpoint",
        )
    })
}

pub async fn dispatch_free(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    let text = if ctx.is_llm_request() {
        ctx.request.text.clone()
    } else {
        ctx.prompt(&ctx.request.text)
    };
    let outer = RetryPolicy {
        layer: "siliconflowfree.http",
        max_attempts: 3,
        selector: RetrySelector::Http,
        wait: WaitPolicy::H,
        wrap_exhaustion: true,
    };
    let inner = RetryPolicy {
        layer: "siliconflowfree.rate_limit",
        max_attempts: 100,
        selector: RetrySelector::FreeRateLimit,
        wait: WaitPolicy::F,
        wrap_exhaustion: true,
    };
    ctx.retry(outer, || async {
        ctx.retry(inner, || async {
            let endpoint = ctx
                .http
                .state
                .lock()
                .await
                .get("free.endpoint")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if ctx.config.endpoint.is_empty() {
                        "https://api1.pdf2zh-next.com/chatproxy".into()
                    } else {
                        ctx.config.endpoint.clone()
                    }
                });
            let mut body = json!({"text":text});
            if boolean(ctx, "enable_json_mode")
                && ctx
                    .request
                    .rate_limit_params
                    .get("request_json_mode")
                    .and_then(Value::as_bool)
                    == Some(true)
            {
                body["requestJsonMode"] = json!(true);
            }
            let response = ctx
                .http
                .send(
                    HttpRequest {
                        method: Method::POST,
                        url: endpoint,
                        headers: BTreeMap::new(),
                        query: vec![],
                        body: HttpBody::Json(body),
                        timeout: Some(Duration::from_secs(60)),
                    },
                    ctx.cancel,
                )
                .await?;
            if response.status == 429 {
                return Err(ctx.error(
                    ErrorKind::FreeRateLimit,
                    "translation provider rate limited the request",
                ));
            }
            if !(200..300).contains(&response.status) {
                return Err(ctx.http_error(&response));
            }
            let value = response.json()?;
            let content = value
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ctx.error(
                        ErrorKind::MalformedResponse,
                        "translation provider returned no content",
                    )
                })?;
            Ok(ProviderOutput::text(remove_think(content)))
        })
        .await
    })
    .await
}

pub async fn preflight_free(ctx: &ProviderContext<'_>) -> ProviderResult<()> {
    if boolean(ctx, "skip_preflight") {
        return Ok(());
    }
    {
        let mut state = ctx.http.state.lock().await;
        state.insert("free.qps".into(), json!(10));
        state.insert("free.max_pool_size".into(), json!(100));
    }
    let endpoints: Vec<String> = ctx
        .option("endpoints")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_else(|| {
            if ctx.config.endpoint.is_empty() {
                vec![
                    "https://api1.pdf2zh-next.com/chatproxy".into(),
                    "https://api2.pdf2zh-next.com/chatproxy".into(),
                ]
            } else {
                vec![ctx.config.endpoint.clone()]
            }
        });
    let fallback = endpoints.first().cloned().ok_or_else(|| {
        ctx.error(
            ErrorKind::InvalidConfiguration,
            "free provider requires at least one endpoint",
        )
    })?;
    let mut tests = Vec::new();
    for endpoint in endpoints {
        tests.push(Box::pin(async move {
            let mut successes = 0;
            let mut elapsed = Duration::ZERO;
            for _ in 0..3 {
                let before = ctx.runtime.now();
                let result = ctx
                    .http
                    .send(
                        HttpRequest {
                            method: Method::POST,
                            url: format!("{}/check", endpoint.trim_end_matches('/')),
                            headers: BTreeMap::new(),
                            query: vec![],
                            body: HttpBody::Empty,
                            timeout: Some(Duration::from_secs(5)),
                        },
                        ctx.cancel,
                    )
                    .await;
                elapsed += ctx.runtime.now().duration_since(before);
                match result {
                    Ok(response) if response.status == 200 => {
                        let data = response.json();
                        if let Ok(data) = data {
                            if data.get("status").and_then(Value::as_str) == Some("ok") {
                                successes += 1;
                            } else if data.get("status").is_some() && data.get("message").is_some()
                            {
                                return Err(ctx.error(
                                    ErrorKind::Provider,
                                    "free translation server is unavailable",
                                ));
                            }
                        }
                    }
                    Err(error) if error.is_cancelled() => return Err(error),
                    _ => {}
                }
            }
            Ok((endpoint, elapsed, successes >= 2))
        }));
    }
    // Poll endpoint probes concurrently, preserving the SDK's three checks each.
    let mut finished = vec![false; tests.len()];
    let mut results = Vec::new();
    let results = std::future::poll_fn(|cx| {
        for (index, test) in tests.iter_mut().enumerate() {
            if !finished[index]
                && let Poll::Ready(result) = test.as_mut().poll(cx)
            {
                if result.as_ref().is_err_and(|error| error.is_cancelled()) {
                    return Poll::Ready(Err(ProviderError::cancelled()));
                }
                finished[index] = true;
                results.push(result);
            }
        }
        if results.len() == tests.len() {
            Poll::Ready(
                std::mem::take(&mut results)
                    .into_iter()
                    .collect::<ProviderResult<Vec<_>>>(),
            )
        } else {
            Poll::Pending
        }
    })
    .await?;
    let selected = results
        .into_iter()
        .filter(|value| value.2)
        .min_by_key(|value| value.1)
        .map(|value| value.0)
        .unwrap_or(fallback);
    ctx.http
        .state
        .lock()
        .await
        .insert("free.endpoint".into(), json!(selected));
    let response = ctx
        .http
        .send(
            HttpRequest {
                method: Method::GET,
                url: format!("{}/config", selected.trim_end_matches('/')),
                headers: BTreeMap::new(),
                query: vec![],
                body: HttpBody::Empty,
                timeout: Some(Duration::from_secs(100)),
            },
            ctx.cancel,
        )
        .await;
    match response {
        Ok(response) if response.status == 200 => {
            if let Ok(value) = response.json()
                && value.get("status").and_then(Value::as_str) == Some("ok")
            {
                let qps = value
                    .get("qps")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0 && *value <= u32::MAX as u64);
                let workers = value
                    .get("max_pool_size")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0 && *value <= u32::MAX as u64);
                if let (Some(qps), Some(workers)) = (qps, workers) {
                    let mut state = ctx.http.state.lock().await;
                    state.insert("free.qps".into(), json!(qps));
                    state.insert("free.max_pool_size".into(), json!(workers));
                    state.insert("free.apply_qps".into(), json!(true));
                }
            }
        }
        Err(error) if error.is_cancelled() => return Err(error),
        _ => {}
    }
    Ok(())
}
