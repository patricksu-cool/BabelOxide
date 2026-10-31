use crate::{ConfigError, ConfigErrorKind, EngineSpec, Result, Settings, positive_u32};
use pdf2zh_core::TranslatorConfig;
use serde_json::{Map, Value, json};

pub(crate) fn build(
    settings: &Settings,
    engine: &EngineSpec,
    term: bool,
) -> Result<TranslatorConfig> {
    let mut detail = settings.engine_detail(engine, term);
    let scope = if term && settings.selected_engine(true).is_some() {
        format!("term_{}", engine.flag)
    } else {
        engine.flag.clone()
    };
    let mut sdk_url = false;
    if let Some((field, value)) = settings.sdk_url_fallbacks.get(&scope)
        && detail.get(field).is_some_and(Value::is_null)
    {
        detail[field] = json!(value);
        sdk_url = true;
    }
    let t = settings.translation()?;
    let main_qps = if settings.main_engine().name == "SiliconFlowFree" {
        20
    } else {
        positive_u32(t.qps, "translation.qps")?
    };
    let qps = if term {
        t.term_qps
            .map(|v| positive_u32(v, "translation.term_qps"))
            .transpose()?
            .unwrap_or(main_qps)
    } else {
        main_qps
    };
    let mut options = match detail.get("core_options") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(object)) => object.clone(),
        _ => return Err(ConfigError::field("core_options")),
    };
    options.insert("qps".into(), json!(qps));
    options.insert("ignore_cache".into(), json!(t.ignore_cache));
    options.insert("lang_in".into(), json!(t.lang_in));
    let mut result = TranslatorConfig {
        provider: String::new(),
        endpoint: String::new(),
        api_key: String::new(),
        model: String::new(),
        timeout_seconds: 600.0,
        lang_out: t.lang_out,
        options: Value::Null,
    };
    match engine.name.as_str() {
        "OpenAI" | "OpenAICompatible" | "AliyunDashScope" => {
            let prefix = match engine.name.as_str() {
                "OpenAI" => "openai",
                "OpenAICompatible" => "openai_compatible",
                _ => "aliyun_dashscope",
            };
            result.provider = "openai".into();
            result.api_key = required(&detail, &format!("{prefix}_api_key"))?;
            result.model = required(&detail, &format!("{prefix}_model"))?;
            let key = format!("{prefix}_base_url");
            result.endpoint = if sdk_url {
                text(&detail, &key)
            } else {
                clean_url(&text(&detail, &key))
            };
            if (engine.name != "OpenAI" || detail.get(&key).is_some_and(|value| !value.is_null()))
                && result.endpoint.is_empty()
            {
                return Err(ConfigError::field(&key));
            }
            let key = format!("{prefix}_timeout");
            let timeout = optional(&detail, &key)
                .map(|v| positive_float(&v, &key))
                .transpose()?;
            result.timeout_seconds = timeout.unwrap_or(600.0);
            options.insert("timeout_explicit".into(), json!(timeout.is_some()));
            options.insert(
                "enable_json_mode".into(),
                json!(boolean(&detail, &format!("{prefix}_enable_json_mode"))),
            );
            let send_temperature = if prefix == "openai" {
                "openai_send_temprature".into()
            } else {
                format!("{prefix}_send_temperature")
            };
            if boolean(&detail, &send_temperature) {
                let key = format!("{prefix}_temperature");
                let value = required(&detail, &key)?
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| ConfigError::field(&key))?;
                options.insert("temperature".into(), json!(value));
            }
            if boolean(&detail, &format!("{prefix}_send_reasoning_effort")) {
                let key = format!("{prefix}_reasoning_effort");
                options.insert("reasoning_effort".into(), json!(required(&detail, &key)?));
            }
        }
        "DeepSeek" | "ModelScope" | "Zhipu" | "Gemini" | "Grok" | "Groq" => {
            let (prefix, endpoint) = match engine.name.as_str() {
                "DeepSeek" => ("deepseek", "https://api.deepseek.com/v1"),
                "ModelScope" => ("modelscope", "https://api-inference.modelscope.cn/v1"),
                "Zhipu" => ("zhipu", "https://open.bigmodel.cn/api/paas/v4"),
                "Gemini" => (
                    "gemini",
                    "https://generativelanguage.googleapis.com/v1beta/openai/",
                ),
                "Grok" => ("grok", "https://api.x.ai/v1"),
                _ => ("groq", "https://api.groq.com/openai/v1"),
            };
            result.provider = "openai".into();
            result.endpoint = endpoint.into();
            result.api_key = required(&detail, &format!("{prefix}_api_key"))?;
            result.model = required(&detail, &format!("{prefix}_model"))?;
            options.insert("timeout_explicit".into(), json!(false));
            options.insert(
                "enable_json_mode".into(),
                json!(boolean(&detail, &format!("{prefix}_enable_json_mode"))),
            );
            if prefix == "deepseek" {
                let mode = optional(&detail, "deepseek_thinking_mode");
                let reasoning = optional(&detail, "deepseek_reasoning_effort");
                if mode
                    .as_deref()
                    .is_some_and(|v| !matches!(v, "enabled" | "disabled"))
                {
                    return Err(ConfigError::field("deepseek_thinking_mode"));
                }
                if reasoning
                    .as_deref()
                    .is_some_and(|v| !v.is_empty() && !matches!(v, "high" | "max"))
                {
                    return Err(ConfigError::field("deepseek_reasoning_effort"));
                }
                if result.model.starts_with("deepseek-v4-")
                    && let Some(mode) = mode
                {
                    let extra = options.entry("extra_body").or_insert_with(|| json!({}));
                    let object = extra
                        .as_object_mut()
                        .ok_or_else(|| ConfigError::field("core_options.extra_body"))?;
                    object.insert("thinking".into(), json!({"type":mode}));
                    if mode == "enabled"
                        && let Some(reasoning) = reasoning.filter(|v| !v.is_empty())
                    {
                        options.insert("reasoning_effort".into(), json!(reasoning));
                    }
                }
            }
        }
        "AzureOpenAI" => {
            result.provider = "azure-openai".into();
            result.endpoint = required(&detail, "azure_openai_base_url")?;
            result.api_key = required(&detail, "azure_openai_api_key")?;
            result.model = required(&detail, "azure_openai_model")?;
            options.insert(
                "api_version".into(),
                json!(text(&detail, "azure_openai_api_version")),
            );
        }
        "SiliconFlow" => {
            result.provider = "siliconflow".into();
            result.endpoint = text(&detail, "siliconflow_base_url");
            reject_explicit_empty_url(&detail, "siliconflow_base_url", &result.endpoint)?;
            result.api_key = required(&detail, "siliconflow_api_key")?;
            result.model = required(&detail, "siliconflow_model")?;
            for (source, target) in [
                ("siliconflow_enable_json_mode", "enable_json_mode"),
                ("siliconflow_enable_thinking", "enable_thinking"),
                (
                    "siliconflow_send_enable_thinking_param",
                    "send_enable_thinking_param",
                ),
            ] {
                options.insert(target.into(), json!(boolean(&detail, source)));
            }
        }
        "SiliconFlowFree" => {
            result.provider = "siliconflowfree".into();
            options.insert(
                "enable_json_mode".into(),
                json!(boolean(&detail, "siliconflow_free_enable_json_mode")),
            );
        }
        "QwenMt" => {
            result.provider = "qwen-mt".into();
            result.endpoint = text(&detail, "qwenmt_base_url");
            reject_explicit_empty_url(&detail, "qwenmt_base_url", &result.endpoint)?;
            result.api_key = required(&detail, "qwenmt_api_key")?;
            result.model = required(&detail, "qwenmt_model")?;
            options.insert(
                "ali_domains".into(),
                detail.get("ali_domains").cloned().unwrap_or(Value::Null),
            );
        }
        "Ollama" => {
            result.provider = "ollama".into();
            result.endpoint = required(&detail, "ollama_host")?;
            result.model = text(&detail, "ollama_model");
            let predict = detail
                .get("num_predict")
                .and_then(Value::as_i64)
                .ok_or_else(|| ConfigError::field("num_predict"))?;
            options.insert("num_predict".into(), json!(predict.max(0)));
        }
        "Xinference" => {
            result.provider = "xinference".into();
            result.endpoint = required(&detail, "xinference_host")?;
            result.model = text(&detail, "xinference_model");
        }
        "Google" => result.provider = "google".into(),
        "Bing" => result.provider = "bing".into(),
        "DeepL" => {
            result.provider = "deepl".into();
            result.api_key = required(&detail, "deepl_auth_key")?;
        }
        "Azure" => {
            result.provider = "azure".into();
            result.endpoint = required(&detail, "azure_endpoint")?;
            result.api_key = required(&detail, "azure_api_key")?;
        }
        "AnythingLLM" => {
            result.provider = "anythingllm".into();
            result.endpoint = required(&detail, "anythingllm_url")?;
            result.api_key = required(&detail, "anythingllm_apikey")?;
        }
        "Dify" => {
            result.provider = "dify".into();
            result.endpoint = required(&detail, "dify_url")?;
            result.api_key = required(&detail, "dify_apikey")?;
        }
        "TencentMechineTranslation" => {
            let mut id = text(&detail, "tencentcloud_secret_id");
            let mut key = text(&detail, "tencentcloud_secret_key");
            if id.is_empty() || key.is_empty() {
                return Err(ConfigError::new(
                    ConfigErrorKind::UnsupportedCredentialSource,
                ));
            }
            let env = &settings.tencent_environment;
            let env_id = env
                .get("TENCENTCLOUD_SECRET_ID")
                .filter(|value| !value.is_empty());
            let env_key = env
                .get("TENCENTCLOUD_SECRET_KEY")
                .filter(|value| !value.is_empty());
            if let (Some(env_id), Some(env_key)) = (env_id, env_key) {
                if env_id.trim() != env_id || env_key.trim() != env_key {
                    return Err(ConfigError::field("Tencent environment credentials"));
                }
                id = env_id.clone();
                key = env_key.clone();
                // Tencent's environment provider does not consume a token.
                options.remove("tencentcloud_token");
            } else if [
                "TKE_REGION",
                "TKE_PROVIDER_ID",
                "TKE_WEB_IDENTITY_TOKEN_FILE",
                "TKE_ROLE_ARN",
            ]
            .iter()
            .all(|name| env.get(*name).is_some_and(|value| !value.is_empty()))
            {
                return Err(ConfigError::new(
                    ConfigErrorKind::UnsupportedCredentialSource,
                ));
            }
            result.provider = "tencent".into();
            result.api_key = key;
            options.insert("tencentcloud_secret_id".into(), json!(id));
        }
        "CLITranslator" => {
            result.provider = "clitranslator".into();
            options.insert(
                "clitranslator_command".into(),
                json!(required(&detail, "clitranslator_command")?),
            );
            let timeout = detail
                .get("clitranslator_timeout")
                .and_then(Value::as_i64)
                .filter(|v| (1..=300).contains(v))
                .ok_or_else(|| ConfigError::field("clitranslator_timeout"))?;
            options.insert("clitranslator_timeout".into(), json!(timeout));
            if let Some(command) = optional(&detail, "clitranslator_postprocess_command") {
                if command.is_empty() {
                    return Err(ConfigError::field("clitranslator_postprocess_command"));
                }
                options.insert("clitranslator_postprocess_command".into(), json!(command));
            }
        }
        "ClaudeCode" => {
            result.provider = "claudecode".into();
            result.model = text(&detail, "claude_code_model");
            options.insert(
                "claude_code_path".into(),
                json!(required(&detail, "claude_code_path")?),
            );
            options.insert("claude_code_model".into(), json!(result.model));
        }
        _ => return Err(ConfigError::field("translate_engine_type")),
    }
    // Explicit extension for offline endpoints / deployments lacking an old
    // provider URL field; never infer it from machine environment or a URL key.
    if let Some(endpoint) = detail.get("core_endpoint").and_then(Value::as_str) {
        result.endpoint = endpoint.trim().into();
    }
    result.options = Value::Object(options);
    Ok(result)
}

fn text(detail: &Value, key: &str) -> String {
    optional(detail, key).unwrap_or_default()
}
fn optional(detail: &Value, key: &str) -> Option<String> {
    detail
        .get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
}
fn required(detail: &Value, key: &str) -> Result<String> {
    optional(detail, key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ConfigError::field(key))
}
fn boolean(detail: &Value, key: &str) -> bool {
    detail.get(key).and_then(Value::as_bool).unwrap_or(false)
}
fn positive_float(value: &str, key: &str) -> Result<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| ConfigError::field(key))
}
fn clean_url(value: &str) -> String {
    let value = value.trim().trim_end_matches('/');
    value
        .strip_suffix("/chat/completions")
        .unwrap_or(value)
        .trim_end_matches('/')
        .to_owned()
}
fn reject_explicit_empty_url(detail: &Value, key: &str, value: &str) -> Result<()> {
    if value.is_empty() && detail.get(key).is_some_and(|value| !value.is_null()) {
        Err(ConfigError::field(key))
    } else {
        Ok(())
    }
}
