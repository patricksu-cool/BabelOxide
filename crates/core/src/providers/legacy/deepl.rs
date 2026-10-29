use super::*;
use serde_json::json;

const COMMON_LANGUAGES: &[&str] = &[
    "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "he", "hu", "id", "it", "ja", "ko",
    "lt", "lv", "nb", "nl", "pl", "ro", "ru", "sk", "sl", "sv", "th", "tr", "uk", "vi",
];

fn source_language(ctx: &ProviderContext<'_>) -> ProviderResult<Option<String>> {
    let language = language_in(ctx).to_ascii_lowercase();
    let mapped = match language.as_str() {
        "auto" => return Ok(None),
        "zh" | "zh-cn" | "zh-tw" | "zh-hk" => "ZH",
        "pt" | "pt-br" | "pt-pt" => "PT",
        "no" => "NB",
        "en" => "EN",
        other if COMMON_LANGUAGES.contains(&other) => return Ok(Some(other.to_ascii_uppercase())),
        _ => {
            return Err(ctx.error(ErrorKind::Value, "Unsupported DeepL source language"));
        }
    };
    Ok(Some(mapped.to_owned()))
}

fn target_language(ctx: &ProviderContext<'_>) -> ProviderResult<String> {
    let language = ctx.config.lang_out.to_ascii_lowercase();
    let mapped = match language.as_str() {
        "en" | "en-us" => "EN-US",
        "en-gb" => "EN-GB",
        // BaseTranslator applies DeepL.lang_map before _map_target_lang:
        // plain zh becomes zh-CN and therefore ZH-HANS.
        "zh" | "zh-cn" | "zh-hans" => "ZH-HANS",
        "zh-tw" | "zh-hk" | "zh-hant" => "ZH-HANT",
        "pt" | "pt-pt" => "PT-PT",
        "pt-br" => "PT-BR",
        "es-419" => "ES-419",
        "no" => "NB",
        other if COMMON_LANGUAGES.contains(&other) => return Ok(other.to_ascii_uppercase()),
        _ => {
            return Err(ctx.error(ErrorKind::Value, "Unsupported DeepL target language"));
        }
    };
    Ok(mapped.to_owned())
}

fn network_retryable(kind: &ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::Transport
            | ErrorKind::Timeout
            | ErrorKind::Connect
            | ErrorKind::ConnectTimeout
            | ErrorKind::ReadTimeout
    )
}

pub(super) async fn translate(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    // The Python SDK rejects an empty key when constructing the translator,
    // before the decorated translation method is entered.
    required_key(ctx)?;
    ctx.retry(
        policy("deepl.application", RetrySelector::Predicate),
        || async {
            let source = source_language(ctx)?;
            let target = target_language(ctx)?;
            if ctx.request.text.is_empty() {
                return Err(ctx.error(ErrorKind::Value, "DeepL text must not be empty"));
            }
            let mut payload = json!({
                "text": [&ctx.request.text],
                "target_lang": target,
                "show_billed_characters": true,
            });
            if let Some(source) = source {
                payload["source_lang"] = Value::String(source);
            }
            let default_endpoint = if ctx.config.api_key.ends_with(":fx") {
                "https://api-free.deepl.com"
            } else {
                "https://api.deepl.com"
            };
            let base = endpoint(ctx, default_endpoint);
            let url = reqwest::Url::parse(&format!("{}/", base.trim_end_matches('/')))
                .and_then(|base| base.join("v2/translate"))
                .map_err(|_| ctx.error(ErrorKind::InvalidConfiguration, "Invalid DeepL endpoint"))?
                .to_string();

            // deepl 1.32.0: max_network_retries=5, not five attempts. The
            // deadline is established before the request; HTTP time consumes the
            // backoff. Retry-After is deliberately not consulted by this SDK.
            let mut backoff = 1.0_f64;
            let mut deadline = ctx.runtime.now() + Duration::from_secs(1);
            let mut retries = 0_u32;
            let response = loop {
                let until_deadline = deadline.saturating_duration_since(ctx.runtime.now());
                let mut req = request(
                    Method::POST,
                    url.clone(),
                    HttpBody::Json(payload.clone()),
                    Some(until_deadline.max(Duration::from_secs(10))),
                );
                req.headers = headers(&[(
                    "Authorization",
                    &format!("DeepL-Auth-Key {}", ctx.config.api_key),
                )]);
                let result = ctx.http.send(req, ctx.cancel).await;
                let should_retry = match &result {
                    Ok(response) => response.status == 429 || response.status >= 500,
                    Err(error) => network_retryable(&error.kind),
                };
                if !should_retry || retries >= 5 {
                    break result?;
                }
                ctx.runtime.record_retry_sleep();
                ctx.runtime
                    .sleep(
                        deadline.saturating_duration_since(ctx.runtime.now()),
                        ctx.cancel,
                    )
                    .await?;
                backoff = (backoff * 1.6).min(120.0);
                let jitter = 1.0 + 0.23 * (2.0 * ctx.runtime.random_unit() - 1.0);
                deadline = ctx.runtime.now() + Duration::from_secs_f64(backoff * jitter);
                retries += 1;
            };
            if response.status != 200 {
                return Err(ctx.http_error(&response));
            }
            let value = response_json(ctx, &response)?;
            let text = require_string(ctx, &value, "/translations/0/text")?;
            // The pinned SDK constructs TextResult with int(billed_characters).
            // A missing or nonnumeric count is a parsing error even when text exists.
            let billed = value.pointer("/translations/0/billed_characters");
            let valid_billed = billed.is_some_and(|value| {
                value.is_i64()
                    || value.is_u64()
                    || value.is_f64()
                    || value.is_boolean()
                    || value
                        .as_str()
                        .is_some_and(|text| text.trim().parse::<i64>().is_ok())
            });
            if !valid_billed {
                return Err(ctx.error(
                    ErrorKind::MalformedResponse,
                    "DeepL response has no billed character count",
                ));
            }
            Ok(output(text))
        },
    )
    .await
}
