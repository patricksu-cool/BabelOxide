use super::*;
use serde_json::json;

fn mapped_language(lang: &str) -> &str {
    if lang.eq_ignore_ascii_case("zh") {
        "zh-Hans"
    } else {
        lang
    }
}

fn retry_after(
    ctx: &ProviderContext<'_>,
    response: &HttpResponse,
) -> ProviderResult<Option<Duration>> {
    for (header, divisor) in [
        ("retry-after", 1.0),
        ("retry-after-ms", 1000.0),
        ("x-ms-retry-after-ms", 1000.0),
    ] {
        let Some(value) = response.header(header).filter(|value| !value.is_empty()) else {
            continue;
        };
        let seconds = match value.trim().parse::<f64>() {
            Ok(value) if value.is_nan() => 0.0,
            Ok(value) => value.max(0.0) / divisor,
            Err(_) => {
                let date = httpdate::parse_http_date(value).map_err(|_| {
                    ctx.error(
                        ErrorKind::Value,
                        "Azure returned an invalid Retry-After header",
                    )
                })?;
                date.duration_since(ctx.runtime.wall_time())
                    .unwrap_or_default()
                    .as_secs_f64()
                    / divisor
            }
        };
        // Python time.sleep(inf) fails rather than silently capping the wait.
        let duration = Duration::try_from_secs_f64(seconds).map_err(|_| {
            ctx.error(
                ErrorKind::Value,
                "Azure returned an unrepresentable retry delay",
            )
        })?;
        return Ok(Some(duration));
    }
    Ok(None)
}

fn safe_method(method: &Method) -> bool {
    matches!(
        method.as_str(),
        "HEAD" | "GET" | "PUT" | "DELETE" | "OPTIONS" | "TRACE"
    )
}

fn retry_status(method: &Method, response: &HttpResponse, total_budget: i32) -> bool {
    if response.status < 400 {
        return false;
    }
    if response
        .header("retry-after")
        .is_some_and(|value| !value.is_empty())
    {
        return true;
    }
    let allowed_method = safe_method(method)
        || (matches!(method.as_str(), "POST" | "PATCH")
            && matches!(response.status, 500 | 503 | 504));
    allowed_method
        && total_budget != 0
        && (matches!(response.status, 408 | 429 | 500 | 502 | 503 | 504)
            || (506..999).contains(&response.status))
}

/// One invocation of azure-core RetryPolicy. RedirectPolicy calls this again
/// at each target, giving it a fresh total/connect/read/status budget.
async fn sdk_send(
    ctx: &ProviderContext<'_>,
    template: &HttpRequest,
) -> ProviderResult<HttpResponse> {
    let mut total_budget = 10_i32;
    let mut connect_budget = 3_i32;
    let mut read_budget = 3_i32;
    let mut status_budget = 3_i32;
    let mut history_len = 0_i32;
    let mut response_error = true;
    let operation_deadline = ctx.runtime.now() + Duration::from_secs(604800);
    loop {
        let remaining = operation_deadline.saturating_duration_since(ctx.runtime.now());
        if remaining.is_zero() {
            let kind = if response_error {
                ErrorKind::ReadTimeout
            } else {
                ErrorKind::ConnectTimeout
            };
            return Err(ctx.error(kind, "Azure operation timed out"));
        }
        let mut req = template.clone();
        req.timeout = Some(Duration::from_secs(300));
        let result = ctx
            .http
            .send_with_connect_timeout(
                req,
                Some(remaining.min(Duration::from_secs(300))),
                ctx.cancel,
            )
            .await;
        let server_delay = match result {
            Ok(response) => {
                response_error = true;
                if !retry_status(&template.method, &response, total_budget) {
                    return Ok(response);
                }
                total_budget -= 1;
                status_budget -= 1;
                history_len += 1;
                if total_budget < 0 || status_budget < 0 {
                    return Ok(response);
                }
                retry_after(ctx, &response)?
            }
            Err(error) => {
                response_error =
                    !matches!(error.kind, ErrorKind::Connect | ErrorKind::ConnectTimeout);
                // A 303 can turn POST into GET. Only then does the SDK method
                // allowlist admit network retries and its independent budgets.
                if !safe_method(&template.method)
                    || !matches!(
                        error.kind,
                        ErrorKind::Transport
                            | ErrorKind::Connect
                            | ErrorKind::ConnectTimeout
                            | ErrorKind::ReadTimeout
                            | ErrorKind::ReadTransport
                            | ErrorKind::Timeout
                    )
                {
                    return Err(error);
                }
                total_budget -= 1;
                if matches!(error.kind, ErrorKind::Connect | ErrorKind::ConnectTimeout) {
                    connect_budget -= 1;
                    history_len += 1;
                } else if matches!(error.kind, ErrorKind::ReadTransport) {
                    // The requests transport converts malformed/incomplete
                    // bodies to HttpResponseError, not ServiceResponseError.
                    // RetryPolicy receives PipelineRequest for this exception:
                    // status budget decreases, but there is no response history.
                    status_budget -= 1;
                } else {
                    read_budget -= 1;
                    history_len += 1;
                }
                if total_budget < 0 || connect_budget < 0 || read_budget < 0 || status_budget < 0 {
                    return Err(error);
                }
                None
            }
        };
        let delay = server_delay
            .filter(|delay| !delay.is_zero())
            .unwrap_or_else(|| {
                if history_len <= 1 {
                    Duration::ZERO
                } else {
                    Duration::from_secs_f64((0.8 * 2_f64.powi(history_len - 1)).min(120.0))
                }
            });
        if !delay.is_zero() {
            ctx.runtime.record_retry_sleep();
            ctx.runtime.sleep(delay, ctx.cancel).await?;
        }
    }
}

fn redirect_allowed(method: &Method, status: u16) -> bool {
    if matches!(status, 301 | 302) {
        return matches!(method.as_str(), "GET" | "HEAD");
    }
    matches!(status, 300 | 303 | 307 | 308)
}

fn redirect_target(
    ctx: &ProviderContext<'_>,
    current: &str,
    location: &str,
) -> ProviderResult<reqwest::Url> {
    let current = reqwest::Url::parse(current)
        .map_err(|_| ctx.error(ErrorKind::Redirect, "Invalid Azure redirect URL"))?;
    // Azure 1.41.0 roots a relative Location at the origin. This deliberately
    // differs from RFC url.join for a target such as /prefix/translate + child.
    let next = if location.starts_with("//") {
        return Err(ctx.error(ErrorKind::Redirect, "Invalid Azure redirect URL"));
    } else if let Ok(url) = reqwest::Url::parse(location) {
        url
    } else {
        reqwest::Url::parse(&format!(
            "{}/{}",
            current.origin().ascii_serialization(),
            location.trim_start_matches('/')
        ))
        .map_err(|_| ctx.error(ErrorKind::Redirect, "Invalid Azure redirect URL"))?
    };
    if !matches!(next.scheme(), "http" | "https")
        || next.host_str().is_none()
        || !next.username().is_empty()
        || next.password().is_some()
    {
        return Err(ctx.error(ErrorKind::Redirect, "Invalid Azure redirect URL"));
    }
    Ok(next)
}

async fn redirect_send(
    ctx: &ProviderContext<'_>,
    mut req: HttpRequest,
) -> ProviderResult<HttpResponse> {
    for redirects in 0..=30 {
        let response = sdk_send(ctx, &req).await?;
        let Some(location) = response
            .header("location")
            .filter(|value| !value.is_empty())
        else {
            return Ok(response);
        };
        if !redirect_allowed(&req.method, response.status) {
            return Ok(response);
        }
        if redirects == 30 {
            return Err(ctx.error(ErrorKind::Redirect, "Azure redirect limit exceeded"));
        }
        let current = reqwest::Url::parse(&req.url)
            .map_err(|_| ctx.error(ErrorKind::Redirect, "Invalid Azure redirect URL"))?;
        let next = redirect_target(ctx, &req.url, location)?;
        // The old SDK removes Authorization for every redirect. On another
        // origin, additionally apply our shared transport's conservative
        // credential policy; never re-add the configured key at the new target.
        req.headers
            .retain(|key, _| !key.eq_ignore_ascii_case("authorization"));
        if current.origin() != next.origin() {
            req.headers.retain(|key, _| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "accept"
                        | "accept-encoding"
                        | "accept-language"
                        | "content-type"
                        | "content-length"
                        | "user-agent"
                )
            });
        }
        if response.status == 303 {
            req.method = Method::GET;
        }
        // The Azure redirect policy preserves the body even after 303 -> GET.
        req.url = next.to_string();
        req.query.clear();
    }
    unreachable!("bounded Azure redirect loop returns")
}

pub(super) async fn translate(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    ctx.retry(
        policy("azure.application", RetrySelector::AllExceptions),
        || async {
            let mut req = request(
                Method::POST,
                format!("{}/translate", ctx.config.endpoint.trim_end_matches('/')),
                // Preserve body=[text], exactly as the Python SDK serializes it.
                HttpBody::Json(json!([ctx.request.text])),
                Some(Duration::from_secs(300)),
            );
            req.headers = headers(&[
                ("Ocp-Apim-Subscription-Key", &ctx.config.api_key),
                ("Ocp-Apim-Subscription-Region", "chinaeast2"),
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
            ]);
            req.query = vec![
                ("api-version".into(), "3.0".into()),
                ("from".into(), mapped_language(language_in(ctx)).into()),
                ("to".into(), mapped_language(&ctx.config.lang_out).into()),
            ];
            let response = redirect_send(ctx, req).await?;
            if response.status != 200 {
                return Err(ctx.http_error(&response));
            }
            let value = response_json(ctx, &response)?;
            Ok(output(require_string(
                ctx,
                &value,
                "/0/translations/0/text",
            )?))
        },
    )
    .await
}
