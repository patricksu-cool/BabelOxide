use babeloxide_config::{
    ConfigErrorKind, ConfigStore, Settings, decode_toml_content, encode_toml_content,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn fixture() -> (TempDir, ConfigStore) {
    let directory = TempDir::new().unwrap();
    let store = ConfigStore::new(directory.path().join("application")).unwrap();
    (directory, store)
}
fn write(store: &ConfigStore, text: &str) {
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    fs::write(store.path(), text).unwrap();
}

#[test]
fn application_path_is_independent_of_cwd_and_no_default_is_written_on_load() {
    let (directory, store) = fixture();
    assert_eq!(
        store.path(),
        directory.path().join("application/config/config.v3.toml")
    );
    assert!(ConfigStore::new(PathBuf::from("relative-app")).is_err());
    let settings = store.load().unwrap();
    assert_eq!(settings.translation().unwrap().qps, 4);
    assert!(!store.path().exists());
    write(&store, "[translation]\nqps=7\n");
    let cwd = directory.path().join("unrelated-cwd");
    fs::create_dir_all(cwd.join("config")).unwrap();
    fs::write(
        cwd.join("config/config.v3.toml"),
        "malformed synthetic configuration [",
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "isolated_cwd_helper", "--nocapture"])
        .env("BABELOXIDE_CONFIG_FIXTURE_ROOT", store.app_root())
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(output.status.success(), "isolated test helper failed");
    let env = BTreeMap::from([(
        "BABELOXIDE_APP_DIR".into(),
        store.app_root().to_str().unwrap().into(),
    )]);
    assert_eq!(
        ConfigStore::from_environment(&env).unwrap().path(),
        store.path()
    );
}

#[test]
fn isolated_cwd_helper() {
    let Some(root) = std::env::var_os("BABELOXIDE_CONFIG_FIXTURE_ROOT") else {
        return;
    };
    let store = ConfigStore::new(PathBuf::from(root)).unwrap();
    assert_eq!(store.load().unwrap().translation().unwrap().qps, 7);
}

#[test]
fn legacy_config_is_copied_once_byte_for_byte_and_never_replaces_local_settings() {
    let (directory, store) = fixture();
    let home = directory.path().join("legacy-home");
    let old = home.join(".config/pdf2zh/config.v3.toml");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    let bytes=b"# synthetic legacy\nopenai = true\n[openai_detail]\nopenai_api_key = 'synthetic-key'\n[future]\nkeep = ['null', 'next-version']\n";
    fs::write(&old, bytes).unwrap();
    assert!(store.migrate_legacy_once(&home).unwrap());
    assert_eq!(fs::read(store.path()).unwrap(), bytes);
    assert_eq!(fs::read(&old).unwrap(), bytes);
    fs::write(&old, b"[unreadable-format synthetic invalid").unwrap();
    assert!(!store.migrate_legacy_once(&home).unwrap());
    assert_eq!(fs::read(store.path()).unwrap(), bytes);
    let configured = store.clone().with_legacy_home(home).unwrap();
    let settings = configured.load().unwrap();
    assert_eq!(
        settings.main_translator_config().unwrap().api_key,
        "synthetic-key"
    );
    assert_eq!(fs::read(configured.path()).unwrap(), bytes);
}

#[test]
fn invalid_legacy_document_is_never_published_as_a_new_configuration() {
    let (directory, store) = fixture();
    let home = directory.path().join("legacy-home");
    let old = home.join(".config/pdf2zh/config.v3.toml");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    let bytes = b"[broken legacy document";
    fs::write(&old, bytes).unwrap();
    assert!(store.migrate_legacy_once(&home).is_err());
    assert!(!store.path().exists());
    assert_eq!(fs::read(old).unwrap(), bytes);
}

#[test]
fn racing_legacy_migrators_have_one_winner_and_leave_no_shared_tempfile() {
    let (directory, store) = fixture();
    let home = directory.path().join("legacy-home");
    let old = home.join(".config/pdf2zh/config.v3.toml");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    let original = format!("[future]\npayload = '{}'\n", "synthetic".repeat(10_000));
    fs::write(old, &original).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let home = home.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                store.migrate_legacy_once(&home).unwrap()
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|won| *won)
            .count(),
        1
    );
    assert_eq!(fs::read_to_string(store.path()).unwrap(), original);
    let names: Vec<_> = fs::read_dir(store.path().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn explicit_cli_environment_and_file_precedence_never_saves_runtime_secrets() {
    let (directory, store) = fixture();
    let base = "openai=true\n[translation]\nqps=4\n[openai_detail]\nopenai_api_key='synthetic-base'\n[future]\nkeep='base'\n";
    write(&store, base);
    let explicit = directory.path().join("explicit.toml");
    let higher = "google=true\n[translation]\nqps=6\n[openai_detail]\nopenai_api_key='synthetic-explicit'\n[future]\nextra='explicit'\n";
    fs::write(&explicit, higher).unwrap();
    let env = BTreeMap::from([
        (
            "BABELOXIDE_CONFIG_FILE".into(),
            explicit.to_str().unwrap().into(),
        ),
        ("BABELOXIDE_QPS".into(), "8".into()),
        ("BABELOXIDE_OPENAI_API_KEY".into(), "synthetic-env".into()),
    ]);
    let settings = store
        .load_with_overrides(
            &env,
            &json!({"openai":true,"qps":3,"openai_api_key":"synthetic-cli"}),
        )
        .unwrap();
    assert_eq!(settings.main_engine().name, "OpenAI");
    assert_eq!(settings.translation().unwrap().qps, 3);
    assert_eq!(
        settings.main_translator_config().unwrap().api_key,
        "synthetic-cli"
    );
    assert_eq!(settings.get("future.keep"), Some(&json!("base")));
    assert_eq!(settings.get("future.extra"), Some(&json!("explicit")));
    assert_eq!(fs::read_to_string(store.path()).unwrap(), base);
    assert_eq!(fs::read_to_string(explicit).unwrap(), higher);
}

#[test]
fn supplied_sdk_url_fallbacks_apply_only_to_none_and_are_not_persisted() {
    let (_directory, store) = fixture();
    let env = BTreeMap::from([
        (
            "OPENAI_BASE_URL".into(),
            "http://synthetic.invalid/sdk/chat/completions".into(),
        ),
        (
            "AZURE_OPENAI_ENDPOINT".into(),
            "https://synthetic-azure.invalid".into(),
        ),
        (
            "TENCENTCLOUD_SECRET_KEY".into(),
            "synthetic-unimported-sdk-key".into(),
        ),
    ]);
    let mut settings=store.load_with_overrides(&env,&json!({"openai":true,"openai_api_key":"synthetic-main","term_azureopenai":true,"term_azure_openai_api_key":"synthetic-term"})).unwrap();
    // SDK-level fallback does not pass through application's clean_url().
    assert_eq!(
        settings.main_translator_config().unwrap().endpoint,
        "http://synthetic.invalid/sdk/chat/completions"
    );
    assert_eq!(
        settings.term_translator_config().unwrap().unwrap().endpoint,
        "https://synthetic-azure.invalid"
    );
    assert_eq!(
        settings.get("openai_detail.openai_base_url"),
        Some(&Value::Null)
    );
    settings
        .set("openai_detail.openai_model", json!("gui-edited-model"))
        .unwrap();
    settings.set("translation.lang_out", json!("de")).unwrap();
    let edited = settings.main_translator_config().unwrap();
    assert_eq!(edited.model, "gui-edited-model");
    assert_eq!(edited.lang_out, "de");
    assert_eq!(
        edited.endpoint,
        "http://synthetic.invalid/sdk/chat/completions"
    );
    assert_eq!(
        settings.term_translator_config().unwrap().unwrap().endpoint,
        "https://synthetic-azure.invalid"
    );
    store.save(&settings).unwrap();
    let saved = fs::read_to_string(store.path()).unwrap();
    assert!(!saved.contains("synthetic.invalid"));
    assert!(!saved.contains("synthetic-azure.invalid"));
    assert!(!saved.contains("synthetic-unimported-sdk-key"));
    let explicit = store
        .load_with_overrides(
            &env,
            &json!({"openai":true,"openai_base_url":"https://explicit.invalid/v1"}),
        )
        .unwrap();
    assert_eq!(
        explicit.main_translator_config().unwrap().endpoint,
        "https://explicit.invalid/v1"
    );
    for (flag, prefix, provider) in [
        ("siliconflow", "siliconflow", "siliconflow"),
        ("qwenmt", "qwenmt", "qwen-mt"),
    ] {
        let mut cli = json!({});
        cli[flag] = json!(true);
        cli[format!("{prefix}_api_key")] = json!("synthetic");
        cli[format!("{prefix}_base_url")] = Value::Null;
        let settings = store.load_with_overrides(&env, &cli).unwrap();
        let config = settings.main_translator_config().unwrap();
        assert_eq!(config.provider, provider);
        assert_eq!(
            config.endpoint,
            "http://synthetic.invalid/sdk/chat/completions"
        );
    }
}

#[test]
fn tencent_environment_credentials_override_config_without_persistence() {
    let (_directory, store) = fixture();
    let cli = json!({"tencentmechinetranslation":true,"tencentcloud_secret_id":"configured-id","tencentcloud_secret_key":"configured-key"});
    let mut env = BTreeMap::from([
        ("TENCENTCLOUD_SECRET_ID".into(), "environment-id".into()),
        ("TENCENTCLOUD_SECRET_KEY".into(), "environment-key".into()),
        (
            "TENCENTCLOUD_TOKEN".into(),
            "environment-token-ignored".into(),
        ),
    ]);
    let mut settings = store.load_with_overrides(&env, &cli).unwrap();
    settings.set("translation.lang_out", json!("de")).unwrap();
    let config = settings.main_translator_config().unwrap();
    assert_eq!(config.options["tencentcloud_secret_id"], "environment-id");
    assert_eq!(config.api_key, "environment-key");
    assert!(config.options.get("tencentcloud_token").is_none());
    assert!(!format!("{settings:?} {config:?}").contains("environment-key"));
    store.save(&settings).unwrap();
    let saved = fs::read_to_string(store.path()).unwrap();
    assert!(!saved.contains("environment-id"));
    assert!(!saved.contains("environment-key"));
    assert!(!saved.contains("environment-token-ignored"));

    env.remove("TENCENTCLOUD_SECRET_KEY");
    let partial = store
        .load_with_overrides(&env, &cli)
        .unwrap()
        .main_translator_config()
        .unwrap();
    assert_eq!(partial.api_key, "configured-key");
    assert_eq!(partial.options["tencentcloud_secret_id"], "configured-id");

    env.insert("TENCENTCLOUD_SECRET_KEY".into(), " invalid-key".into());
    let error = store
        .load_with_overrides(&env, &cli)
        .unwrap()
        .main_translator_config()
        .unwrap_err();
    assert_eq!(error.kind, ConfigErrorKind::InvalidField);
    assert!(!error.to_string().contains("invalid-key"));

    env.remove("TENCENTCLOUD_SECRET_KEY");
    for key in [
        "TKE_REGION",
        "TKE_PROVIDER_ID",
        "TKE_WEB_IDENTITY_TOKEN_FILE",
        "TKE_ROLE_ARN",
    ] {
        env.insert(key.into(), "synthetic".into());
    }
    let error = store
        .load_with_overrides(&env, &cli)
        .unwrap()
        .main_translator_config()
        .unwrap_err();
    assert_eq!(error.kind, ConfigErrorKind::UnsupportedCredentialSource);
    assert!(error.to_string().contains("TKE"));
}

#[test]
fn null_codec_matches_dictionary_only_legacy_format_and_unknown_fields_roundtrip() {
    let decoded = decode_toml_content(
        json!({"none":"null","nested":{"none":"null"},"array":["null",{"none":"null"}]}),
    );
    assert_eq!(
        decoded,
        json!({"none":null,"nested":{"none":null},"array":["null",{"none":"null"}]})
    );
    assert_eq!(
        encode_toml_content(decoded),
        json!({"none":"null","nested":{"none":"null"},"array":["null",{"none":"null"}]})
    );
    let (_directory, store) = fixture();
    write(
        &store,
        "[translation]\noutput='null'\n[future]\nreleased=1979-05-27T07:32:00Z\nnonfinite=nan\n'contains.dot'='retained'\narray=['null','future']\n[future.nested]\nvalue='synthetic'\n",
    );
    let mut settings = store.load().unwrap();
    assert_eq!(settings.translation().unwrap().output, None);
    assert_eq!(
        settings.get("future.released"),
        Some(&json!("1979-05-27T07:32:00Z"))
    );
    settings.set("translation.qps", json!(7)).unwrap();
    store.save(&settings).unwrap();
    let persisted: toml::Value =
        toml::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
    assert!(persisted["future"]["released"].is_datetime());
    assert!(
        persisted["future"]["nonfinite"]
            .as_float()
            .unwrap()
            .is_nan()
    );
    assert_eq!(
        persisted["future"]["contains.dot"].as_str(),
        Some("retained")
    );
    assert_eq!(persisted["future"]["array"][0].as_str(), Some("null"));
    let reloaded = store.load().unwrap();
    assert_eq!(
        reloaded.get("future.nested.value"),
        Some(&json!("synthetic"))
    );
    assert_eq!(reloaded.translation().unwrap().qps, 7);
    assert_eq!(reloaded.translation().unwrap().output, None);
}

#[test]
fn malformed_or_invalid_configuration_is_not_replaced_or_echoed_in_errors() {
    let (_directory, store) = fixture();
    for source in [
        "[openai_detail]\nopenai_api_key='synthetic-private-key\n",
        "[translation]\nqps='synthetic-private-key'\n",
    ] {
        write(&store, source);
        let error = store.load().unwrap_err();
        assert!(matches!(
            error.kind,
            ConfigErrorKind::InvalidToml | ConfigErrorKind::InvalidField
        ));
        assert!(!format!("{error:?} {error}").contains("synthetic-private-key"));
        assert_eq!(fs::read_to_string(store.path()).unwrap(), source);
    }
}

#[test]
fn concurrent_atomic_saves_never_expose_partial_or_mixed_documents() {
    let (_directory, store) = fixture();
    let mut initial = Settings::default();
    initial.set("future.marker", json!("initial")).unwrap();
    initial
        .set("future.payload", json!("initial".repeat(1000)))
        .unwrap();
    store.save(&initial).unwrap();
    let barrier = Arc::new(Barrier::new(5));
    let handles: Vec<_> = (0..4)
        .map(|index| {
            let store = store.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                let marker = format!("writer-{index}");
                let mut settings = Settings::default();
                settings.set("future.marker", json!(marker)).unwrap();
                settings
                    .set("future.payload", json!(marker.repeat(1000)))
                    .unwrap();
                for _ in 0..5 {
                    store.save(&settings).unwrap();
                }
            })
        })
        .collect();
    barrier.wait();
    let mut reads = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let bytes = fs::read_to_string(store.path()).unwrap();
        let parsed: toml::Value = toml::from_str(&bytes).unwrap();
        let marker = parsed["future"]["marker"].as_str().unwrap();
        assert_eq!(
            parsed["future"]["payload"].as_str().unwrap(),
            marker.repeat(1000)
        );
        reads += 1;
        if handles.iter().all(thread::JoinHandle::is_finished) {
            break;
        }
        assert!(Instant::now() < deadline, "atomic writer fixture timed out");
        thread::yield_now();
    }
    for handle in handles {
        handle.join().unwrap();
    }
    assert!(reads > 0);
    assert_eq!(
        fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn user_default_save_clears_only_the_persisted_input_files_and_preserves_caller() {
    let (_directory, store) = fixture();
    let mut settings = Settings::from_value(
        json!({"basic":{"input_files":["synthetic.pdf"]},"future":{"keep":"synthetic extension"}}),
    )
    .unwrap();
    settings.set("translation.output", Value::Null).unwrap();
    store.save_user_defaults(&settings).unwrap();
    assert_eq!(settings.basic().unwrap().input_files.len(), 1);
    let reloaded = store.load().unwrap();
    assert!(reloaded.basic().unwrap().input_files.is_empty());
    assert_eq!(
        reloaded.get("future.keep"),
        Some(&json!("synthetic extension"))
    );
    assert_eq!(reloaded.translation().unwrap().output, None);
}

#[cfg(windows)]
#[test]
fn windows_replace_failure_preserves_old_file_and_cleans_its_private_tempfile() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_directory, store) = fixture();
    let mut settings = Settings::default();
    settings.set("future.generation", json!(1)).unwrap();
    store.save(&settings).unwrap();
    let old = fs::read(store.path()).unwrap();
    // Permit readers, deny deletion/replacement. A truncate-in-place fallback
    // would corrupt the old content; the expected operation instead fails.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(store.path())
        .unwrap();
    settings.set("future.generation", json!(2)).unwrap();
    assert_eq!(store.save(&settings).unwrap_err().kind, ConfigErrorKind::Io);
    assert_eq!(fs::read(store.path()).unwrap(), old);
    assert_eq!(
        fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    drop(held);
    store.save(&settings).unwrap();
    assert_eq!(
        store.load().unwrap().get("future.generation"),
        Some(&json!(2))
    );
}
