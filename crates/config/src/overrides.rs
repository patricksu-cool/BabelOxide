use crate::schema::{lookup, put};
use crate::{ConfigError, FieldSpec, FieldType, Result, engine_specs, field_specs};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Matches Python's PDF2ZH_ field names. Invalid environment values are ignored,
/// as in ConfigManager.parse_env_vars; neither values nor parser errors escape.
pub fn parse_env_vars(values: &BTreeMap<String, String>) -> Value {
    let normalized: BTreeMap<_, _> = values
        .iter()
        .map(|(key, value)| (key.replace('-', "_").to_ascii_uppercase(), value))
        .collect();
    let mut result = json!({});
    for spec in field_specs() {
        if let Some(value) = normalized.get(&spec.env)
            && let Ok(value) = coerce(spec, &Value::String((*value).clone()), true)
        {
            put(&mut result, &spec.path, value);
        }
    }
    result
}

/// Accepts the legacy flat CLI names as well as already nested section values.
/// Only explicitly supplied values belong here; clap defaults must not override
/// a file or environment value.
pub fn parse_cli_overrides(values: &Value) -> Result<Value> {
    let object = values
        .as_object()
        .ok_or_else(|| ConfigError::new(crate::ConfigErrorKind::InvalidDocument))?;
    let mut result = json!({});
    for (key, value) in object {
        let normalized = key.replace('-', "_").to_ascii_lowercase();
        if let Some(spec) = field_specs()
            .iter()
            .find(|spec| spec.flag.replace('-', "_") == normalized || spec.path == normalized)
        {
            put(&mut result, &spec.path, coerce(spec, value, false)?);
        } else {
            result[&normalized] = value.clone();
        }
    }
    normalize_known_types(&mut result)?;
    Ok(result)
}

pub(crate) fn normalize_known_types(value: &mut Value) -> Result<()> {
    for spec in field_specs() {
        if let Some(current) = lookup(value, &spec.path).cloned() {
            put(value, &spec.path, coerce(spec, &current, false)?);
        }
    }
    Ok(())
}

pub(crate) fn coerce(spec: &FieldSpec, value: &Value, env: bool) -> Result<Value> {
    let invalid = || ConfigError::field(&spec.path);
    if value.is_null() {
        return if spec.nullable {
            Ok(Value::Null)
        } else {
            Err(invalid())
        };
    }
    match spec.kind {
        FieldType::String => value.as_str().map(|s| json!(s)).ok_or_else(invalid),
        FieldType::Bool => match value {
            Value::Bool(v) => Ok(json!(v)),
            Value::String(v) => {
                let v = v.to_ascii_lowercase();
                if env {
                    return Ok(json!(matches!(
                        v.as_str(),
                        "true" | "1" | "yes" | "y" | "on"
                    )));
                }
                if env
                    || matches!(
                        v.as_str(),
                        "true"
                            | "false"
                            | "1"
                            | "0"
                            | "yes"
                            | "no"
                            | "y"
                            | "n"
                            | "on"
                            | "off"
                            | "t"
                            | "f"
                    )
                {
                    Ok(json!(matches!(
                        v.as_str(),
                        "true" | "1" | "yes" | "y" | "on" | "t"
                    )))
                } else {
                    Err(invalid())
                }
            }
            Value::Number(v) if v.as_i64() == Some(0) || v.as_i64() == Some(1) => {
                Ok(json!(v.as_i64() == Some(1)))
            }
            _ => Err(invalid()),
        },
        FieldType::Integer => {
            let integer = value
                .as_i64()
                .or_else(|| value.as_str().and_then(|v| clean_numeric(v).parse().ok()))
                .or_else(|| {
                    value
                        .as_f64()
                        .filter(|v| {
                            v.is_finite()
                                && v.fract() == 0.0
                                && *v >= i64::MIN as f64
                                && *v < i64::MAX as f64
                        })
                        .map(|v| v as i64)
                });
            integer.map(|v| json!(v)).ok_or_else(invalid)
        }
        FieldType::Float => value
            .as_f64()
            .or_else(|| value.as_str().and_then(|v| clean_numeric(v).parse().ok()))
            .filter(|v| v.is_finite())
            .map(|v| json!(v))
            .ok_or_else(invalid),
        FieldType::StringSet => {
            let parsed = if let Some(v) = value.as_str() {
                parse_string_set(v).ok_or_else(invalid)?
            } else {
                value.clone()
            };
            let items = parsed.as_array().ok_or_else(invalid)?;
            if items.iter().any(|v| !v.is_string()) {
                return Err(invalid());
            }
            let dedup: std::collections::BTreeSet<_> =
                items.iter().filter_map(Value::as_str).collect();
            Ok(json!(dedup))
        }
    }
}

fn clean_numeric(value: &str) -> String {
    // Python int/float accept underscores between digits, not doubled or at an edge.
    let chars: Vec<_> = value.trim().chars().collect();
    if chars.iter().enumerate().any(|(i, c)| {
        *c == '_'
            && (i == 0
                || i + 1 == chars.len()
                || !chars[i - 1].is_ascii_digit()
                || !chars[i + 1].is_ascii_digit())
    }) {
        return String::new();
    }
    chars.into_iter().filter(|c| *c != '_').collect()
}

fn parse_string_set(text: &str) -> Option<Value> {
    let text = text.trim();
    if matches!(text, "None" | "set()") {
        return Some(json!([]));
    }
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return value.is_array().then_some(value);
    }
    let mut chars = text.chars().peekable();
    let closing = match chars.next()? {
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    let mut values = Vec::new();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek() == Some(&closing) {
            chars.next();
            break;
        }
        let quote = chars.next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        let mut value = String::new();
        loop {
            let c = chars.next()?;
            if c == quote {
                break;
            }
            if c != '\\' {
                value.push(c);
                continue;
            }
            let escaped = chars.next()?;
            match escaped {
                '\\' | '\'' | '"' => value.push(escaped),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                'a' => value.push('\x07'),
                'b' => value.push('\x08'),
                'f' => value.push('\x0c'),
                'v' => value.push('\x0b'),
                '\n' => {}
                'x' | 'u' | 'U' => {
                    let count = match escaped {
                        'x' => 2,
                        'u' => 4,
                        _ => 8,
                    };
                    let hex: String = (0..count).map(|_| chars.next()).collect::<Option<_>>()?;
                    value.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                '0'..='7' => {
                    let mut octal = String::from(escaped);
                    for _ in 0..2 {
                        if chars.peek().is_some_and(|c| matches!(c, '0'..='7')) {
                            octal.push(chars.next()?);
                        } else {
                            break;
                        }
                    }
                    value.push(char::from_u32(u32::from_str_radix(&octal, 8).ok()?)?);
                }
                other => {
                    value.push('\\');
                    value.push(other);
                }
            }
        }
        values.push(value);
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        match chars.next()? {
            ',' => {}
            c if c == closing => break,
            _ => return None,
        }
    }
    if chars.any(|c| !c.is_whitespace()) {
        return None;
    }
    Some(json!(values))
}

pub(crate) fn deep_merge(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                if let Some(old) = target.get_mut(key) {
                    deep_merge(old, value);
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        (Value::Array(target), Value::Array(source)) => target.extend(source.iter().cloned()),
        (target, source) => *target = source.clone(),
    }
}

pub(crate) fn select_flag(layer: &Value, term: bool) -> Option<String> {
    engine_specs()
        .iter()
        .filter(|e| !term || e.supports_llm)
        .find_map(|engine| {
            let flag = if term {
                format!("term_{}", engine.flag)
            } else {
                engine.flag.clone()
            };
            let enabled = layer.get(&flag).and_then(|value| {
                field_specs()
                    .iter()
                    .find(|spec| spec.path == flag)
                    .and_then(|spec| coerce(spec, value, false).ok())
                    .and_then(|value| value.as_bool())
            }) == Some(true);
            enabled.then_some(flag)
        })
}
