use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use babeloxide_core::*;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{Notify, oneshot},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

fn legacy_contract() -> Value {
    serde_json::from_str(include_str!("fixtures/legacy_retry_contract_v1.json")).unwrap()
}

fn contract_seconds(values: &Value) -> Vec<Duration> {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|value| Duration::from_secs_f64(value.as_f64().unwrap()))
        .collect()
}

struct VirtualClock {
    origin: Instant,
    nanos: AtomicU64,
    sleeps: Mutex<Vec<Duration>>,
}
impl VirtualClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            origin: Instant::now(),
            nanos: AtomicU64::new(0),
            sleeps: Mutex::new(vec![]),
        })
    }
    fn sleeps(&self) -> Vec<Duration> {
        self.sleeps.lock().unwrap().clone()
    }
    fn advance(&self, duration: Duration) {
        self.nanos
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }
}
impl Clock for VirtualClock {
    fn now(&self) -> Instant {
        self.origin + Duration::from_nanos(self.nanos.load(Ordering::Relaxed))
    }
    fn wall_time(&self) -> SystemTime {
        UNIX_EPOCH
            + Duration::from_secs(1_700_000_000)
            + Duration::from_nanos(self.nanos.load(Ordering::Relaxed))
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
            self.advance(duration);
            Ok(())
        })
    }
}
struct FixedRandom(f64);
impl RandomSource for FixedRandom {
    fn unit(&self) -> f64 {
        self.0
    }
}

#[derive(Clone)]
enum Step {
    Reply(u16, Value, BTreeMap<String, String>),
    Error(ErrorKind),
    Raw(u16, Vec<u8>),
    Gate(Arc<Notify>, Arc<Notify>, u16, Value),
}
fn reply(status: u16, body: Value) -> Step {
    Step::Reply(status, body, BTreeMap::new())
}
fn headers(status: u16, pairs: &[(&str, &str)]) -> Step {
    Step::Reply(
        status,
        json!({"error":{"message":"local fixture failure"}}),
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    )
}
fn chat() -> Step {
    reply(
        200,
        json!({"choices":[{"message":{"content":"  測試譯文  "}}],"usage":{"total_tokens":8,"prompt_tokens":5,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":2}}}),
    )
}
struct Script {
    steps: Mutex<VecDeque<Step>>,
    requests: Mutex<Vec<HttpRequest>>,
    times: Mutex<Vec<Instant>>,
    clock: Arc<VirtualClock>,
}
impl Script {
    fn new(steps: Vec<Step>, clock: Arc<VirtualClock>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(vec![]),
            times: Mutex::new(vec![]),
            clock,
        })
    }
    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
    fn empty(&self) {
        assert_eq!(
            self.steps.lock().unwrap().len(),
            0,
            "unconsumed mock HTTP operations"
        );
    }
    fn body(&self, index: usize) -> Value {
        match &self.requests.lock().unwrap()[index].body {
            HttpBody::Json(value) => value.clone(),
            _ => Value::Null,
        }
    }
}
impl HttpExecutor for Script {
    fn send<'a>(
        &'a self,
        request: HttpRequest,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<HttpResponse>> + Send + 'a>> {
        Box::pin(async move {
            let url = request.url.clone();
            self.requests.lock().unwrap().push(request);
            self.times.lock().unwrap().push(self.clock.now());
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected mock HTTP request");
            let (status, body, headers) = match step {
                Step::Reply(status, body, headers) => (status, body, headers),
                Step::Error(kind) => {
                    return Err(ProviderError::new(kind, "synthetic transport failure"));
                }
                Step::Raw(status, body) => {
                    return Ok(HttpResponse {
                        status,
                        body,
                        headers: BTreeMap::new(),
                        elapsed: Duration::ZERO,
                        final_url: url,
                        set_cookies: vec![],
                    });
                }
                Step::Gate(entered, release, status, body) => {
                    entered.notify_one();
                    tokio::select! {biased;_ = cancel.cancelled()=>return Err(ProviderError::cancelled()),_ = release.notified()=>{}}
                    (status, body, BTreeMap::new())
                }
            };
            self.clock.advance(Duration::from_millis(1));
            Ok(HttpResponse {
                status,
                headers,
                body: serde_json::to_vec(&body).unwrap(),
                elapsed: Duration::from_millis(1),
                final_url: url,
                set_cookies: vec![],
            })
        })
    }
}
fn config(provider: &str) -> TranslatorConfig {
    TranslatorConfig {
        provider: provider.into(),
        endpoint: "http://127.0.0.1:1/v1".into(),
        api_key: "synthetic-private-key".into(),
        model: if provider == "qwen-mt" {
            "qwen-mt-plus"
        } else {
            "offline-fixture"
        }
        .into(),
        timeout_seconds: 7.0,
        lang_out: "zh-TW".into(),
        options: json!({"qps":1_000_000,"skip_preflight":true,"num_predict":1}),
    }
}
fn request() -> TranslationRequest {
    TranslationRequest {
        id: "fixture-1".into(),
        text: "private fixture source {v0}".into(),
        method: "translate".into(),
        ignore_cache: true,
        rate_limit_params: Value::Null,
        translator: "primary".into(),
    }
}
fn service(
    config: TranslatorConfig,
    steps: Vec<Step>,
    jitter: f64,
) -> (TranslationService, Arc<Script>, Arc<VirtualClock>) {
    let clock = VirtualClock::new();
    let runtime = RetryRuntime::with_sources(clock.clone(), Arc::new(FixedRandom(jitter)));
    let script = Script::new(steps, clock.clone());
    let http = HttpTransport::with_executor(runtime.clone(), script.clone());
    (
        TranslationService::new_with_transport(config, runtime, http).unwrap(),
        script,
        clock,
    )
}

#[tokio::test]
async fn openai_success_preserves_prompt_and_accounts_usage_before_malformed_choices() {
    let oracle = legacy_contract();
    let (service, script, _) = service(
        config("openai"),
        vec![
            chat(),
            reply(200, json!({"choices":[],"usage":{"total_tokens":4}})),
        ],
        0.0,
    );
    assert_eq!(
        service
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap(),
        oracle["transport"]["T01"]["text"].as_str().unwrap()
    );
    assert_eq!(
        service.metrics().usage.total_tokens,
        oracle["transport"]["T01"]["total_tokens"].as_u64().unwrap()
    );
    assert_eq!(script.body(0)["model"], oracle["transport"]["T01"]["model"]);
    assert_eq!(
        script.body(0)["messages"][0]["content"],
        format!(
            "You are a professional,authentic machine translation engine.\n\n;; Treat next line as plain text input and translate it into zh-TW, output translation ONLY. If translation is unnecessary (e.g. proper nouns, codes, {{{{1}}}}, etc. ), return the original text. NO explanations. NO notes. Input:\n\n{}",
            request().text
        )
    );
    let error = service
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::MalformedResponse);
    let metrics = service.metrics();
    assert_eq!(metrics.calls, 2);
    assert_eq!(metrics.admissions, 2);
    assert_eq!(metrics.http_attempts, 2);
    assert_eq!(metrics.usage.total_tokens, 12);
    assert_eq!(metrics.usage.cache_hit_prompt_tokens, 2);
    script.empty();
}

#[tokio::test]
async fn frozen_r29_outer_recovery_and_direct_server_error() {
    let oracle = legacy_contract();
    let r29 = &oracle["behaviors"]["R29"];
    let disabled = [("x-should-retry", "false")];
    let (translator, script, clock) = service(
        config("openai"),
        vec![headers(429, &disabled), headers(429, &disabled), chat()],
        0.0,
    );
    assert_eq!(
        translator
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap(),
        "測試譯文"
    );
    assert_eq!(
        script.requests().len(),
        r29["recovery_attempts"].as_u64().unwrap() as usize
    );
    assert_eq!(
        clock.sleeps(),
        contract_seconds(&r29["recovery_sleeps_seconds"])
    );
    script.empty();

    let (translator, script, clock) = service(config("openai"), vec![headers(500, &disabled)], 0.0);
    let error = translator
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::HttpStatus(500));
    assert_eq!(
        script.requests().len(),
        r29["direct_500_calls"].as_u64().unwrap() as usize
    );
    assert!(clock.sleeps().is_empty());
    script.empty();
}

#[tokio::test]
async fn sdk_status_retries_and_transport_retries_are_distinct_from_outer_429() {
    for status in [408, 409, 500, 503] {
        let (service, script, clock) = service(
            config("openai"),
            vec![reply(status, json!({})), reply(status, json!({})), chat()],
            0.0,
        );
        service
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(script.requests().len(), 3);
        assert_eq!(
            clock.sleeps(),
            vec![Duration::from_millis(500), Duration::from_secs(1)]
        );
        assert_eq!(service.metrics().admissions, 1);
        script.empty();
    }
    for kind in [
        ErrorKind::Connect,
        ErrorKind::ConnectTimeout,
        ErrorKind::ReadTimeout,
        ErrorKind::ReadTransport,
    ] {
        let (service, script, clock) =
            service(config("openai"), vec![Step::Error(kind.clone()); 3], 0.5);
        let error = service
            .translate_typed(&request(), &CancellationToken::new())
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind, kind);
        assert_eq!(script.requests().len(), 3);
        assert_eq!(
            clock.sleeps(),
            vec![Duration::from_micros(437_500), Duration::from_millis(875)]
        );
        script.empty();
    }
}

#[tokio::test]
async fn openai_300_http_attempts_wrap_only_the_application_layer() {
    let (service, script, clock) = service(config("openai"), vec![reply(429, json!({})); 300], 0.0);
    let error = service
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    match error.kind {
        ErrorKind::RetryExhausted {
            layer,
            attempts,
            last,
        } => {
            assert_eq!(layer, "openai.application");
            assert_eq!(attempts, 100);
            assert_eq!(last.kind, ErrorKind::HttpStatus(429));
        }
        _ => panic!("expected outer exhaustion"),
    }
    assert_eq!(script.requests().len(), 300);
    assert_eq!(clock.sleeps().len(), 299);
    assert_eq!(
        &clock.sleeps()[..6],
        &[
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(2)
        ]
    );
    assert_eq!(service.metrics().admissions, 1);
    script.empty();
}

#[tokio::test]
async fn x_should_retry_false_and_excessive_server_delay_still_allow_outer_429() {
    for pairs in [
        vec![("x-should-retry", "false")],
        vec![("retry-after", "121"), ("x-should-retry", "true")],
    ] {
        let (service, script, clock) =
            service(config("openai"), vec![headers(429, &pairs), chat()], 0.0);
        service
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(script.requests().len(), 2);
        assert_eq!(clock.sleeps(), vec![Duration::from_secs(1)]);
        script.empty();
    }
}

#[tokio::test]
async fn retry_after_numeric_overflow_dates_and_jitter_follow_pinned_sdk() {
    type HeaderPairs = Vec<(&'static str, &'static str)>;
    let cases: Vec<(HeaderPairs, Option<f64>)> = vec![
        (
            vec![("retry-after-ms", "1250"), ("retry-after", "9")],
            Some(1.25),
        ),
        (
            vec![("retry-after-ms", "bad"), ("retry-after", "1.5")],
            Some(1.5),
        ),
        (vec![("retry-after", "120")], Some(120.0)),
        (
            vec![("retry-after", "120.0001"), ("x-should-retry", "true")],
            None,
        ),
        (
            vec![("retry-after", "1e9999"), ("x-should-retry", "true")],
            None,
        ),
        (vec![("retry-after-ms", "1e9999")], None),
        (vec![("retry-after", "0")], Some(0.4375)),
        (vec![("retry-after", "-1")], Some(0.4375)),
        (vec![("retry-after", "NaN")], Some(0.4375)),
        (vec![("retry-after", "Infinity")], Some(0.4375)),
        (vec![("retry-after", "invalid")], Some(0.4375)),
        (
            vec![("retry-after-ms", "NaN"), ("retry-after", "120")],
            Some(0.4375),
        ),
    ];
    for (pairs, expected) in cases {
        let mut steps = vec![headers(500, &pairs)];
        if expected.is_some() {
            steps.push(chat());
        }
        let (service, script, clock) = service(config("openai"), steps, 0.5);
        let result = service
            .translate_typed(&request(), &CancellationToken::new())
            .await;
        match expected {
            Some(delay) => {
                assert!(result.is_ok());
                assert_eq!(clock.sleeps(), vec![Duration::from_secs_f64(delay)]);
            }
            None => {
                assert!(matches!(
                    result.err().unwrap().kind,
                    ErrorKind::HttpStatus(500)
                ));
                assert!(clock.sleeps().is_empty());
            }
        }
        script.empty();
    }
    let date = httpdate::fmt_http_date(UNIX_EPOCH + Duration::from_secs(1_700_000_002));
    let (service, script, clock) = service(
        config("openai"),
        vec![headers(500, &[("retry-after", &date)]), chat()],
        0.0,
    );
    service
        .translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(clock.sleeps(), vec![Duration::from_millis(1999)]);
    script.empty();
}

#[tokio::test]
async fn azure_qwen_and_siliconflow_preserve_their_wire_options() {
    let mut azure = config("azure-openai");
    azure.endpoint = "http://127.0.0.1:1/base".into();
    azure.model = "model with space".into();
    azure.options["api_version"] = json!("2024-test");
    let (azure, script, _) = service(azure, vec![chat()], 0.0);
    azure
        .translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    let requests = script.requests();
    assert!(
        requests[0]
            .url
            .ends_with("/base/openai/deployments/model%20with%20space/chat/completions")
    );
    assert_eq!(
        requests[0].query,
        vec![("api-version".into(), "2024-test".into())]
    );
    assert_eq!(requests[0].headers["api-key"], "synthetic-private-key");
    assert_eq!(script.body(0)["temperature"], 0);
    script.empty();
    let (qwen, script, _) = service(config("qwen-mt"), vec![chat()], 0.0);
    qwen.translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(script.body(0)["messages"][0]["content"], request().text);
    assert_eq!(
        script.body(0)["translation_options"]["source_lang"],
        "English"
    );
    assert_eq!(
        script.body(0)["translation_options"]["target_lang"],
        "Chinese"
    );
    assert_eq!(qwen.metrics().usage.total_tokens, 0);
    let mut raw = request();
    raw.method = "llm_translate".into();
    assert!(
        qwen.translate(&raw, &CancellationToken::new())
            .await
            .is_err()
    );
    script.empty();
    let mut silicon = config("siliconflow");
    silicon.options["enable_json_mode"] = json!(true);
    silicon.options["send_enable_thinking_param"] = json!(true);
    silicon.options["enable_thinking"] = json!(false);
    let (silicon, script, _) = service(silicon, vec![chat()], 0.0);
    silicon
        .translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        script.body(0)["response_format"],
        json!({"type":"json_object"})
    );
    assert_eq!(script.body(0)["enable_thinking"], false);
    assert_eq!(silicon.metrics().usage.cache_hit_prompt_tokens, 2);
    script.empty();
}

#[tokio::test]
async fn free_nested_layers_do_not_multiply_rate_limit_exhaustion() {
    let oracle = legacy_contract();
    let r30 = &oracle["behaviors"]["R30"];
    let (free, script, clock) = service(
        config("siliconflowfree"),
        vec![reply(429, json!({})); 100],
        0.0,
    );
    let error = free
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    match error.kind {
        ErrorKind::RetryExhausted {
            layer,
            attempts,
            last,
        } => {
            assert_eq!(layer, "siliconflowfree.rate_limit");
            assert_eq!(attempts, 100);
            assert_eq!(last.kind, ErrorKind::FreeRateLimit);
        }
        _ => panic!("inner exhaustion expected"),
    };
    assert_eq!(
        script.requests().len(),
        r30["rate_limit_attempts"].as_u64().unwrap() as usize
    );
    assert_eq!(
        clock.sleeps().len(),
        r30["inner_sleep_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        &clock.sleeps()[..5],
        contract_seconds(&r30["inner_first_sleeps_seconds"])
    );
    script.empty();
    let (free, script, clock) = service(
        config("siliconflowfree"),
        vec![reply(503, json!({})); 3],
        0.0,
    );
    let error = free
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    match error.kind {
        ErrorKind::RetryExhausted {
            layer,
            attempts,
            last,
        } => {
            assert_eq!(layer, "siliconflowfree.http");
            assert_eq!(
                attempts as u64,
                r30["http_error_attempts"].as_u64().unwrap()
            );
            assert_eq!(last.kind, ErrorKind::HttpStatus(503));
        }
        _ => panic!("outer HTTP exhaustion expected"),
    }
    assert_eq!(
        clock.sleeps(),
        contract_seconds(&r30["outer_sleeps_seconds"])
    );
    script.empty();
    let (free, script, clock) = service(
        config("siliconflowfree"),
        vec![
            reply(429, json!({})),
            reply(503, json!({})),
            reply(429, json!({})),
            reply(200, json!({"content":" <think>keep</think> result "})),
        ],
        0.0,
    );
    assert_eq!(
        free.translate(&request(), &CancellationToken::new())
            .await
            .unwrap(),
        " <think>keep</think> result "
    );
    assert_eq!(
        clock.sleeps(),
        vec![
            Duration::from_secs(4),
            Duration::from_secs(30),
            Duration::from_secs(4)
        ]
    );
    script.empty();
}

#[tokio::test]
async fn ollama_nonstream_shape_dynamic_predict_and_response_error_selector() {
    let good = json!({"message":{"role":"assistant","content":" <think>x</think> translated "},"prompt_eval_count":2,"eval_count":3,"error":"ignored nonstream extension"});
    let (ollama, script, clock) = service(
        config("ollama"),
        vec![
            reply(500, json!({"error":"busy"})),
            reply(200, good.clone()),
            reply(200, good),
        ],
        0.0,
    );
    let mut long = request();
    long.text = "語".repeat(40);
    assert_eq!(
        ollama
            .translate(&long, &CancellationToken::new())
            .await
            .unwrap(),
        " translated"
    );
    ollama
        .translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(script.body(0)["options"]["num_predict"], 200);
    assert_eq!(script.body(2)["options"]["num_predict"], 200);
    assert_eq!(script.requests()[0].timeout, None);
    assert_eq!(clock.sleeps()[0], Duration::from_secs(1));
    assert_eq!(ollama.metrics().usage.total_tokens, 10);
    script.empty();
    for invalid in [
        json!({"error":"body error is not ResponseError"}),
        json!({"message":{"content":"missing role"},"prompt_eval_count":1,"eval_count":1}),
    ] {
        let (ollama, script, clock) = service(config("ollama"), vec![reply(200, invalid)], 0.0);
        assert_eq!(
            ollama
                .translate_typed(&request(), &CancellationToken::new())
                .await
                .err()
                .unwrap()
                .kind,
            ErrorKind::MalformedResponse
        );
        assert_eq!(script.requests().len(), 1);
        assert!(clock.sleeps().is_empty());
        script.empty();
    }
    let (ollama, script, _) = service(config("ollama"), vec![Step::Error(ErrorKind::Connect)], 0.0);
    assert_eq!(
        ollama
            .translate_typed(&request(), &CancellationToken::new())
            .await
            .err()
            .unwrap()
            .kind,
        ErrorKind::Connect
    );
    assert_eq!(script.requests().len(), 1);
    script.empty();
}

#[tokio::test]
async fn xinference_runs_one_model_fallback_round_and_rejects_missing_auth() {
    let mut cfg = config("xinference");
    cfg.model = "first;second".into();
    cfg.options["skip_preflight"] = json!(false);
    let (xin, script, clock) = service(
        cfg.clone(),
        vec![
            reply(404, json!({})),
            reply(500, json!({})),
            reply(200, json!({"model_type":"LLM","model_ability":["chat"]})),
            reply(
                200,
                json!({"choices":[{"message":{"content":" <end_of_turn>result<end_of_turn> "}}]}),
            ),
        ],
        0.0,
    );
    assert_eq!(
        xin.translate(&request(), &CancellationToken::new())
            .await
            .unwrap(),
        "result"
    );
    assert_eq!(script.requests().len(), 4);
    assert!(script.requests()[1].url.ends_with("/v1/models/first"));
    assert!(script.requests()[2].url.ends_with("/v1/models/second"));
    assert!(clock.sleeps().is_empty());
    script.empty();
    cfg.options["skip_preflight"] = json!(true);
    let (xin, script, clock) = service(cfg.clone(), vec![reply(500, json!({})); 2], 0.0);
    assert_eq!(
        xin.translate_typed(&request(), &CancellationToken::new())
            .await
            .err()
            .unwrap()
            .kind,
        ErrorKind::Provider
    );
    assert_eq!(script.requests().len(), 2);
    assert!(clock.sleeps().is_empty());
    script.empty();
    cfg.options["skip_preflight"] = json!(false);
    let (xin, script, _) = service(cfg, vec![reply(200, json!({}))], 0.0);
    assert_eq!(
        xin.prepare(&CancellationToken::new())
            .await
            .err()
            .unwrap()
            .kind,
        ErrorKind::MalformedResponse
    );
    assert!(
        xin.translate(&request(), &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(script.requests().len(), 1);
    script.empty();
}

#[tokio::test]
async fn p04_cancelled_preflight_waiter_does_not_drop_or_repeat_initializer() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let mut cfg = config("xinference");
    cfg.options["skip_preflight"] = json!(false);
    let (service, script, _) = service(
        cfg,
        vec![Step::Gate(
            entered.clone(),
            release.clone(),
            200,
            json!({"auth":false}),
        )],
        0.0,
    );
    let service = Arc::new(service);
    let a = service.clone();
    let first = tokio::spawn(async move { a.prepare(&CancellationToken::new()).await });
    entered.notified().await;
    let b = service.clone();
    let cancel = CancellationToken::new();
    let b_cancel = cancel.clone();
    let second = tokio::spawn(async move { b.prepare(&b_cancel).await });
    tokio::task::yield_now().await;
    cancel.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(100), second)
            .await
            .unwrap()
            .unwrap()
            .err()
            .unwrap()
            .kind,
        ErrorKind::Cancelled
    );
    assert!(!first.is_finished());
    assert_eq!(script.requests().len(), 1);
    release.notify_one();
    first.await.unwrap().unwrap();
    service.prepare(&CancellationToken::new()).await.unwrap();
    script.empty();
}

#[tokio::test]
async fn p09_free_bootstrap_probes_endpoints_concurrently_and_reuses_recommendations() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let mut cfg = config("siliconflowfree");
    cfg.options["skip_preflight"] = json!(false);
    cfg.options["endpoints"] = json!(["http://127.0.0.1:1/a", "http://127.0.0.1:1/b"]);
    let mut steps = vec![Step::Gate(
        entered.clone(),
        release.clone(),
        200,
        json!({"status":"ok"}),
    )];
    steps.extend(vec![reply(200, json!({"status":"ok"})); 5]);
    steps.push(reply(
        200,
        json!({"status":"ok","qps":7,"max_pool_size":29}),
    ));
    steps.push(reply(200, json!({"content":"done"})));
    let (free, script, _) = service(cfg, steps, 0.0);
    let free = Arc::new(free);
    let a = free.clone();
    let prepare = tokio::spawn(async move { a.prepare(&CancellationToken::new()).await });
    entered.notified().await;
    tokio::task::yield_now().await;
    let calls = script.requests();
    assert_eq!(calls.len(), 4);
    assert!(
        calls[1..]
            .iter()
            .all(|request| request.url.ends_with("/b/check"))
    );
    release.notify_one();
    prepare.await.unwrap().unwrap();
    assert_eq!(free.recommendations(), (Some(7), Some(29)));
    free.prepare(&CancellationToken::new()).await.unwrap();
    assert_eq!(
        free.translate(&request(), &CancellationToken::new())
            .await
            .unwrap(),
        "done"
    );
    assert!(script.requests()[7].url.ends_with("/b"));
    script.empty();
}

#[tokio::test]
async fn p09_free_missing_status_and_invalid_shape_are_failed_probes_not_fatal() {
    for bad in [json!({}), json!({"status":"bad"}), json!("not an object")] {
        let mut cfg = config("siliconflowfree");
        cfg.options["skip_preflight"] = json!(false);
        let mut steps = vec![reply(200, bad); 3];
        steps.push(reply(503, json!({})));
        let (free, script, _) = service(cfg, steps, 0.0);
        free.prepare(&CancellationToken::new()).await.unwrap();
        assert_eq!(free.recommendations(), (Some(10), Some(100)));
        script.empty();
    }
    let mut cfg = config("siliconflowfree");
    cfg.options["skip_preflight"] = json!(false);
    let (free, script, _) = service(
        cfg,
        vec![reply(200, json!({"status":"bad","message":"unavailable"}))],
        0.0,
    );
    assert_eq!(
        free.prepare(&CancellationToken::new())
            .await
            .err()
            .unwrap()
            .kind,
        ErrorKind::Provider
    );
    script.empty();
}

#[tokio::test]
async fn p01_p02_safe_provider_errors_scrub_boundaries_headers_and_overlapping_input() {
    for (_key, value) in [
        ("api_key", "synthetic-private-key"),
        ("token", "synthetic-token"),
        ("source", "private fixture source {v0}"),
    ] {
        let mut cfg = config("openai");
        cfg.options["token"] = json!("synthetic-token");
        let message = format!("{}{}", "x".repeat(4090), value);
        let (service, script, _) = service(
            cfg,
            vec![reply(400, json!({"error":{"message":message}}))],
            0.0,
        );
        let error = service
            .translate_typed(&request(), &CancellationToken::new())
            .await
            .err()
            .unwrap();
        // The historical bug leaked only the first six characters here.
        assert!(!error.message.contains(&value[..6]));
        assert!(error.message.ends_with("[REDAC"));
        script.empty();
    }
    let mut cfg = config("openai");
    cfg.api_key = "TOKEN".into();
    cfg.options["headers"] = json!({"X-Auth":"CUSTOM-SECRET","Authorization":"Bearer BARE-SECRET"});
    let mut input = request();
    input.text = "private text TOKEN".into();
    let (service, script, _) = service(
        cfg,
        vec![reply(
            400,
            json!({"error":{"message":"local invalid model; private text TOKEN; CUSTOM-SECRET; Bearer BARE-SECRET; BARE-SECRET https://fixture.invalid/?key=hidden"}}),
        )],
        0.0,
    );
    let error = service
        .translate_typed(&input, &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert!(error.message.contains("local invalid model"));
    for secret in [
        "private text",
        "TOKEN",
        "CUSTOM-SECRET",
        "BARE-SECRET",
        "https://",
    ] {
        assert!(!format!("{error:?}").contains(secret));
    }
    assert_eq!(script.requests().len(), 1);
    script.empty();
}

#[tokio::test]
async fn debug_server_errors_keep_remote_worker_reason_and_redact_credentials() {
    let mut cfg = config("openai");
    cfg.api_key = "synthetic-upstream-secret".into();
    cfg.options["diagnostic_http"] = json!(true);
    let response = reply(
        500,
        json!({"error":{"message":"Remote Python Worker path is unavailable; synthetic-upstream-secret https://invalid/?key=hidden"}}),
    );
    let (service, script, _) = service(cfg, vec![response; 3], 0.0);
    let error = service
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::HttpStatus(500));
    assert!(
        error
            .message
            .contains("Remote Python Worker path is unavailable")
    );
    assert!(!error.message.contains("synthetic-upstream-secret"));
    assert!(!error.message.contains("https://"));
    assert_eq!(script.requests().len(), 3);
    script.empty();
}

#[tokio::test]
async fn service_process_ownership_accepts_normalized_aliases() {
    for name in [
        "CLITranslator",
        "cli-translator",
        "CLI_Translator",
        "cli translator",
        "ClaudeCode",
        "Claude-Code",
        "CLAUDE_CODE",
        "claude code",
    ] {
        let service = TranslationService::new(config(name)).unwrap();
        assert!(service.requires_reap());
        assert!(service.owns_process());
    }
    assert!(
        !TranslationService::new(config("openai"))
            .unwrap()
            .requires_reap()
    );
}

async fn wire_server(
    response: String,
) -> (
    String,
    oneshot::Receiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = vec![];
        let end = loop {
            let mut buf = [0; 1024];
            let count = stream.read(&mut buf).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buf[..count]);
            if let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
        let length = head
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while bytes.len() < end + length {
            let mut buf = [0; 1024];
            let count = stream.read(&mut buf).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buf[..count]);
        }
        let _ = tx.send(head);
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    (url, rx, task)
}
fn wire_response(body: Value) -> String {
    let body = body.to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn p03_cross_origin_307_and_308_strip_all_custom_credentials_on_the_wire() {
    for status in [307, 308] {
        let (second, second_request, second_task) = wire_server(wire_response(json!({}))).await;
        let(first,first_request,first_task)=wire_server(format!("HTTP/1.1 {status} Redirect\r\nLocation: {second}/destination\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")).await;
        let runtime = RetryRuntime::default();
        let http = HttpTransport::new(runtime, None);
        let response = http
            .send(
                HttpRequest {
                    method: reqwest::Method::POST,
                    url: first,
                    headers: BTreeMap::from([
                        ("Authorization".into(), "Bearer synthetic-token".into()),
                        ("api-key".into(), "synthetic-key".into()),
                        ("X-TC-Token".into(), "synthetic-sts".into()),
                        ("X-Auth".into(), "synthetic-custom".into()),
                    ]),
                    query: vec![],
                    body: HttpBody::Json(json!({"text":"fixture"})),
                    timeout: Some(Duration::from_secs(2)),
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(response.final_url, format!("{second}/destination"));
        let original = first_request.await.unwrap().to_ascii_lowercase();
        assert!(original.contains("synthetic-token"));
        let redirected = second_request.await.unwrap().to_ascii_lowercase();
        for header in [
            "authorization:",
            "api-key:",
            "x-tc-token:",
            "x-auth:",
            "synthetic-",
        ] {
            assert!(!redirected.contains(header));
        }
        first_task.await.unwrap();
        second_task.await.unwrap();
    }
}

#[tokio::test]
async fn p08_custom_authorization_replaces_default_case_insensitively_on_the_wire() {
    let (url, received, task) = wire_server(wire_response(
        json!({"choices":[{"message":{"content":"done"}}]}),
    ))
    .await;
    let mut cfg = config("openai");
    cfg.endpoint = url;
    cfg.options["headers"] = json!({"Authorization":"Bearer synthetic-override"});
    let service = TranslationService::new(cfg).unwrap();
    service
        .translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    let headers = received.await.unwrap().to_ascii_lowercase();
    let auth: Vec<_> = headers
        .lines()
        .filter(|line| line.starts_with("authorization:"))
        .collect();
    assert_eq!(auth, vec!["authorization: bearer synthetic-override"]);
    task.await.unwrap();
}

#[tokio::test]
async fn p06_truncated_and_malformed_chunked_bodies_are_read_transport_errors() {
    for response in [
        "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort",
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nZZ\r\nbroken\r\n",
    ] {
        let (url, received, task) = wire_server(response.into()).await;
        let http = HttpTransport::new(RetryRuntime::default(), None);
        let error = http
            .send(
                HttpRequest {
                    method: reqwest::Method::GET,
                    url,
                    headers: BTreeMap::new(),
                    query: vec![],
                    body: HttpBody::Empty,
                    timeout: Some(Duration::from_secs(2)),
                },
                &CancellationToken::new(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::ReadTransport);
        received.await.unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn p05_openai_implicit_and_explicit_read_timeouts_reach_actual_requests() {
    for (explicit, expected) in [(false, 600.0), (true, 7.0)] {
        let mut cfg = config("openai");
        cfg.options["timeout_explicit"] = json!(explicit);
        let (service, script, _) = service(cfg, vec![chat()], 0.0);
        service
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            script.requests()[0].timeout,
            Some(Duration::from_secs_f64(expected))
        );
        script.empty();
    }
}

#[tokio::test]
async fn free_failed_config_keeps_original_limiter_but_returns_default_recommendations() {
    let mut cfg = config("siliconflowfree");
    cfg.options["qps"] = json!(2);
    cfg.options["skip_preflight"] = json!(false);
    let mut steps = vec![reply(200, json!({"status":"ok"})); 3];
    steps.push(reply(500, json!({})));
    steps.extend(vec![reply(200, json!({"content":"done"})); 2]);
    let (service, script, _) = service(cfg, steps, 0.0);
    service.prepare(&CancellationToken::new()).await.unwrap();
    assert_eq!(service.recommendations(), (Some(10), Some(100)));
    service
        .translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    service
        .translate(&request(), &CancellationToken::new())
        .await
        .unwrap();
    let times = script.times.lock().unwrap();
    assert_eq!(times[5] - times[4], Duration::from_millis(500));
    script.empty();
}

#[tokio::test]
async fn free_invalid_json_probe_falls_back_after_three_checks() {
    let mut cfg = config("siliconflowfree");
    cfg.options["skip_preflight"] = json!(false);
    let mut steps = vec![Step::Raw(200, b"invalid json".to_vec()); 3];
    steps.push(reply(503, json!({})));
    let (service, script, _) = service(cfg, steps, 0.0);
    service.prepare(&CancellationToken::new()).await.unwrap();
    assert_eq!(script.requests().len(), 4);
    assert_eq!(service.recommendations(), (Some(10), Some(100)));
    script.empty();
}

#[tokio::test]
async fn qps_admission_does_not_impose_an_inflight_cap() {
    let first_entered = Arc::new(Notify::new());
    let second_entered = Arc::new(Notify::new());
    let first_release = Arc::new(Notify::new());
    let second_release = Arc::new(Notify::new());
    let body = json!({"choices":[{"message":{"content":"done"}}]});
    let mut cfg = config("openai");
    cfg.options["qps"] = json!(2);
    let (service, script, clock) = service(
        cfg,
        vec![
            Step::Gate(
                first_entered.clone(),
                first_release.clone(),
                200,
                body.clone(),
            ),
            Step::Gate(second_entered.clone(), second_release.clone(), 200, body),
        ],
        0.0,
    );
    let service = Arc::new(service);
    let one = service.clone();
    let first =
        tokio::spawn(async move { one.translate(&request(), &CancellationToken::new()).await });
    first_entered.notified().await;
    let two = service.clone();
    let second =
        tokio::spawn(async move { two.translate(&request(), &CancellationToken::new()).await });
    second_entered.notified().await;
    assert_eq!(service.metrics().inflight, 2);
    assert_eq!(service.metrics().peak_inflight, 2);
    assert_eq!(clock.sleeps(), vec![Duration::from_millis(500)]);
    first_release.notify_one();
    second_release.notify_one();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert_eq!(service.metrics().inflight, 0);
    script.empty();
}

#[tokio::test(start_paused = true)]
async fn frozen_r33_three_admissions_preserve_half_second_spacing() {
    let oracle = legacy_contract();
    let r33 = &oracle["behaviors"]["R33"];
    let limiter = RateLimiter::new(r33["qps"].as_u64().unwrap() as u32);
    let cancel = CancellationToken::new();
    let mut starts = Vec::new();
    for _ in 0..3 {
        limiter.wait(&cancel).await.unwrap();
        starts.push(Instant::now());
    }
    let intervals: Vec<_> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert_eq!(
        intervals,
        contract_seconds(&r33["three_admission_sleeps_seconds"])
    );
    assert!(limiter.set_max_qps(0).await.is_err());
}

#[tokio::test(start_paused = true)]
async fn qps_dynamic_updates_idle_spacing_and_cancelled_waiters_preserve_the_next_slot() {
    let limiter = RateLimiter::new(2);
    let cancel = CancellationToken::new();
    let start = Instant::now();
    limiter.wait(&cancel).await.unwrap();
    let queued_cancel = CancellationToken::new();
    let queued = limiter.wait(&queued_cancel);
    tokio::pin!(queued);
    tokio::select! {biased;_=&mut queued=>panic!("second admission should wait"),_ = tokio::task::yield_now()=>{}}
    queued_cancel.cancel();
    assert_eq!(queued.await.err().unwrap().kind, ErrorKind::Cancelled);
    limiter.wait(&cancel).await.unwrap();
    assert_eq!(Instant::now() - start, Duration::from_millis(500));
    assert!(limiter.set_max_qps(0).await.is_err());
    limiter.set_max_qps(4).await.unwrap();
    limiter.wait(&cancel).await.unwrap();
    assert_eq!(Instant::now() - start, Duration::from_secs(1));
    tokio::time::advance(Duration::from_secs(10)).await;
    limiter.wait(&cancel).await.unwrap();
    assert_eq!(Instant::now() - start, Duration::from_secs(11));
    limiter.wait(&cancel).await.unwrap();
    assert_eq!(Instant::now() - start, Duration::from_millis(11_250));
}

struct SuspendedClock {
    entered: Arc<Notify>,
}
impl Clock for SuspendedClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn wall_time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }
    fn sleep<'a>(
        &'a self,
        _duration: Duration,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.entered.notify_one();
            cancel.cancelled().await;
            Err(ProviderError::cancelled())
        })
    }
}

#[tokio::test]
async fn cancellation_interrupts_sdk_backoff_without_a_background_retry() {
    let entered = Arc::new(Notify::new());
    let runtime = RetryRuntime::with_sources(
        Arc::new(SuspendedClock {
            entered: entered.clone(),
        }),
        Arc::new(FixedRandom(0.0)),
    );
    let script = Script::new(vec![reply(429, json!({})), chat()], VirtualClock::new());
    let transport = HttpTransport::with_executor(runtime.clone(), script.clone());
    let service = Arc::new(
        TranslationService::new_with_transport(config("openai"), runtime, transport).unwrap(),
    );
    let cancel = CancellationToken::new();
    let operation_cancel = cancel.clone();
    let operation_service = service.clone();
    let operation = tokio::spawn(async move {
        operation_service
            .translate_typed(&request(), &operation_cancel)
            .await
    });
    entered.notified().await;
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_millis(100), operation)
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert_eq!(script.requests().len(), 1);
    assert_eq!(script.steps.lock().unwrap().len(), 1);
    assert_eq!(service.metrics().inflight, 0);
}

#[tokio::test]
async fn cancellation_interrupts_active_http_without_becoming_retryable() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (service, script, _) = service(
        config("openai"),
        vec![Step::Gate(entered.clone(), release, 200, json!({})), chat()],
        0.0,
    );
    let service = Arc::new(service);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let target = service.clone();
    let operation = tokio::spawn(async move { target.translate_typed(&request(), &token).await });
    entered.notified().await;
    cancel.cancel();
    assert_eq!(
        operation.await.unwrap().err().unwrap().kind,
        ErrorKind::Cancelled
    );
    assert_eq!(script.requests().len(), 1);
    assert_eq!(service.metrics().inflight, 0);
}

#[tokio::test]
async fn source_and_credentials_use_original_overlapping_ranges_in_both_directions() {
    for (key, source, message) in [
        ("TOKEN", "private text TOKEN", "private text TOKEN"),
        ("synthetic-private-key", "private", "synthetic-private-key"),
        ("abcdef", "defghi", "abcdefghi"),
        ("aba", "unrelated", "ababa"),
    ] {
        let mut cfg = config("openai");
        cfg.api_key = key.into();
        let mut input = request();
        input.text = source.into();
        let (service, script, _) = service(
            cfg,
            vec![reply(
                400,
                json!({"error":{"message":format!("local invalid model: {message}")}}),
            )],
            0.0,
        );
        let error = service
            .translate_typed(&input, &CancellationToken::new())
            .await
            .err()
            .unwrap();
        assert_eq!(
            error.message,
            "translation provider returned HTTP 400: local invalid model: [REDACTED]"
        );
        script.empty();
    }
}

#[tokio::test]
async fn sdk_jitter_extremes_apply_to_real_retry_sequences() {
    let oracle = legacy_contract();
    let sdk = &oracle["sdk"]["openai"];
    let first = sdk["backoff_first_seconds"].as_f64().unwrap();
    let fraction = sdk["jitter_fraction"].as_f64().unwrap();
    for unit in [0.0, 0.5, 1.0 - f64::EPSILON] {
        let (service, script, clock) = service(
            config("openai"),
            vec![reply(500, json!({})), reply(500, json!({})), chat()],
            unit,
        );
        service
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            script.requests().len(),
            sdk["max_retries"].as_u64().unwrap() as usize + 1
        );
        assert_eq!(
            clock.sleeps(),
            vec![
                Duration::from_secs_f64(first * (1.0 - fraction * unit)),
                Duration::from_secs_f64(2.0 * first * (1.0 - fraction * unit))
            ]
        );
        script.empty();
    }
}

#[tokio::test]
async fn each_openai_family_method_keeps_the_100_attempt_outer_policy() {
    let oracle = legacy_contract();
    for (provider, module) in [
        ("openai", "openai"),
        ("azure-openai", "azureopenai"),
        ("qwen-mt", "qwenmt"),
        ("siliconflow", "siliconflow"),
    ] {
        for method in ["translate", "llm_translate"] {
            if provider == "qwen-mt" && method == "llm_translate" {
                continue;
            }
            let python_method = format!("do_{method}");
            let row = oracle["policies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["module"] == module && row["method"] == python_method)
                .unwrap();
            let attempts = row["attempts"].as_u64().unwrap() as usize;
            let steps = vec![headers(429, &[("x-should-retry", "false")]); attempts];
            let (service, script, clock) = service(config(provider), steps, 0.0);
            let mut input = request();
            input.method = method.into();
            let error = service
                .translate_typed(&input, &CancellationToken::new())
                .await
                .err()
                .unwrap();
            match error.kind {
                ErrorKind::RetryExhausted {
                    attempts: actual,
                    last,
                    ..
                } => {
                    assert_eq!(actual as usize, attempts);
                    assert_eq!(last.kind, ErrorKind::HttpStatus(429));
                }
                _ => panic!("outer exhaustion expected"),
            }
            assert_eq!(script.requests().len(), attempts);
            let sleeps = clock.sleeps();
            assert_eq!(sleeps.len(), attempts - 1);
            let expected_waits = contract_seconds(&row["probe_waits_seconds"]);
            assert_eq!(&sleeps[..expected_waits.len()], expected_waits);
            assert_eq!(service.metrics().admissions, 1);
            script.empty();
        }
    }
}

#[tokio::test]
async fn ollama_both_methods_retry_all_response_status_errors_100_times() {
    let oracle = legacy_contract();
    for method in ["translate", "llm_translate"] {
        let python_method = format!("do_{method}");
        let row = oracle["policies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["module"] == "ollama" && row["method"] == python_method)
            .unwrap();
        let expected_attempts = row["attempts"].as_u64().unwrap() as usize;
        let (service, script, clock) = service(
            config("ollama"),
            vec![reply(400, json!({"error":"model unavailable"})); expected_attempts],
            0.0,
        );
        let mut input = request();
        input.method = method.into();
        let error = service
            .translate_typed(&input, &CancellationToken::new())
            .await
            .err()
            .unwrap();
        match error.kind {
            ErrorKind::RetryExhausted {
                layer,
                attempts,
                last,
            } => {
                assert_eq!(layer, "ollama.application");
                assert_eq!(attempts as usize, expected_attempts);
                assert_eq!(last.kind, ErrorKind::OllamaResponse(400));
            }
            _ => panic!("Ollama outer exhaustion expected"),
        }
        let sleeps = clock.sleeps();
        assert_eq!(sleeps.len(), expected_attempts - 1);
        let expected_waits = contract_seconds(&row["probe_waits_seconds"]);
        assert_eq!(&sleeps[..expected_waits.len()], expected_waits);
        assert_eq!(script.requests().len(), expected_attempts);
        script.empty();
    }
}

#[tokio::test]
async fn google_public_service_retries_once_per_logical_admission_and_returns_400_sentinel() {
    let (service, script, clock) = service(
        config("google"),
        vec![
            headers(429, &[("retry-after", "120")]),
            Step::Raw(
                200,
                b"<div class=\"result-container\">translated</div>".to_vec(),
            ),
        ],
        0.0,
    );
    assert_eq!(
        service
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap(),
        "translated"
    );
    let metrics = service.metrics();
    assert_eq!(
        (metrics.calls, metrics.admissions, metrics.http_attempts),
        (1, 1, 2)
    );
    assert_eq!(clock.sleeps(), vec![Duration::from_secs(1)]);
    script.empty();
    let (service, script, clock) =
        self::service(config("google"), vec![reply(400, json!({}))], 0.0);
    assert_eq!(
        service
            .translate(&request(), &CancellationToken::new())
            .await
            .unwrap(),
        "IRREPARABLE TRANSLATION ERROR"
    );
    let metrics = service.metrics();
    assert_eq!(
        (metrics.calls, metrics.admissions, metrics.http_attempts),
        (1, 1, 1)
    );
    assert!(clock.sleeps().is_empty());
    script.empty();
}

#[tokio::test]
async fn xinference_both_public_methods_make_one_fallback_round_without_repacking_raw_input() {
    for method in ["translate", "llm_translate"] {
        let mut cfg = config("xinference");
        cfg.model = "first;second".into();
        let mut input = request();
        input.method = method.into();
        let (service, script, clock) = service(
            cfg.clone(),
            vec![
                reply(500, json!({})),
                reply(200, json!({"model_type":"LLM","model_ability":["chat"]})),
                reply(200, json!({"choices":[{"message":{"content":"done"}}]})),
            ],
            0.0,
        );
        assert_eq!(
            service
                .translate(&input, &CancellationToken::new())
                .await
                .unwrap(),
            "done"
        );
        assert_eq!(script.requests().len(), 3);
        assert_eq!(script.body(2)["model"], "second");
        let content = script.body(2)["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_owned();
        if method == "llm_translate" {
            assert_eq!(content, input.text);
        } else {
            assert!(content.starts_with("You are a professional,authentic"));
            assert!(content.ends_with(&input.text));
        }
        assert!(clock.sleeps().is_empty());
        script.empty();
        let (failed, script, clock) = self::service(cfg, vec![reply(500, json!({})); 2], 0.0);
        assert_eq!(
            failed
                .translate_typed(&input, &CancellationToken::new())
                .await
                .err()
                .unwrap()
                .kind,
            ErrorKind::Provider
        );
        assert_eq!(script.requests().len(), 2);
        assert!(clock.sleeps().is_empty());
        script.empty();
    }
}

async fn consume_wire_request(stream: &mut tokio::net::TcpStream) {
    let mut bytes = Vec::new();
    let (end, length) = loop {
        let mut buffer = [0; 1024];
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(&bytes[..end]);
            let length = text
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            break (end + 4, length);
        }
    };
    while bytes.len() < end + length {
        let mut buffer = [0; 1024];
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
    }
}

#[tokio::test]
async fn free_httpx_no_follow_policy_returns_3xx_to_outer_retry_on_real_http() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/destination", destination.local_addr().unwrap());
    let destination_calls = Arc::new(AtomicU64::new(0));
    let calls = destination_calls.clone();
    let stop = CancellationToken::new();
    let destination_stop = stop.clone();
    let destination_task = tokio::spawn(async move {
        loop {
            let connection = tokio::select! {_ = destination_stop.cancelled()=>return,result=destination.accept()=>result.unwrap()};
            let (mut stream, _) = connection;
            calls.fetch_add(1, Ordering::Relaxed);
            consume_wire_request(&mut stream).await;
            stream
                .write_all(
                    wire_response(json!({"content":"unexpected redirected success"})).as_bytes(),
                )
                .await
                .unwrap();
        }
    });
    let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", source.local_addr().unwrap());
    let source_task = tokio::spawn(async move {
        for _ in 0..3 {
            let (mut stream, _) = source.accept().await.unwrap();
            consume_wire_request(&mut stream).await;
            stream.write_all(format!("HTTP/1.1 307 Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        }
    });
    let mut cfg = config("siliconflowfree");
    cfg.endpoint = endpoint;
    let clock = VirtualClock::new();
    let runtime = RetryRuntime::with_sources(clock.clone(), Arc::new(FixedRandom(0.0)));
    let free = TranslationService::new_with_runtime(cfg, runtime).unwrap();
    let error = free
        .translate_typed(&request(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    match error.kind {
        ErrorKind::RetryExhausted {
            layer,
            attempts,
            last,
        } => {
            assert_eq!(layer, "siliconflowfree.http");
            assert_eq!(attempts, 3);
            assert_eq!(last.kind, ErrorKind::HttpStatus(307));
        }
        _ => panic!("Free outer HTTP exhaustion expected"),
    }
    assert_eq!(destination_calls.load(Ordering::Relaxed), 0);
    assert_eq!(free.metrics().http_attempts, 3);
    assert_eq!(clock.sleeps(), vec![Duration::from_secs(30); 2]);
    source_task.await.unwrap();
    stop.cancel();
    destination_task.await.unwrap();
}
