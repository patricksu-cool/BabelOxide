use super::*;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::time::UNIX_EPOCH;

fn mapped_language(lang: &str) -> &str {
    match lang.to_ascii_lowercase().as_str() {
        "zh-cn" => "zh",
        "zh-tw" | "zh-hk" => "zh-TW",
        _ => lang,
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").expect("write to String");
    }
    result
}

fn hmac(key: &[u8], message: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC supports arbitrary key length");
    mac.update(message.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

// Python json.dumps uses ensure_ascii=True and spaces after separators. This
// matters because the TC3 signature hashes the exact bytes sent on the wire.
fn python_json_string(value: &str) -> String {
    let mut output = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            ch if (' '..='~').contains(&ch) => output.push(ch),
            ch => {
                let mut utf16 = [0; 2];
                for unit in ch.encode_utf16(&mut utf16) {
                    write!(&mut output, "\\u{unit:04x}").expect("write to String");
                }
            }
        }
    }
    output.push('"');
    output
}

fn request_body(text: &str, source: &str, target: &str) -> String {
    format!(
        "{{\"SourceText\": {}, \"Source\": {}, \"Target\": {}, \"ProjectId\": 0}}",
        python_json_string(text),
        python_json_string(source),
        python_json_string(target),
    )
}

// Gregorian civil date from days since 1970-01-01; timestamps come from the
// injected wall clock, never the monotonic retry clock.
fn utc_date(timestamp: u64) -> String {
    let days = (timestamp / 86400) as i64 + 719468;
    let era = days.div_euclid(146097);
    let day_of_era = days - era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn authorization(
    secret_id: &str,
    secret_key: &str,
    host: &str,
    path: &str,
    timestamp: u64,
    body: &str,
) -> String {
    let date = utc_date(timestamp);
    let body_hash = hex(&Sha256::digest(body.as_bytes()));
    let canonical_request = format!(
        "POST\n{path}\n\ncontent-type:application/json\nhost:{host}\n\ncontent-type;host\n{body_hash}"
    );
    let credential_scope = format!("{date}/tmt/tc3_request");
    let to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes())),
    );
    let date_key = hmac(format!("TC3{secret_key}").as_bytes(), &date);
    let service_key = hmac(&date_key, "tmt");
    let signing_key = hmac(&service_key, "tc3_request");
    format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{credential_scope}, SignedHeaders=content-type;host, Signature={}",
        hex(&hmac(&signing_key, &to_sign)),
    )
}

pub(super) async fn translate(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    let secret_id = option_string(ctx, "tencentcloud_secret_id")
        .or_else(|| option_string(ctx, "secret_id"))
        .unwrap_or("");
    if secret_id.trim().is_empty()
        || secret_id.trim() != secret_id
        || ctx.config.api_key.trim().is_empty()
        || ctx.config.api_key.trim() != ctx.config.api_key
    {
        return Err(ctx.error(
            ErrorKind::InvalidConfiguration,
            "Tencent credentials are invalid",
        ));
    }
    ctx.retry(
        policy("tencent.application", RetrySelector::Predicate),
        || async {
            let url = reqwest::Url::parse(&endpoint(ctx, "https://tmt.tencentcloudapi.com/"))
                .map_err(|_| {
                    ctx.error(ErrorKind::InvalidConfiguration, "Invalid Tencent endpoint")
                })?;
            let host = url.host_str().ok_or_else(|| {
                ctx.error(
                    ErrorKind::InvalidConfiguration,
                    "Tencent endpoint has no host",
                )
            })?;
            let host = match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            };
            let timestamp = ctx
                .runtime
                .wall_time()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| {
                    ctx.error(
                        ErrorKind::Value,
                        "Tencent signing clock is before the Unix epoch",
                    )
                })?
                .as_secs();
            let body = request_body(
                &ctx.request.text,
                mapped_language(language_in(ctx)),
                mapped_language(&ctx.config.lang_out),
            );
            let auth = authorization(
                secret_id,
                &ctx.config.api_key,
                &host,
                url.path(),
                timestamp,
                &body,
            );
            let mut req = request(
                Method::POST,
                url.to_string(),
                HttpBody::Bytes(body.into_bytes()),
                Some(Duration::from_secs(60)),
            );
            req.headers = headers(&[
                ("Authorization", &auth),
                ("Host", &host),
                ("Content-Type", "application/json"),
                ("X-TC-Action", "TextTranslate"),
                ("X-TC-Version", "2018-03-21"),
                ("X-TC-Region", "ap-beijing"),
                ("X-TC-Language", "zh-CN"),
                ("X-TC-Timestamp", &timestamp.to_string()),
                ("X-TC-RequestClient", "SDK_PYTHON_3.1.121"),
            ]);
            if let Some(token) = option_string(ctx, "tencentcloud_token")
                && !token.is_empty()
            {
                req.headers.insert("X-TC-Token".into(), token.into());
            }
            // common 3.1.192 uses NoopRetryer by default: precisely one send per
            // application attempt, including errors carried by a 200 response.
            let response = ctx.http.send(req, ctx.cancel).await?;
            if response.status != 200 {
                return Err(ctx.http_error(&response));
            }
            let value = response_json(ctx, &response)?;
            if let Some(error) = value.pointer("/Response/Error") {
                let code = error
                    .get("Code")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown");
                let message = error
                    .get("Message")
                    .and_then(Value::as_str)
                    .unwrap_or("Tencent API error");
                return Err(ctx.error(ErrorKind::Provider, format!("Tencent {code}: {message}")));
            }
            Ok(output(require_string(ctx, &value, "/Response/TargetText")?))
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_payload_matches_python_json_dumps() {
        assert_eq!(
            request_body("a\n\"繁😀", "en", "zh-TW"),
            r#"{"SourceText": "a\n\"\u7e41\ud83d\ude00", "Source": "en", "Target": "zh-TW", "ProjectId": 0}"#,
        );
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(951782400), "2000-02-29");
        // Independent stdlib Python hashlib/hmac fixture, using only fake keys.
        assert!(
            authorization(
                "fixture-id",
                "fixture-key",
                "tmt.tencentcloudapi.com",
                "/",
                1_700_000_000,
                &request_body("繁😀", "en", "zh-TW"),
            )
            .ends_with(
                "Signature=2d68b2a3fbcf923eab901d3ff73abbc4ae6fc715046f21b0aba5c39a372ccf10"
            )
        );
    }
}
