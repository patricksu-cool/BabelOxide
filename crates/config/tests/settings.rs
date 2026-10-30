use pdf2zh_config::{
    ConfigErrorKind, FieldType, Settings, engine_specs, field_specs, parse_cli_overrides,
    parse_env_vars,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn chosen(name: &str, detail: Value) -> Settings {
    let engine = engine_specs()
        .iter()
        .find(|engine| engine.name == name)
        .unwrap();
    let mut value = json!({engine.flag.clone():true});
    if let Some(key) = &engine.detail_key {
        value[key] = detail;
    }
    Settings::from_value(value).unwrap()
}

#[test]
fn one_schema_exposes_legacy_cli_env_types_defaults_and_sensitive_fields() {
    let fields = field_specs();
    assert_eq!(fields.len(), 235);
    assert_eq!(engine_specs().len(), 24);
    let unique_flags: BTreeSet<_> = fields.iter().map(|field| &field.flag).collect();
    let unique_env: BTreeSet<_> = fields.iter().map(|field| &field.env).collect();
    assert_eq!(unique_flags.len(), fields.len());
    assert_eq!(unique_env.len(), fields.len());
    for (flag, path, kind, default, secret) in [
        (
            "qps",
            "translation.qps",
            FieldType::Integer,
            json!(4),
            false,
        ),
        (
            "translate-table-text",
            "pdf.translate_table_text",
            FieldType::Bool,
            json!(true),
            false,
        ),
        (
            "openai-send-temprature",
            "openai_detail.openai_send_temprature",
            FieldType::Bool,
            Value::Null,
            false,
        ),
        (
            "term-openai-api-key",
            "term_openai_detail.term_openai_api_key",
            FieldType::String,
            Value::Null,
            true,
        ),
        (
            "azure-endpoint",
            "azure_detail.azure_endpoint",
            FieldType::String,
            json!("https://api.translator.azure.cn"),
            true,
        ),
    ] {
        let field = fields.iter().find(|field| field.flag == flag).unwrap();
        assert_eq!(
            (&*field.path, field.kind, &field.default, field.secret),
            (path, kind, &default, secret)
        );
    }
    assert!(
        !fields
            .iter()
            .any(|field| field.flag == "term-qwenmt" || field.flag == "term-claudecode")
    );
    assert_eq!(engine_specs()[0].name, "SiliconFlowFree");
}

#[test]
fn environment_conversion_preserves_python_boolean_and_literal_set_behavior() {
    let env = BTreeMap::from([
        ("PDF2ZH_QPS".into(), "8".into()),
        ("PDF2ZH_REPORT_INTERVAL".into(), "0.2".into()),
        ("PDF2ZH_DEBUG".into(), "yes".into()),
        ("PDF2ZH_TRANSLATE_TABLE_TEXT".into(), "not truthy".into()),
        (
            "PDF2ZH_INPUT_FILES".into(),
            r#"{'two.pdf', 'one.pdf', 'two.pdf', 'line\nname.pdf',}"#.into(),
        ),
        ("PDF2ZH_OPENAI".into(), "1".into()),
        ("PDF2ZH_OPENAI_API_KEY".into(), "null".into()),
        (
            "PDF2ZH_SERVER_PORT".into(),
            "invalid synthetic private text".into(),
        ),
        ("UNRELATED_SECRET".into(), "never imported".into()),
    ]);
    let parsed = parse_env_vars(&env);
    assert_eq!(parsed["translation"]["qps"], 8);
    assert_eq!(parsed["report_interval"], 0.2);
    assert_eq!(parsed["basic"]["debug"], true);
    assert_eq!(parsed["pdf"]["translate_table_text"], false);
    assert_eq!(
        parsed["basic"]["input_files"],
        json!(["line\nname.pdf", "one.pdf", "two.pdf"])
    );
    assert_eq!(parsed["openai_detail"]["openai_api_key"], "null");
    assert!(parsed.get("gui_settings").is_none());
    assert!(parsed.get("UNRELATED_SECRET").is_none());
    let t = parse_env_vars(&BTreeMap::from([
        ("PDF2ZH_DEBUG".into(), "t".into()),
        ("PDF2ZH_INPUT_FILES".into(), "None".into()),
    ]));
    assert_eq!(t["basic"]["debug"], false);
    assert_eq!(t["basic"]["input_files"], json!([]));
}

#[test]
fn highest_priority_engine_and_term_selection_do_not_follow_a_lower_true_flag() {
    let cli=parse_cli_overrides(&json!({"bing":true,"qps":3,"input_files":["cli.pdf"],"term_openai":true,"term_openai_api_key":"synthetic term"})).unwrap();
    let env = parse_env_vars(&BTreeMap::from([
        ("PDF2ZH_GOOGLE".into(), "true".into()),
        ("PDF2ZH_QPS".into(), "8".into()),
    ]));
    let file = json!({"siliconflowfree":true,"translation":{"qps":12},"basic":{"input_files":["file.pdf"]},"future":{"keep":[1]}});
    let settings = Settings::from_layers(&[cli, env, file]).unwrap();
    assert_eq!(settings.main_engine().name, "Bing");
    assert_eq!(settings.term_engine().unwrap().name, "OpenAI");
    assert_eq!(settings.translation().unwrap().qps, 3);
    assert_eq!(
        settings.basic().unwrap().input_files,
        BTreeSet::from(["cli.pdf".into(), "file.pdf".into()])
    );
    assert_eq!(settings.get("google"), Some(&json!(false)));
    assert_eq!(settings.get("siliconflowfree"), Some(&json!(false)));
    assert_eq!(settings.get("future.keep"), Some(&json!([1])));
    // If a single layer selects multiple engines, preserve metadata order.
    assert_eq!(
        Settings::from_value(json!({"google":true,"bing":true}))
            .unwrap()
            .main_engine()
            .name,
        "Google"
    );
    assert_eq!(
        Settings::from_layers(&[json!({"bing":"true"}), json!({"google":true})])
            .unwrap()
            .main_engine()
            .name,
        "Bing"
    );
    assert!(
        Settings::from_value(json!({"openai_detail":{"translate_engine_type":"mismatched"}}))
            .is_err()
    );
}

#[test]
fn all_sixteen_core_providers_map_without_network_or_machine_credentials() {
    let cases = [
        ("SiliconFlowFree", "siliconflowfree", json!({}), "", ""),
        (
            "OpenAI",
            "openai",
            json!({"openai_api_key":" synthetic "}),
            "",
            "gpt-4o-mini",
        ),
        ("Google", "google", json!({}), "", ""),
        ("Bing", "bing", json!({}), "", ""),
        (
            "DeepL",
            "deepl",
            json!({"deepl_auth_key":"synthetic:fx"}),
            "",
            "",
        ),
        (
            "Ollama",
            "ollama",
            json!({}),
            "http://localhost:11434",
            "gemma2",
        ),
        (
            "Xinference",
            "xinference",
            json!({"xinference_host":"http://127.0.0.1:1"}),
            "http://127.0.0.1:1",
            "gemma-2-it",
        ),
        (
            "AzureOpenAI",
            "azure-openai",
            json!({"azure_openai_api_key":"synthetic","azure_openai_base_url":"https://synthetic.invalid"}),
            "https://synthetic.invalid",
            "gpt-4o-mini",
        ),
        (
            "SiliconFlow",
            "siliconflow",
            json!({"siliconflow_api_key":"synthetic"}),
            "https://api.siliconflow.cn/v1",
            "Qwen/Qwen2.5-7B-Instruct",
        ),
        (
            "TencentMechineTranslation",
            "tencent",
            json!({"tencentcloud_secret_id":"synthetic-id","tencentcloud_secret_key":"synthetic-key"}),
            "",
            "",
        ),
        (
            "Azure",
            "azure",
            json!({"azure_api_key":"synthetic"}),
            "https://api.translator.azure.cn",
            "",
        ),
        (
            "AnythingLLM",
            "anythingllm",
            json!({"anythingllm_apikey":"synthetic","anythingllm_url":"http://127.0.0.1:1/workspace/test/chat"}),
            "http://127.0.0.1:1/workspace/test/chat",
            "",
        ),
        (
            "Dify",
            "dify",
            json!({"dify_apikey":"synthetic","dify_url":"http://127.0.0.1:1/workflows/run"}),
            "http://127.0.0.1:1/workflows/run",
            "",
        ),
        (
            "QwenMt",
            "qwen-mt",
            json!({"qwenmt_api_key":"synthetic"}),
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen-mt-plus",
        ),
        (
            "CLITranslator",
            "clitranslator",
            json!({"clitranslator_command":"synthetic-test-program --fixture"}),
            "",
            "",
        ),
        (
            "ClaudeCode",
            "claudecode",
            json!({"claude_code_path":"synthetic-test-program"}),
            "",
            "sonnet",
        ),
    ];
    let mut seen = BTreeSet::new();
    for (name, provider, detail, endpoint, model) in cases {
        let settings = chosen(name, detail);
        let config = settings.main_translator_config().unwrap();
        assert_eq!(config.provider, provider);
        assert_eq!(config.endpoint, endpoint);
        assert_eq!(config.model, model);
        assert_eq!(config.lang_out, "zh");
        assert_eq!(
            config.options["qps"],
            if name == "SiliconFlowFree" { 20 } else { 4 }
        );
        assert_eq!(config.options["lang_in"], "en");
        if name == "TencentMechineTranslation" {
            assert_eq!(config.api_key, "synthetic-key");
            assert_eq!(config.options["tencentcloud_secret_id"], "synthetic-id");
        }
        if name == "Google" || name == "Bing" {
            assert!(config.api_key.is_empty());
        }
        if name == "Ollama" {
            assert_eq!(config.options["num_predict"], 2000);
            assert!(config.options.get("timeout_seconds").is_none());
        }
        if name == "AzureOpenAI" {
            assert_eq!(config.options["api_version"], "2024-06-01");
        }
        seen.insert(config.provider);
    }
    assert_eq!(seen.len(), 16);
}

#[test]
fn all_eight_openai_aliases_keep_their_pinned_endpoints_models_and_options() {
    for (name, prefix, endpoint, model) in [
        (
            "DeepSeek",
            "deepseek",
            "https://api.deepseek.com/v1",
            "deepseek-chat",
        ),
        (
            "ModelScope",
            "modelscope",
            "https://api-inference.modelscope.cn/v1",
            "Qwen/Qwen2.5-32B-Instruct",
        ),
        (
            "Zhipu",
            "zhipu",
            "https://open.bigmodel.cn/api/paas/v4",
            "glm-4-flash",
        ),
        (
            "Gemini",
            "gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai/",
            "gemini-1.5-flash",
        ),
        ("Grok", "grok", "https://api.x.ai/v1", "grok-2-1212"),
        (
            "Groq",
            "groq",
            "https://api.groq.com/openai/v1",
            "llama-3-3-70b-versatile",
        ),
        (
            "OpenAICompatible",
            "openai_compatible",
            "http://127.0.0.1:1/v1",
            "gpt-4o-mini",
        ),
        (
            "AliyunDashScope",
            "aliyun_dashscope",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen-plus-latest",
        ),
    ] {
        let mut detail = json!({format!("{prefix}_api_key"):"synthetic"});
        if name == "OpenAICompatible" {
            detail[format!("{prefix}_base_url")] =
                json!(" http://127.0.0.1:1/v1/chat/completions/// ");
        }
        let config = chosen(name, detail).main_translator_config().unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.endpoint, endpoint);
        assert_eq!(config.model, model);
        assert_eq!(
            config.timeout_seconds,
            if name == "AliyunDashScope" {
                500.0
            } else {
                600.0
            }
        );
        assert_eq!(
            config.options["timeout_explicit"],
            name == "AliyunDashScope"
        );
    }
    let config=chosen("DeepSeek",json!({"deepseek_api_key":"synthetic","deepseek_model":"deepseek-v4-pro","deepseek_thinking_mode":"enabled","deepseek_reasoning_effort":"max"})).main_translator_config().unwrap();
    assert_eq!(
        config.options["extra_body"],
        json!({"thinking":{"type":"enabled"}})
    );
    assert_eq!(config.options["reasoning_effort"], "max");
}

#[test]
fn openai_default_and_explicit_timeout_temperature_typo_and_json_policy_are_distinct() {
    let settings = chosen(
        "OpenAI",
        json!({"openai_api_key":" synthetic ","openai_base_url":" https://synthetic.invalid/v1/chat/completions/ ","openai_temperature":"0.25"}),
    );
    let config = settings.main_translator_config().unwrap();
    assert_eq!(config.endpoint, "https://synthetic.invalid/v1");
    assert_eq!(config.api_key, "synthetic");
    assert_eq!(config.timeout_seconds, 600.0);
    assert_eq!(config.options["timeout_explicit"], false);
    assert!(config.options.get("temperature").is_none());
    let mut explicit = settings.clone();
    explicit
        .set("openai_detail.openai_timeout", json!("7.5"))
        .unwrap();
    explicit
        .set("openai_detail.openai_send_temprature", json!(true))
        .unwrap();
    explicit
        .set("openai_detail.openai_enable_json_mode", json!(true))
        .unwrap();
    let config = explicit.main_translator_config().unwrap();
    assert_eq!(config.timeout_seconds, 7.5);
    assert_eq!(config.options["timeout_explicit"], true);
    assert_eq!(config.options["temperature"], 0.25);
    assert_eq!(config.options["enable_json_mode"], true);
    for timeout in ["0", "-1", "NaN", "Infinity", "synthetic private invalid"] {
        explicit
            .set("openai_detail.openai_timeout", json!(timeout))
            .unwrap();
        let error = explicit.main_translator_config().unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::InvalidField);
        assert!(!format!("{error:?}").contains("synthetic private"));
    }
}

#[test]
fn term_provider_has_separate_details_and_rate_while_non_llm_main_disables_only_without_term() {
    let mut settings = chosen("Google", json!({}));
    let input = PathBuf::from("synthetic.pdf");
    assert!(settings.term_translator_config().unwrap().is_none());
    assert!(
        settings
            .pdf_job(input.clone())
            .unwrap()
            .no_auto_extract_glossary
    );
    settings.set("term_openai", json!(true)).unwrap();
    settings
        .set(
            "term_openai_detail.term_openai_api_key",
            json!("synthetic-term-key"),
        )
        .unwrap();
    settings
        .set(
            "term_openai_detail.term_openai_model",
            json!("term-fixture"),
        )
        .unwrap();
    settings.set("translation.term_qps", json!(2)).unwrap();
    let term = settings.term_translator_config().unwrap().unwrap();
    assert_eq!(term.model, "term-fixture");
    assert_eq!(term.api_key, "synthetic-term-key");
    assert_eq!(term.options["qps"], 2);
    assert!(!settings.pdf_job(input).unwrap().no_auto_extract_glossary);
    assert_eq!(
        settings.main_translator_config().unwrap().provider,
        "google"
    );
    let settings = chosen("OpenAI", json!({"openai_api_key":"synthetic-main"}));
    let term = settings.term_translator_config().unwrap().unwrap();
    assert_eq!(term.api_key, "synthetic-main");
    assert_eq!(term.options["qps"], 4);
}

#[test]
fn pdf_builder_keeps_baseline_engine_defaults_inversions_and_compatibility_rules() {
    let mut settings = chosen("Google", json!({}));
    for (path, value) in [
        ("translation.output", json!("synthetic-output")),
        ("translation.qps", json!(6)),
        ("translation.ignore_cache", json!(true)),
        ("translation.glossaries", Value::Null),
        ("pdf.enhance_compatibility", json!(true)),
        ("pdf.auto_enable_ocr_workaround", json!(true)),
        ("pdf.ocr_workaround", json!(true)),
        ("pdf.skip_scanned_detection", json!(true)),
        ("pdf.watermark_output_mode", json!("NoWatermark")),
        ("pdf.max_pages_per_part", json!(50)),
        ("pdf.no_remove_non_formula_lines", json!(true)),
    ] {
        settings.set(path, value).unwrap();
    }
    let job = settings.pdf_job(PathBuf::from("synthetic.pdf")).unwrap();
    assert_eq!(job.qps, 6);
    assert_eq!(job.pool_max_workers, 6);
    assert_eq!(job.output_dir, PathBuf::from("synthetic-output"));
    assert_eq!(job.watermark_output_mode, "no_watermark");
    assert_eq!(job.extra["remove_non_formula_lines"], false);
    assert_eq!(job.extra["merge_alternating_line_numbers"], true);
    assert_eq!(job.extra["skip_clean"], true);
    assert_eq!(job.extra["disable_rich_text_translate"], true);
    assert_eq!(job.extra["ocr_workaround"], false);
    assert_eq!(job.extra["skip_scanned_detection"], false);
    assert_eq!(job.extra["max_pages_per_part"], 50);
    assert!(!job.extra.contains_key("ignore_cache"));
    assert!(!job.extra.contains_key("translate_table_text"));
    assert_eq!(
        settings.main_translator_config().unwrap().options["ignore_cache"],
        true
    );
    let default = Settings::default()
        .pdf_job(PathBuf::from("synthetic.pdf"))
        .unwrap();
    assert_eq!(default.qps, 20);
    assert_eq!(default.extra["remove_non_formula_lines"], true);
    assert_eq!(default.extra["merge_alternating_line_numbers"], true);
}

#[test]
fn pages_and_pdf_validation_fail_without_exposing_values_or_touching_input_files() {
    let mut settings = Settings::default();
    settings.set("pdf.pages", json!("-3, 5-, 7-9, 11")).unwrap();
    assert_eq!(
        settings.parse_pages().unwrap(),
        Some(vec![(1, 3), (5, -1), (7, 9), (11, 11)])
    );
    for pages in ["0", "3-2", "1--1", "one synthetic secret", "1,,2"] {
        settings.set("pdf.pages", json!(pages)).unwrap();
        let error = settings.validate().unwrap_err();
        assert_eq!(error.field.as_deref(), Some("pdf.pages"));
        assert!(!error.to_string().contains("synthetic secret"));
    }
    settings.set("pdf.pages", Value::Null).unwrap();
    settings.set("pdf.no_dual", json!(true)).unwrap();
    settings.set("pdf.no_mono", json!(true)).unwrap();
    assert!(settings.validate().is_err());
    settings.set("pdf.no_mono", json!(false)).unwrap();
    settings.set("translation.term_qps", json!(0)).unwrap();
    assert!(settings.validate().is_err());
}

#[test]
fn tencent_requires_explicit_credentials_and_debug_never_formats_sensitive_options() {
    let settings = chosen("TencentMechineTranslation", json!({}));
    let error = settings.main_translator_config().unwrap_err();
    assert_eq!(error.kind, ConfigErrorKind::UnsupportedCredentialSource);
    let mut settings = chosen(
        "OpenAI",
        json!({"openai_api_key":"synthetic-secret-key","core_options":{"headers":{"X-Auth":"synthetic-header"}}}),
    );
    settings
        .set("future.private", json!("synthetic-future-secret"))
        .unwrap();
    settings
        .set(
            "translation.custom_system_prompt",
            json!("synthetic-private-prompt"),
        )
        .unwrap();
    let config = settings.main_translator_config().unwrap();
    assert_eq!(config.options["headers"]["X-Auth"], "synthetic-header");
    let debug = format!(
        "{settings:?} {:?} {:?} {:?} {:?} {config:?}",
        settings.basic().unwrap(),
        settings.translation().unwrap(),
        settings.pdf().unwrap(),
        settings.gui().unwrap()
    );
    for secret in [
        "synthetic-secret-key",
        "synthetic-header",
        "synthetic-future-secret",
        "synthetic-private-prompt",
    ] {
        assert!(!debug.contains(secret));
    }
}

#[test]
fn pdf_job_matches_the_actual_worker_whitelist_and_keeps_glossaries_as_typed_paths() {
    // Read only repository source. Do not import the worker / BabelDOC or open
    // a glossary; the bridge owns canonicalization and the worker owns CSV load.
    fn quoted_block(source: &str, marker: &str, end: &str) -> BTreeSet<String> {
        source
            .split_once(marker)
            .unwrap()
            .1
            .lines()
            .skip(1)
            .take_while(|line| line.trim() != end)
            .filter_map(|line| {
                line.trim()
                    .strip_prefix('"')
                    .and_then(|rest| rest.split_once('"'))
                    .map(|(name, _)| name.to_owned())
            })
            .collect()
    }
    let worker = include_str!("../../../python-worker/worker.py");
    let mut allowed = quoted_block(worker, "ENGINE_OPTIONS = frozenset(", ")");
    allowed.extend(quoted_block(worker, "required = {", "}"));
    allowed.insert("term_qps".into()); // Consumed by Rust before any worker frame.
    let mut settings = chosen(
        "OpenAI",
        json!({"openai_api_key":"synthetic-not-for-worker"}),
    );
    for (path, value) in [
        ("translation.glossaries", json!("first.csv,second.csv")),
        ("translation.term_qps", json!(2)),
        ("report_interval", json!(0.4)),
        ("pdf.max_pages_per_part", json!(50)),
        (
            "translation.rpc_doclayout",
            json!("http://synthetic.invalid/rpc"),
        ),
        (
            "future.not_an_engine_option",
            json!("retained only in settings"),
        ),
    ] {
        settings.set(path, value).unwrap();
    }
    let job = settings.pdf_job(PathBuf::from("synthetic.pdf")).unwrap();
    for key in job.extra.keys() {
        assert!(allowed.contains(key), "unsupported worker option: {key}");
    }
    let paths: Vec<PathBuf> = serde_json::from_value(job.extra["glossary_files"].clone()).unwrap();
    assert_eq!(
        paths,
        vec![PathBuf::from("first.csv"), PathBuf::from("second.csv")]
    );
    assert!(!job.extra.contains_key("glossaries"));
    assert!(!job.extra.contains_key("rpc_doclayout"));
    assert!(!job.extra.contains_key("not_an_engine_option"));
    assert_eq!(job.extra["report_interval"], 0.4);
    assert!(
        !serde_json::to_string(&job)
            .unwrap()
            .contains("synthetic-not-for-worker")
    );
}
