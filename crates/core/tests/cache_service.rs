use pdf2zh_core::{
    CacheBackend, CacheError, CacheErrorKind, CacheFuture, CacheKey, CacheMethod, CacheStore,
    Clock, ErrorKind, HttpExecutor, HttpRequest, HttpResponse, HttpTransport, LegacyBinding,
    LegacyNamespace, ProviderError, ProviderResult, RandomSource, RetryRuntime, TranslationRequest,
    TranslationService, TranslatorConfig, Usage,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Barrier,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

struct TestClock {
    base: Instant,
    elapsed: Mutex<Duration>,
    sleeps: Mutex<Vec<Duration>>,
}
impl TestClock {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
            sleeps: Mutex::new(Vec::new()),
        }
    }
}
impl Clock for TestClock {
    fn now(&self) -> Instant {
        self.base + *self.elapsed.lock().unwrap()
    }
    fn wall_time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000) + *self.elapsed.lock().unwrap()
    }
    fn sleep<'a>(
        &'a self,
        duration: Duration,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(ProviderError::cancelled());
            }
            self.sleeps.lock().unwrap().push(duration);
            *self.elapsed.lock().unwrap() += duration;
            tokio::task::yield_now().await;
            Ok(())
        })
    }
}
struct CenteredRandom;
impl RandomSource for CenteredRandom {
    fn unit(&self) -> f64 {
        0.5
    }
}
enum Plan {
    Text(&'static str),
    Status(u16),
    Error(ErrorKind),
    Hang,
    Concurrent(&'static str, Arc<Barrier>),
}
struct Script {
    steps: Mutex<VecDeque<Plan>>,
    calls: AtomicUsize,
}
impl Script {
    fn new(steps: Vec<Plan>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            calls: AtomicUsize::new(0),
        }
    }
    fn drained(&self) {
        assert!(self.steps.lock().unwrap().is_empty());
    }
}
fn response(request: &HttpRequest, status: u16, value: Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
        elapsed: Duration::ZERO,
        final_url: request.url.clone(),
        set_cookies: Vec::new(),
    }
}
fn success(request: &HttpRequest, text: &str) -> HttpResponse {
    response(
        request,
        200,
        json!({"choices":[{"message":{"content":text}}],"usage":{"total_tokens":8,"prompt_tokens":5,"completion_tokens":3}}),
    )
}
impl HttpExecutor for Script {
    fn send<'a>(
        &'a self,
        request: HttpRequest,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<HttpResponse>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected HTTP request after a cache hit");
            match step {
                Plan::Text(text) => Ok(success(&request, text)),
                Plan::Status(status) => {
                    let mut reply = response(
                        &request,
                        status,
                        json!({"error":{"message":"synthetic failure"}}),
                    );
                    reply
                        .headers
                        .insert("x-should-retry".into(), "false".into());
                    Ok(reply)
                }
                Plan::Error(kind) => Err(ProviderError::new(kind, "synthetic transport failure")),
                Plan::Hang => {
                    cancel.cancelled().await;
                    Err(ProviderError::cancelled())
                }
                Plan::Concurrent(text, barrier) => {
                    tokio::select! {_ =cancel.cancelled()=>return Err(ProviderError::cancelled()),_ =barrier.wait()=>{}}
                    Ok(success(&request, text))
                }
            }
        })
    }
}
fn config() -> TranslatorConfig {
    TranslatorConfig {
        provider: "openai".into(),
        endpoint: "http://127.0.0.1:12345/v1".into(),
        api_key: "synthetic-key".into(),
        model: "synthetic-model".into(),
        timeout_seconds: 1.0,
        lang_out: "zh-TW".into(),
        options: json!({"lang_in":"en","qps":2}),
    }
}
fn request() -> TranslationRequest {
    TranslationRequest {
        id: "synthetic".into(),
        text: "source text".into(),
        method: "translate".into(),
        ignore_cache: false,
        rate_limit_params: Value::Null,
        translator: "main".into(),
    }
}
fn make_service(
    config: TranslatorConfig,
    steps: Vec<Plan>,
    cache: Option<Arc<dyn CacheBackend>>,
) -> (TranslationService, Arc<Script>, Arc<TestClock>) {
    let clock = Arc::new(TestClock::new());
    let runtime = RetryRuntime::with_sources(clock.clone(), Arc::new(CenteredRandom));
    let script = Arc::new(Script::new(steps));
    let http = HttpTransport::with_executor(runtime.clone(), script.clone());
    let mut service = TranslationService::new_with_transport(config, runtime, http).unwrap();
    if let Some(cache) = cache {
        service = service.with_cache(cache);
    }
    (service, script, clock)
}
async fn eventually(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn persistent_hit_skips_qps_http_and_usage_after_reopening_service() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let cancel = CancellationToken::new();
    let req = request();
    let cache = Arc::new(CacheStore::open(&path).unwrap());
    let (first, script, clock) = make_service(config(), vec![Plan::Text("譯文")], Some(cache));
    assert_eq!(
        first
            .translate_typed(&req, &cancel)
            .await
            .unwrap()
            .usage
            .total_tokens,
        8
    );
    for _ in 0..2 {
        let output = first.translate_typed(&req, &cancel).await.unwrap();
        assert_eq!(output.text, "譯文");
        assert_eq!(output.usage, Usage::default());
    }
    let metrics = first.metrics();
    assert_eq!(
        (
            metrics.calls,
            metrics.cache_hits,
            metrics.admissions,
            metrics.http_attempts
        ),
        (3, 2, 1, 1)
    );
    assert_eq!(metrics.usage.total_tokens, 8);
    assert!(clock.sleeps.lock().unwrap().is_empty());
    script.drained();
    drop(first);
    let cache = Arc::new(CacheStore::open(&path).unwrap());
    let (reopened, script, clock) = make_service(config(), Vec::new(), Some(cache));
    assert_eq!(
        reopened.translate_typed(&req, &cancel).await.unwrap().text,
        "譯文"
    );
    assert_eq!(reopened.metrics().admissions, 0);
    assert_eq!(reopened.metrics().usage, Usage::default());
    assert_eq!(script.calls.load(Ordering::SeqCst), 0);
    assert!(clock.sleeps.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cache_hit_precedes_network_preflight_and_requires_cooperative_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Arc::new(CacheStore::open(directory.path().join("cache.db")).unwrap());
    let mut cfg = config();
    cfg.provider = "siliconflowfree".into();
    let mut req = request();
    req.method = "llm_translate".into();
    cache
        .set(
            &CacheKey::from_request(&cfg, &req),
            "cached before /check",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let (cached, script, _) = make_service(cfg, Vec::new(), Some(cache));
    assert!(cached.requires_cooperative_cleanup());
    assert!(!cached.requires_reap());
    assert!(!cached.owns_process());
    assert_eq!(
        cached
            .translate_typed(&req, &CancellationToken::new())
            .await
            .unwrap()
            .text,
        "cached before /check"
    );
    assert_eq!(script.calls.load(Ordering::SeqCst), 0);
    let (uncached, _, _) = make_service(config(), Vec::new(), None);
    assert!(!uncached.requires_cooperative_cleanup());
}

#[tokio::test]
async fn ignore_cache_at_request_or_instance_skips_both_reads_and_writes() {
    for instance in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let cache = Arc::new(CacheStore::open(directory.path().join("cache.db")).unwrap());
        let mut cfg = config();
        let original = request();
        let key = CacheKey::from_request(&cfg, &original);
        let cancel = CancellationToken::new();
        cache.set(&key, "old cached value", &cancel).await.unwrap();
        cfg.options["ignore_cache"] = instance.into();
        let (service, script, _) =
            make_service(cfg, vec![Plan::Text("uncached fresh")], Some(cache.clone()));
        let mut req = original.clone();
        req.ignore_cache = !instance;
        assert_eq!(
            service.translate_typed(&req, &cancel).await.unwrap().text,
            "uncached fresh"
        );
        assert_eq!(service.metrics().cache_hits, 0);
        assert_eq!(script.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            cache.get(&key, &cancel).await.unwrap().as_deref(),
            Some("old cached value")
        );
    }
}

#[tokio::test]
async fn output_context_endpoint_method_role_and_options_do_not_reuse_other_rows() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Arc::new(CacheStore::open(directory.path().join("cache.db")).unwrap());
    let cancel = CancellationToken::new();
    let base = config();
    let req = request();
    let (initial, _, _) = make_service(base.clone(), vec![Plan::Text("base")], Some(cache.clone()));
    assert_eq!(
        initial.translate_typed(&req, &cancel).await.unwrap().text,
        "base"
    );
    for variant in 0..6 {
        let mut cfg = base.clone();
        let mut req = req.clone();
        match variant {
            0 => cfg.endpoint = "http://127.0.0.1:54321/v1".into(),
            1 => cfg.model = "different-model".into(),
            2 => cfg.lang_out = "ja".into(),
            3 => req.method = "llm_translate".into(),
            4 => req.translator = "term".into(),
            _ => cfg.options["extra_body"] = json!({"max_tokens":100}),
        }
        let (other, script, _) = make_service(
            cfg,
            vec![Plan::Text("different output")],
            Some(cache.clone()),
        );
        assert_eq!(
            other.translate_typed(&req, &cancel).await.unwrap().text,
            "different output"
        );
        assert_eq!(script.calls.load(Ordering::SeqCst), 1);
        script.drained();
    }
    let mut alias = req.clone();
    alias.method = "do_translate".into();
    assert_eq!(
        initial.translate_typed(&alias, &cancel).await.unwrap().text,
        "base"
    );
}

struct FailingCache {
    reads: AtomicUsize,
    writes: AtomicUsize,
    kind: CacheErrorKind,
}
impl CacheBackend for FailingCache {
    fn get<'a>(
        &'a self,
        _: &'a CacheKey,
        _: &'a CancellationToken,
    ) -> CacheFuture<'a, Option<String>> {
        Box::pin(async move {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Err(CacheError::new(self.kind))
        })
    }
    fn set<'a>(
        &'a self,
        _: &'a CacheKey,
        _: &'a str,
        _: &'a CancellationToken,
    ) -> CacheFuture<'a, ()> {
        Box::pin(async move {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Err(CacheError::new(self.kind))
        })
    }
}

#[tokio::test]
async fn factory_health_bypasses_cache_but_counts_usage_and_rate_admissions() {
    let cache = Arc::new(FailingCache {
        reads: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        kind: CacheErrorKind::Cancelled,
    });
    let mut config = config();
    config.options["qps"] = json!(1);
    let (service, script, clock) = make_service(
        config,
        vec![Plan::Text("health-one"), Plan::Text("health-two")],
        Some(cache.clone()),
    );
    let cancel = CancellationToken::new();
    service.prepare(&cancel).await.unwrap();
    service.health_check(&cancel).await.unwrap();
    service.health_check(&cancel).await.unwrap();
    assert_eq!(cache.reads.load(Ordering::SeqCst), 0);
    assert_eq!(cache.writes.load(Ordering::SeqCst), 0);
    let metrics = service.metrics();
    assert_eq!(
        (metrics.calls, metrics.admissions, metrics.http_attempts),
        (2, 2, 2)
    );
    assert_eq!(metrics.usage.total_tokens, 16);
    assert_eq!(metrics.cache_hits, 0);
    assert_eq!(*clock.sleeps.lock().unwrap(), vec![Duration::from_secs(1)]);
    script.drained();
}

#[tokio::test]
async fn cache_failures_are_best_effort_but_cancelled_is_a_control_event() {
    let cache = Arc::new(FailingCache {
        reads: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        kind: CacheErrorKind::Database,
    });
    let (service, script, _) = make_service(
        config(),
        vec![Plan::Text("valid provider output")],
        Some(cache.clone()),
    );
    assert!(service.requires_cooperative_cleanup());
    assert_eq!(
        service
            .translate_typed(&request(), &CancellationToken::new())
            .await
            .unwrap()
            .text,
        "valid provider output"
    );
    assert_eq!(
        (
            service.metrics().cache_read_errors,
            service.metrics().cache_write_errors
        ),
        (1, 1)
    );
    assert_eq!(cache.reads.load(Ordering::SeqCst), 1);
    assert_eq!(cache.writes.load(Ordering::SeqCst), 1);
    script.drained();
    let cache = Arc::new(FailingCache {
        reads: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        kind: CacheErrorKind::Cancelled,
    });
    let (service, script, _) = make_service(config(), Vec::new(), Some(cache));
    assert_eq!(
        service
            .translate_typed(&request(), &CancellationToken::new())
            .await
            .err()
            .unwrap()
            .kind,
        ErrorKind::Cancelled
    );
    assert_eq!(service.metrics().admissions, 0);
    assert_eq!(service.metrics().cache_read_errors, 0);
    assert_eq!(script.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn actual_sqlite_get_and_set_failures_do_not_fail_a_translation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let cache = Arc::new(CacheStore::open(&path).unwrap());
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE rust_translation_cache_v2")
        .unwrap();
    let (service, script, _) = make_service(
        config(),
        vec![Plan::Text("successful despite schema error")],
        Some(cache),
    );
    assert_eq!(
        service
            .translate_typed(&request(), &CancellationToken::new())
            .await
            .unwrap()
            .text,
        "successful despite schema error"
    );
    assert_eq!(
        (
            service.metrics().cache_read_errors,
            service.metrics().cache_write_errors
        ),
        (1, 1)
    );
    assert_eq!(service.metrics().usage.total_tokens, 8);
    script.drained();
}

#[tokio::test]
async fn actual_read_only_sqlite_set_failure_is_swallowed_after_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    drop(CacheStore::open(&path).unwrap());
    let cache = Arc::new(CacheStore::open_read_only(&path).unwrap());
    let (service, _, _) = make_service(
        config(),
        vec![Plan::Text("read only still translates")],
        Some(cache),
    );
    assert_eq!(
        service
            .translate_typed(&request(), &CancellationToken::new())
            .await
            .unwrap()
            .text,
        "read only still translates"
    );
    assert_eq!(
        (
            service.metrics().cache_read_errors,
            service.metrics().cache_write_errors
        ),
        (0, 1)
    );
}

#[tokio::test]
async fn failures_and_http_cancellation_never_create_a_cache_entry() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Arc::new(CacheStore::open(directory.path().join("cache.db")).unwrap());
    let req = request();
    let key = CacheKey::from_request(&config(), &req);
    let cancel = CancellationToken::new();
    let (failed, _, _) = make_service(
        config(),
        vec![Plan::Error(ErrorKind::Value)],
        Some(cache.clone()),
    );
    assert_eq!(
        failed
            .translate_typed(&req, &cancel)
            .await
            .err()
            .unwrap()
            .kind,
        ErrorKind::Value
    );
    assert_eq!(cache.get(&key, &cancel).await.unwrap(), None);
    let (running, script, _) = make_service(config(), vec![Plan::Hang], Some(cache.clone()));
    let running = Arc::new(running);
    let task_service = running.clone();
    let task_cancel = cancel.clone();
    let task_req = req.clone();
    let task =
        tokio::spawn(async move { task_service.translate_typed(&task_req, &task_cancel).await });
    eventually(|| script.calls.load(Ordering::SeqCst) == 1).await;
    cancel.cancel();
    assert_eq!(
        task.await.unwrap().err().unwrap().kind,
        ErrorKind::Cancelled
    );
    assert_eq!(
        cache.get(&key, &CancellationToken::new()).await.unwrap(),
        None
    );
    assert_eq!(running.metrics().cache_write_errors, 0);
}

#[tokio::test]
async fn retry_admission_happens_once_and_the_result_is_cached_only_after_success() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Arc::new(CacheStore::open(directory.path().join("cache.db")).unwrap());
    let (service, script, clock) = make_service(
        config(),
        vec![Plan::Status(429), Plan::Text("after retry")],
        Some(cache),
    );
    let req = request();
    let cancel = CancellationToken::new();
    assert_eq!(
        service.translate_typed(&req, &cancel).await.unwrap().text,
        "after retry"
    );
    assert_eq!(
        service.translate_typed(&req, &cancel).await.unwrap().text,
        "after retry"
    );
    let metrics = service.metrics();
    assert_eq!(
        (
            metrics.calls,
            metrics.admissions,
            metrics.http_attempts,
            metrics.retry_sleeps,
            metrics.cache_hits
        ),
        (2, 1, 2, 1, 1)
    );
    assert_eq!(
        clock.sleeps.lock().unwrap().as_slice(),
        [Duration::from_secs(1)]
    );
    script.drained();
}

#[tokio::test]
async fn simultaneous_same_key_misses_are_not_relabelled_as_cache_hits() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Arc::new(CacheStore::open(directory.path().join("cache.db")).unwrap());
    let barrier = Arc::new(Barrier::new(2));
    let (service, script, _) = make_service(
        config(),
        vec![
            Plan::Concurrent("first", barrier.clone()),
            Plan::Concurrent("second", barrier),
        ],
        Some(cache),
    );
    let service = Arc::new(service);
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let service = service.clone();
        tasks.push(tokio::spawn(async move {
            service
                .translate_typed(&request(), &CancellationToken::new())
                .await
                .unwrap()
                .text
        }));
    }
    let a = tasks.remove(0).await.unwrap();
    let b = tasks.remove(0).await.unwrap();
    assert_ne!(a, b);
    assert_eq!(service.metrics().cache_hits, 0);
    assert_eq!(service.metrics().peak_inflight, 2);
    let cached = service
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .unwrap();
    assert!(cached.text == a || cached.text == b);
    assert_eq!(cached.usage, Usage::default());
    assert_eq!(service.metrics().cache_hits, 1);
    assert_eq!(script.calls.load(Ordering::SeqCst), 2);
    script.drained();
}

#[tokio::test]
async fn cancelled_sqlite_write_is_reaped_and_never_commits_after_return() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let cache = Arc::new(CacheStore::open(&path).unwrap());
    let locker = Connection::open(&path).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let req = request();
    let key = CacheKey::from_request(&config(), &req);
    let cancel = CancellationToken::new();
    let (service, script, _) = make_service(
        config(),
        vec![Plan::Text("must not persist after cancel")],
        Some(cache.clone()),
    );
    let service = Arc::new(service);
    let task_service = service.clone();
    let task_cancel = cancel.clone();
    let task = tokio::spawn(async move { task_service.translate_typed(&req, &task_cancel).await });
    eventually(|| service.metrics().usage.total_tokens == 8).await;
    assert_eq!(script.calls.load(Ordering::SeqCst), 1);
    cancel.cancel();
    assert_eq!(
        task.await.unwrap().err().unwrap().kind,
        ErrorKind::Cancelled
    );
    locker.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        cache.get(&key, &CancellationToken::new()).await.unwrap(),
        None
    );
    assert_eq!(service.metrics().cache_write_errors, 0);
}

#[tokio::test]
async fn cancelled_cache_gate_wait_never_reaches_preflight_or_qps() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let cache = Arc::new(CacheStore::open(&path).unwrap());
    let locker = Connection::open(&path).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let key = CacheKey::from_request(&config(), &request());
    let writer_cancel = CancellationToken::new();
    let mut write = Box::pin(cache.set(&key, "blocked", &writer_cancel));
    std::future::poll_fn(|cx| {
        assert!(write.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let (service, script, _) = make_service(config(), Vec::new(), Some(cache.clone()));
    let req = request();
    let cancel = CancellationToken::new();
    let mut translation = Box::pin(service.translate_typed(&req, &cancel));
    std::future::poll_fn(|cx| {
        assert!(translation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    cancel.cancel();
    assert_eq!(translation.await.err().unwrap().kind, ErrorKind::Cancelled);
    assert_eq!(script.calls.load(Ordering::SeqCst), 0);
    assert_eq!(service.metrics().admissions, 0);
    writer_cancel.cancel();
    assert_eq!(write.await.err().unwrap().kind, CacheErrorKind::Cancelled);
    locker.execute_batch("ROLLBACK").unwrap();
}

#[tokio::test]
async fn legacy_binding_is_explicit_scoped_and_promoted_without_modifying_python_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE _translationcache(id INTEGER PRIMARY KEY,translate_engine TEXT NOT NULL,translate_engine_params TEXT NOT NULL,original_text TEXT NOT NULL,translation TEXT NOT NULL,UNIQUE(translate_engine,translate_engine_params,original_text) ON CONFLICT REPLACE)").unwrap();
    let legacy_params = json!({"lang_in":"en","lang_out":"zh-TW"});
    let canonical = r#"{"lang_in": "en", "lang_out": "zh-TW"}"#;
    writer.execute("INSERT INTO _translationcache VALUES(1,'openai',?1,'source text','legacy translation')",params![canonical]).unwrap();
    let cache = Arc::new(CacheStore::open(&path).unwrap());
    assert!(cache.legacy_present());
    let cancel = CancellationToken::new();
    let req = request();
    let (unbound, script, _) = make_service(
        config(),
        vec![Plan::Text("unbound must translate")],
        Some(cache.clone()),
    );
    assert_eq!(
        unbound.translate_typed(&req, &cancel).await.unwrap().text,
        "unbound must translate"
    );
    assert_eq!(unbound.metrics().legacy_cache_hits, 0);
    script.drained();
    let mut cfg = config();
    cfg.endpoint = "http://127.0.0.1:54321/verified-legacy-origin".into();
    let namespace = LegacyNamespace::new("openai", &legacy_params).unwrap();
    let binding = LegacyBinding::new(namespace, CacheMethod::Translate, "main");
    let (bound, script, _) = make_service(cfg.clone(), Vec::new(), Some(cache.clone()));
    let bound = bound.with_legacy_cache(binding);
    assert_eq!(
        bound.translate_typed(&req, &cancel).await.unwrap().text,
        "legacy translation"
    );
    assert_eq!(
        (
            bound.metrics().cache_hits,
            bound.metrics().legacy_cache_hits,
            bound.metrics().admissions
        ),
        (1, 1, 0)
    );
    assert_eq!(script.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        cache
            .get(&CacheKey::from_request(&cfg, &req), &cancel)
            .await
            .unwrap()
            .as_deref(),
        Some("legacy translation")
    );
    let (reopened, _, _) = make_service(cfg.clone(), Vec::new(), Some(cache.clone()));
    assert_eq!(
        reopened.translate_typed(&req, &cancel).await.unwrap().text,
        "legacy translation"
    );
    for (method, role) in [("llm_translate", "main"), ("translate", "term")] {
        let mut changed = req.clone();
        changed.method = method.into();
        changed.translator = role.into();
        let (different, script, _) = make_service(
            cfg.clone(),
            vec![Plan::Text("another namespace")],
            Some(cache.clone()),
        );
        let different = different.with_legacy_cache(LegacyBinding::new(
            LegacyNamespace::new("openai", &legacy_params).unwrap(),
            CacheMethod::Translate,
            "main",
        ));
        assert_eq!(
            different
                .translate_typed(&changed, &cancel)
                .await
                .unwrap()
                .text,
            "another namespace"
        );
        assert_eq!(different.metrics().legacy_cache_hits, 0);
        script.drained();
    }
    assert_eq!(
        writer
            .query_row::<String, _, _>(
                "SELECT translation FROM _translationcache WHERE id=1",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "legacy translation"
    );
}

#[tokio::test]
async fn real_http_and_reopened_sqlite_cache_only_send_one_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let (end, length) = loop {
            let mut buffer = [0; 8192];
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                            .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                break (end + 4, length);
            }
        };
        while bytes.len() < end + length {
            let mut buffer = [0; 8192];
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
        }
        let request: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
        assert_eq!(request["model"], "synthetic-model");
        let body = r#"{"choices":[{"message":{"content":"real loopback"}}],"usage":{"total_tokens":8,"prompt_tokens":5,"completion_tokens":3}}"#;
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let mut cfg = config();
    cfg.endpoint = endpoint;
    let first = TranslationService::new(cfg.clone())
        .unwrap()
        .with_cache(Arc::new(CacheStore::open(&path).unwrap()));
    let req = request();
    let cancel = CancellationToken::new();
    assert_eq!(
        first.translate_typed(&req, &cancel).await.unwrap().text,
        "real loopback"
    );
    server.await.unwrap();
    drop(first);
    // The listener is now closed. A cache miss would fail on a new request.
    let reopened = TranslationService::new(cfg)
        .unwrap()
        .with_cache(Arc::new(CacheStore::open(&path).unwrap()));
    assert_eq!(
        reopened.translate_typed(&req, &cancel).await.unwrap().text,
        "real loopback"
    );
    assert_eq!(
        (
            reopened.metrics().cache_hits,
            reopened.metrics().http_attempts
        ),
        (1, 0)
    );
    assert_eq!(reopened.metrics().usage, Usage::default());
}
