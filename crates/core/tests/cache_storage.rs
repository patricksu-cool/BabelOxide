//! All databases are synthetic files in test-owned temporary directories.
//! No application imports, home-directory lookup or user cache access occurs.

use pdf2zh_core::{
    CacheErrorKind, CacheKey, CacheStore, LegacyNamespace, TranslationRequest, TranslatorConfig,
    cache::python_json,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, task::Poll, time::Duration};
use tokio_util::sync::CancellationToken;

fn config() -> TranslatorConfig {
    TranslatorConfig {
        provider: "openai".into(),
        endpoint: "http://127.0.0.1:12345/v1".into(),
        api_key: "synthetic-key".into(),
        model: "synthetic-model".into(),
        timeout_seconds: 1.0,
        lang_out: "zh-TW".into(),
        options: json!({"lang_in":"en"}),
    }
}
fn request(text: &str) -> TranslationRequest {
    TranslationRequest {
        id: "synthetic".into(),
        text: text.into(),
        method: "translate".into(),
        ignore_cache: false,
        rate_limit_params: Value::Null,
        translator: "main".into(),
    }
}
fn key(text: &str) -> CacheKey {
    CacheKey::from_request(&config(), &request(text))
}
const LEGACY_SCHEMA: &str = "CREATE TABLE _translationcache (
    id INTEGER NOT NULL PRIMARY KEY,
    translate_engine VARCHAR(20) NOT NULL,
    translate_engine_params TEXT NOT NULL,
    original_text TEXT NOT NULL,
    translation TEXT NOT NULL,
    UNIQUE(translate_engine,translate_engine_params,original_text) ON CONFLICT REPLACE
)";

#[test]
fn python_json_recursively_sorts_and_preserves_ascii_escapes_and_spaces() {
    let value = json!({"z":[{"繁":"😀", "a":true}], "a":{"n":0.00001,"null":null}});
    // Independent Python json.dumps(_sort_dict_recursively(value)) oracle.
    assert_eq!(
        python_json(&value),
        r#"{"a": {"n": 1e-05, "null": null}, "z": [{"a": true, "\u7e41": "\ud83d\ude00"}]}"#
    );
    assert_eq!(
        python_json(&json!(["\u{7f}\u{1}\n\r\t\u{8}\u{c}\\\"/", false, 42])),
        r#"["\u007f\u0001\n\r\t\b\f\\\"/", false, 42]"#
    );
    assert_eq!(
        python_json(&json!({"😀":1,"繁":2,"a":3})),
        r#"{"a": 3, "\u7e41": 2, "\ud83d\ude00": 1}"#
    );
}

#[test]
fn python_float_json_matches_exponent_boundaries_and_preserves_integer_type() {
    for (value, expected) in [
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (1.0, "1.0"),
        (1e-4, "0.0001"),
        (1e-5, "1e-05"),
        (1e15, "1000000000000000.0"),
        (1e16, "1e+16"),
        (1.25e20, "1.25e+20"),
        (-1.25e-6, "-1.25e-06"),
        (f64::MAX, "1.7976931348623157e+308"),
        (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
        (f64::from_bits(1), "5e-324"),
    ] {
        assert_eq!(python_json(&json!(value)), expected, "value={value:?}");
    }
    assert_eq!(python_json(&json!(1)), "1");
    assert_eq!(python_json(&json!(u64::MAX)), "18446744073709551615");
    assert_eq!(
        python_json(&json!([{"b":2,"a":1},{"a":0}])),
        r#"[{"a": 1, "b": 2}, {"a": 0}]"#
    );
}

#[tokio::test]
async fn sqlite_cache_miss_overwrite_empty_success_and_reopen_are_persistent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let cancel = CancellationToken::new();
    let text_key = key("source");
    let store = CacheStore::open(&path).unwrap();
    assert!(!store.legacy_present());
    assert_eq!(store.get(&text_key, &cancel).await.unwrap(), None);
    store.set(&text_key, "first", &cancel).await.unwrap();
    store.set(&text_key, "second", &cancel).await.unwrap();
    store.set(&key("empty"), "", &cancel).await.unwrap();
    drop(store);
    let reopened = CacheStore::open(&path).unwrap();
    assert_eq!(
        reopened.get(&text_key, &cancel).await.unwrap().as_deref(),
        Some("second")
    );
    assert_eq!(
        reopened
            .get(&key("empty"), &cancel)
            .await
            .unwrap()
            .as_deref(),
        Some("")
    );
    let reader = Connection::open(&path).unwrap();
    assert_eq!(
        reader
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM rust_translation_cache_v2",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
    );
}

#[test]
fn namespace_separates_output_context_and_normalizes_method_aliases() {
    let cfg = config();
    let req = request("source");
    let initial = CacheKey::from_request(&cfg, &req);
    for mutate in [
        |cfg: &mut TranslatorConfig| cfg.endpoint = "http://127.0.0.1:54321/v1".into(),
        |cfg: &mut TranslatorConfig| cfg.model = "other-model".into(),
        |cfg: &mut TranslatorConfig| cfg.lang_out = "ja".into(),
        |cfg: &mut TranslatorConfig| cfg.options["lang_in"] = "ja".into(),
        |cfg: &mut TranslatorConfig| cfg.options["enable_json_mode"] = true.into(),
        |cfg: &mut TranslatorConfig| cfg.options["prompt"] = "another prompt".into(),
        |cfg: &mut TranslatorConfig| cfg.options["extra_body"] = json!({"max_tokens":100}),
        |cfg: &mut TranslatorConfig| cfg.options["headers"] = json!({"X-Feature":"other-context"}),
    ] {
        let mut changed = cfg.clone();
        mutate(&mut changed);
        assert_ne!(initial, CacheKey::from_request(&changed, &req));
    }
    let mut changed = req.clone();
    changed.method = "llm_translate".into();
    assert_ne!(initial, CacheKey::from_request(&cfg, &changed));
    changed = req.clone();
    changed.translator = "term".into();
    assert_ne!(initial, CacheKey::from_request(&cfg, &changed));
    changed = req.clone();
    changed.rate_limit_params = json!({"request_json_mode":true});
    assert_ne!(initial, CacheKey::from_request(&cfg, &changed));
    changed = req.clone();
    changed.text = "other source".into();
    assert_ne!(initial, CacheKey::from_request(&cfg, &changed));
    assert_eq!(
        initial.namespace_digest(),
        CacheKey::from_request(&cfg, &changed).namespace_digest()
    );
    changed = req.clone();
    changed.method = "do_translate".into();
    assert_eq!(initial, CacheKey::from_request(&cfg, &changed));
    assert!(!format!("{initial:?}").contains("source"));
}

#[test]
fn credentials_and_operational_options_do_not_partition_the_output_namespace() {
    let cfg = config();
    let req = request("source");
    let initial = CacheKey::from_request(&cfg, &req);
    let mut changed = cfg.clone();
    changed.api_key = "rotated-key".into();
    changed.timeout_seconds = 20.0;
    changed.options = json!({"lang_in":"en","qps":99,"max_concurrency":1,"ignore_cache":true,"timeout_explicit":false,"cache_path":"private-path","api_key":"not-an-output-option"});
    assert_eq!(initial, CacheKey::from_request(&changed, &req));
    let mut first = cfg.clone();
    first.options["extra_body"] = json!({"max_tokens":100});
    let mut second = first.clone();
    second.options["extra_body"]["max_tokens"] = 200.into();
    assert_ne!(
        CacheKey::from_request(&first, &req),
        CacheKey::from_request(&second, &req)
    );
}

#[tokio::test]
async fn synthetic_python_wal_rows_are_read_exactly_and_never_rewritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.db");
    let writer = Connection::open(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer.execute_batch(LEGACY_SCHEMA).unwrap();
    let params = json!({"z":[{"繁":"😀","a":true}],"a":{"n":0.00001,"null":null}});
    let canonical =
        r#"{"a": {"n": 1e-05, "null": null}, "z": [{"a": true, "\u7e41": "\ud83d\ude00"}]}"#;
    writer
        .execute(
            "INSERT INTO _translationcache VALUES(1,?1,?2,?3,?4)",
            params!["synthetic", canonical, "source", "舊譯文"],
        )
        .unwrap();
    assert!(
        std::fs::metadata(path.with_extension("db-wal"))
            .unwrap()
            .len()
            > 32
    );
    // Writer stays open, and committed pages remain in WAL: read through
    // SQLite instead of copying an active database file.
    let store = CacheStore::open(&path).unwrap();
    assert!(store.legacy_present());
    let namespace = LegacyNamespace::new("synthetic", &params).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        store
            .read_legacy(&namespace, "source", &cancel)
            .await
            .unwrap()
            .as_deref(),
        Some("舊譯文")
    );
    assert_eq!(store.get(&key("source"), &cancel).await.unwrap(), None);
    let unchanged: (String, String) = writer
        .query_row(
            "SELECT translate_engine_params,translation FROM _translationcache WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(unchanged, (canonical.into(), "舊譯文".into()));
    let wrong = LegacyNamespace::new("other-engine", &params).unwrap();
    assert_eq!(
        store.read_legacy(&wrong, "source", &cancel).await.unwrap(),
        None
    );
    let wrong = LegacyNamespace::new("synthetic", &json!({"a":1})).unwrap();
    assert_eq!(
        store.read_legacy(&wrong, "source", &cancel).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn legacy_array_and_default_namespaces_follow_python_constructor() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch(LEGACY_SCHEMA).unwrap();
    writer.execute("INSERT INTO _translationcache VALUES(1,'test_engine','[\"param1\", \"param2\"]','source','array')",[]).unwrap();
    writer
        .execute(
            "INSERT INTO _translationcache VALUES(2,'test_engine','{}','source','default')",
            [],
        )
        .unwrap();
    let store = CacheStore::open(&path).unwrap();
    let cancel = CancellationToken::new();
    for (params, expected) in [
        (json!(["param1", "param2"]), "array"),
        (Value::Null, "default"),
    ] {
        let namespace = LegacyNamespace::new("test_engine", &params).unwrap();
        assert_eq!(
            store
                .read_legacy(&namespace, "source", &cancel)
                .await
                .unwrap()
                .as_deref(),
            Some(expected)
        );
    }
    assert_eq!(
        LegacyNamespace::new(&"x".repeat(20), &json!({}))
            .unwrap_err()
            .kind,
        CacheErrorKind::InvalidNamespace
    );
    assert!(LegacyNamespace::new(&"繁".repeat(19), &json!({})).is_ok());
}

#[tokio::test]
async fn sqlite_read_only_and_missing_schema_errors_are_safe_typed_failures() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let cancel = CancellationToken::new();
    let store = CacheStore::open(&path).unwrap();
    store.set(&key("source"), "saved", &cancel).await.unwrap();
    drop(store);
    let store = CacheStore::open_read_only(&path).unwrap();
    assert_eq!(
        store.get(&key("source"), &cancel).await.unwrap().as_deref(),
        Some("saved")
    );
    assert_eq!(
        store
            .set(&key("source"), "replacement", &cancel)
            .await
            .unwrap_err()
            .kind,
        CacheErrorKind::ReadOnly
    );
    drop(store);
    let store = CacheStore::open(&path).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE rust_translation_cache_v2")
        .unwrap();
    let error = store.get(&key("source"), &cancel).await.unwrap_err();
    assert_eq!(error.kind, CacheErrorKind::Database);
    assert!(!format!("{error:?} {error}").contains("source"));
}

#[test]
fn invalid_sqlite_open_never_exposes_database_contents_or_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("private-key-path.db");
    std::fs::write(&path, b"private-key-contents not sqlite").unwrap();
    let error = CacheStore::open(&path).unwrap_err();
    assert_eq!(error.kind, CacheErrorKind::Corrupt);
    assert!(!format!("{error:?} {error}").contains("private-key"));
}

#[tokio::test]
async fn persisted_rust_namespace_never_contains_raw_credentials_or_endpoint() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let mut cfg = config();
    cfg.api_key = "unique-api-key-to-exclude".into();
    cfg.endpoint = "https://private-endpoint.invalid/?access_key=endpoint-secret".into();
    cfg.options = json!({"headers":{"Authorization":"Bearer private-token","X-Auth":"custom-secret"},"extra_body":{"private_field":"opaque-option"}});
    let cache_key = CacheKey::from_request(&cfg, &request("source"));
    let store = CacheStore::open(&path).unwrap();
    store
        .set(&cache_key, "translation", &CancellationToken::new())
        .await
        .unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    for secret in [
        "unique-api-key-to-exclude",
        "private-endpoint",
        "endpoint-secret",
        "private-token",
        "custom-secret",
        "opaque-option",
    ] {
        assert!(
            !bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "raw config persisted: {secret}"
        );
    }
}

#[tokio::test]
async fn concurrent_sqlite_handles_preserve_all_distinct_keys() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let first = Arc::new(CacheStore::open(&path).unwrap());
    let second = Arc::new(CacheStore::open(&path).unwrap());
    let mut tasks = Vec::new();
    for index in 0..24 {
        let store = if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        tasks.push(tokio::spawn(async move {
            let key = key(&format!("source-{index}"));
            let text = format!("translation-{index}");
            let cancel = CancellationToken::new();
            store.set(&key, &text, &cancel).await.unwrap();
            assert_eq!(store.get(&key, &cancel).await.unwrap(), Some(text));
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM rust_translation_cache_v2",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        24
    );
}

#[tokio::test]
async fn cancelling_sqlite_gate_wait_and_busy_write_releases_resources_without_late_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let store = CacheStore::open(&path).unwrap();
    let locker = Connection::open(&path).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let blocked_key = key("blocked");
    let write_cancel = CancellationToken::new();
    let mut write = Box::pin(store.set(&blocked_key, "must-not-commit", &write_cancel));
    // One poll acquires the store gate before awaiting its blocking worker.
    std::future::poll_fn(|context| {
        assert!(write.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    let read_cancel = CancellationToken::new();
    let read_key = key("other");
    let mut read = Box::pin(store.get(&read_key, &read_cancel));
    std::future::poll_fn(|context| {
        assert!(read.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    read_cancel.cancel();
    assert_eq!(read.await.unwrap_err().kind, CacheErrorKind::Cancelled);
    write_cancel.cancel();
    assert_eq!(write.await.unwrap_err().kind, CacheErrorKind::Cancelled);
    locker.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        store
            .get(&blocked_key, &CancellationToken::new())
            .await
            .unwrap(),
        None
    );
    store
        .set(&blocked_key, "after-cancel", &CancellationToken::new())
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_busy_budget_returns_busy_and_does_not_leave_a_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let store = CacheStore::open(&path).unwrap();
    let locker = Connection::open(&path).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = tokio::time::Instant::now();
    let error = store
        .set(&key("busy"), "must-not-commit", &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.kind, CacheErrorKind::Busy);
    assert!(started.elapsed() >= Duration::from_millis(900));
    locker.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        store
            .get(&key("busy"), &CancellationToken::new())
            .await
            .unwrap(),
        None
    );
}
