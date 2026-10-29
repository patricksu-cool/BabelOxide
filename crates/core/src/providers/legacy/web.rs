use super::*;
use regex::Regex;
use serde_json::json;
use unicode_general_category::{GeneralCategory, get_general_category};

const GOOGLE_UA: &str = "Mozilla/4.0 (compatible;MSIE 6.0;Windows NT 5.1;SV1;.NET CLR 1.1.4322;.NET CLR 2.0.50727;.NET CLR 3.0.04506.30)";
const BING_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0";

// HTML5 permits these historical named references without a semicolon. The
// complete named table is supplied by html-escape, whose decoder deliberately
// omits this rule and the numeric C1 replacement rule used by Python's html.
const SEMICOLONLESS: &[&str] = &[
    "AElig", "AMP", "Aacute", "Acirc", "Agrave", "Aring", "Atilde", "Auml", "COPY", "Ccedil",
    "ETH", "Eacute", "Ecirc", "Egrave", "Euml", "GT", "Iacute", "Icirc", "Igrave", "Iuml", "LT",
    "Ntilde", "Oacute", "Ocirc", "Ograve", "Oslash", "Otilde", "Ouml", "QUOT", "REG", "THORN",
    "Uacute", "Ucirc", "Ugrave", "Uuml", "Yacute", "aacute", "acirc", "acute", "aelig", "agrave",
    "amp", "aring", "atilde", "auml", "brvbar", "ccedil", "cedil", "cent", "copy", "curren", "deg",
    "divide", "eacute", "ecirc", "egrave", "eth", "euml", "frac12", "frac14", "frac34", "gt",
    "iacute", "icirc", "iexcl", "igrave", "iquest", "iuml", "laquo", "lt", "macr", "micro",
    "middot", "nbsp", "not", "ntilde", "oacute", "ocirc", "ograve", "ordf", "ordm", "oslash",
    "otilde", "ouml", "para", "plusmn", "pound", "quot", "raquo", "reg", "sect", "shy", "sup1",
    "sup2", "sup3", "szlig", "thorn", "times", "uacute", "ucirc", "ugrave", "uml", "uuml",
    "yacute", "yen", "yuml",
];

fn named_reference(name: &str) -> Option<&'static str> {
    if let Ok(index) =
        super::html5::COMPOUND_REFERENCES.binary_search_by(|(candidate, _)| candidate.cmp(&name))
    {
        return Some(super::html5::COMPOUND_REFERENCES[index].1);
    }
    html_escape::NAMED_ENTITIES
        .binary_search_by(|(candidate, _)| candidate.cmp(&name.as_bytes()))
        .ok()
        .map(|index| html_escape::NAMED_ENTITIES[index].1)
}

fn numeric_reference(reference: &str) -> String {
    const C1: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    let digits = reference.trim_end_matches(';');
    let number = if digits.starts_with("#x") || digits.starts_with("#X") {
        u32::from_str_radix(&digits[2..], 16)
    } else {
        digits[1..].parse::<u32>()
    }
    .unwrap_or(u32::MAX);
    if (0x80..=0x9f).contains(&number) {
        return C1[(number - 0x80) as usize].to_string();
    }
    if number == 0 || (0xd800..=0xdfff).contains(&number) || number > 0x10ffff {
        return "\u{fffd}".into();
    }
    if matches!(number, 1..=8 | 0xb | 0xe..=0x1f | 0x7f)
        || (0xfdd0..=0xfdef).contains(&number)
        || number & 0xffff >= 0xfffe
    {
        return String::new();
    }
    char::from_u32(number)
        .expect("validated Unicode scalar")
        .to_string()
}

fn unescape_html(text: &str) -> String {
    static REFERENCE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\x0c <&#;]{1,32};?)")
            .expect("constant HTML5 character reference pattern")
    });
    REFERENCE
        .replace_all(text, |capture: &regex::Captures<'_>| {
            let reference = &capture[1];
            if reference.starts_with('#') {
                return numeric_reference(reference);
            }
            if let Some(name) = reference.strip_suffix(';') {
                if let Some(decoded) = named_reference(name) {
                    return decoded.to_owned();
                }
            } else if SEMICOLONLESS.contains(&reference) {
                return named_reference(reference)
                    .expect("HTML5 legacy reference")
                    .to_owned();
            }
            for (index, _) in reference.char_indices().rev() {
                if index < 2 {
                    break;
                }
                let name = &reference[..index];
                if SEMICOLONLESS.contains(&name) {
                    return format!(
                        "{}{}",
                        named_reference(name).expect("HTML5 legacy reference"),
                        &reference[index..]
                    );
                }
            }
            capture[0].to_owned()
        })
        .into_owned()
}

fn mapped_language<'a>(provider: &str, lang: &'a str) -> &'a str {
    match (provider, lang.to_ascii_lowercase().as_str()) {
        ("google", "zh") => "zh-CN",
        ("bing", "zh" | "zh-cn") => "zh-Hans",
        ("bing", "zh-tw") => "zh-Hant",
        ("bing", "auto") => "en",
        _ => lang,
    }
}

pub(super) async fn google(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    let pattern = Regex::new(r#"(?s)class="(?:t0|result-container)">(.*?)<"#)
        .expect("constant Google pattern");
    ctx.retry(
        policy("google.application", RetrySelector::AllExceptions),
        || async {
            let mut req = request(
                Method::GET,
                endpoint(ctx, "https://translate.google.com/m"),
                HttpBody::Empty,
                None,
            );
            req.headers = headers(&[("User-Agent", GOOGLE_UA)]);
            req.query = vec![
                (
                    "tl".into(),
                    mapped_language("google", &ctx.config.lang_out).into(),
                ),
                (
                    "sl".into(),
                    mapped_language("google", language_in(ctx)).into(),
                ),
                ("q".into(), ctx.request.text.chars().take(5000).collect()),
            ];
            let response = ctx.http.send(req, ctx.cancel).await?;
            if response.status == 400 {
                return Ok(output("IRREPARABLE TRANSLATION ERROR"));
            }
            check_status(ctx, &response)?;
            let body = String::from_utf8_lossy(&response.body);
            let captures = pattern.captures(&body).ok_or_else(|| {
                ctx.error(
                    ErrorKind::MalformedResponse,
                    "Google response has no translation container",
                )
            })?;
            let decoded = unescape_html(captures.get(1).unwrap().as_str());
            let text: String = decoded
                .chars()
                .filter(|ch| {
                    !matches!(
                        get_general_category(*ch),
                        GeneralCategory::Control
                            | GeneralCategory::Format
                            | GeneralCategory::PrivateUse
                            | GeneralCategory::Surrogate
                            | GeneralCategory::Unassigned
                    )
                })
                .collect();
            Ok(output(text))
        },
    )
    .await
}

fn sid_capture(
    ctx: &ProviderContext<'_>,
    pattern: &str,
    body: &str,
    last: bool,
) -> ProviderResult<Vec<String>> {
    let regex = Regex::new(pattern).expect("constant Bing pattern");
    let mut captures = regex.captures_iter(body);
    let capture = if last {
        captures.last()
    } else {
        captures.next()
    };
    let capture = capture.ok_or_else(|| {
        ctx.error(
            ErrorKind::MalformedResponse,
            "Bing session response is incomplete",
        )
    })?;
    Ok(capture
        .iter()
        .skip(1)
        .map(|item| item.unwrap().as_str().to_owned())
        .collect())
}

pub(super) async fn bing(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    ctx.retry(
        policy("bing.application", RetrySelector::Predicate),
        || async {
            // SID is acquired again on every application attempt.
            let req = request(
                Method::GET,
                endpoint(ctx, "https://www.bing.com/translator"),
                HttpBody::Empty,
                None,
            );
            let session = ctx.http.send(req, ctx.cancel).await?;
            check_status(ctx, &session)?;
            let body = String::from_utf8_lossy(&session.body);
            let ig = sid_capture(ctx, r#""ig":"(.*?)""#, &body, false)?;
            let iid = sid_capture(ctx, r#"data-iid="(.*?)""#, &body, true)?;
            let params = sid_capture(
                ctx,
                r#"params_AbusePreventionHelper\s=\s\[(.*?),"(.*?)","#,
                &body,
                false,
            )?;
            // The Python implementation slices the final response URL, including
            // any regional host selected by a redirect.
            let base: String = session
                .final_url
                .chars()
                .take(session.final_url.chars().count().saturating_sub(10))
                .collect();
            let mut req = request(
                Method::POST,
                format!("{base}ttranslatev3"),
                HttpBody::Form(vec![
                    (
                        "fromLang".into(),
                        mapped_language("bing", language_in(ctx)).into(),
                    ),
                    (
                        "to".into(),
                        mapped_language("bing", &ctx.config.lang_out).into(),
                    ),
                    ("text".into(), ctx.request.text.chars().take(1000).collect()),
                    ("token".into(), params[1].clone()),
                    ("key".into(), params[0].clone()),
                ]),
                None,
            );
            req.query = vec![("IG".into(), ig[0].clone()), ("IID".into(), iid[0].clone())];
            req.headers = headers(&[("User-Agent", BING_UA)]);
            let response = ctx.http.send(req, ctx.cancel).await?;
            check_status(ctx, &response)?;
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

pub(super) async fn anythingllm(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    ctx.retry(
        policy("anythingllm.application", RetrySelector::AllExceptions),
        || async {
            let mut req = request(
                Method::POST,
                ctx.config.endpoint.clone(),
                HttpBody::Json(json!({
                    "message": [{"role": "user", "content": ctx.prompt(&ctx.request.text)}],
                    "mode": "chat",
                    "sessionId": "translation_expert"
                })),
                Some(Duration::from_secs(60)),
            );
            req.headers = headers(&[
                ("accept", "application/json"),
                ("Authorization", &format!("Bearer {}", ctx.config.api_key)),
                ("Content-Type", "application/json"),
            ]);
            let response = ctx.http.send(req, ctx.cancel).await?;
            check_status(ctx, &response)?;
            let value = response_json(ctx, &response)?;
            // Rust's typed output cannot represent Python's implicit None when the
            // field is absent. Treat it as a malformed response, never empty success.
            Ok(output(require_string(ctx, &value, "/textResponse")?.trim()))
        },
    )
    .await
}

pub(super) async fn dify(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    ctx.retry(
        policy("dify.application", RetrySelector::AllExceptions),
        || async {
            let mut req = request(
                Method::POST,
                ctx.config.endpoint.clone(),
                HttpBody::Json(json!({
                    "inputs": {
                        "lang_out": ctx.config.lang_out,
                        "lang_in": language_in(ctx),
                        "text": ctx.request.text,
                    },
                    "response_mode": "blocking",
                    "user": "translator-service",
                })),
                Some(Duration::from_secs(60)),
            );
            req.headers = headers(&[
                ("Authorization", &format!("Bearer {}", ctx.config.api_key)),
                ("Content-Type", "application/json"),
            ]);
            let response = ctx.http.send(req, ctx.cancel).await?;
            check_status(ctx, &response)?;
            let value = response_json(ctx, &response)?;
            // Unlike AnythingLLM, Dify does not trim the returned string.
            Ok(output(require_string(ctx, &value, "/data/outputs/text")?))
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_html_unescape_matches_python_html5_rules() {
        assert_eq!(
            unescape_html(
                "&amp &notit; &NotEqualTilde; &#128; &#0; &#xD800; &#1114112; &#xFDD0; &#99999999999999999999999;"
            ),
            "& ¬it; ≂\u{338} € � � �  �"
        );
        assert_eq!(
            unescape_html("&unknown; &amp;lt; &#65x &#x41z &COPY &#+65;"),
            "&unknown; &lt; Ax Az © &#+65;"
        );
    }
}
