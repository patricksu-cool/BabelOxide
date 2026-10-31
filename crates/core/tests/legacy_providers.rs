//! Offline provider contracts: real loopback HTTP, injected I/O failures and
//! controlled native subprocesses. These tests never load application settings,
//! user API keys or a real Claude installation.

use pdf2zh_core::{
    Clock, ErrorKind, HttpBody, HttpExecutor, HttpRequest, HttpResponse, HttpTransport,
    ProviderContext, ProviderError, ProviderOutput, ProviderResult, RandomSource, RetryRuntime,
    TranslationRequest, TranslatorConfig, providers::legacy,
};
use reqwest::Method;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

const DEEPL_OK: &str = r#"{"translations":[{"text":"譯文","billed_characters":6}]}"#;
const AZURE_OK: &str = r#"[{"translations":[{"text":"譯文"}]}]"#;
const BING_SID: &str = r#"{"ig":"IG-fixture"} data-iid="old" data-iid="IID-fixture" params_AbusePreventionHelper = [12345,"token-fixture",0]"#;

struct TestClock {
    base: Instant,
    elapsed: Mutex<Duration>,
    sleeps: Mutex<Vec<Duration>>,
    block_sleep: AtomicBool,
}

impl TestClock {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
            sleeps: Mutex::new(Vec::new()),
            block_sleep: AtomicBool::new(false),
        }
    }
    fn advance(&self, duration: Duration) {
        *self.elapsed.lock().unwrap() += duration;
    }
    fn seconds(&self) -> Vec<f64> {
        self.sleeps
            .lock()
            .unwrap()
            .iter()
            .map(Duration::as_secs_f64)
            .collect()
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
            self.sleeps.lock().unwrap().push(duration);
            if self.block_sleep.load(Ordering::SeqCst) {
                cancel.cancelled().await;
                return Err(ProviderError::cancelled());
            }
            self.advance(duration);
            tokio::task::yield_now().await;
            if cancel.is_cancelled() {
                Err(ProviderError::cancelled())
            } else {
                Ok(())
            }
        })
    }
}
struct TestRandom(Mutex<VecDeque<f64>>);
impl RandomSource for TestRandom {
    fn unit(&self) -> f64 {
        self.0.lock().unwrap().pop_front().unwrap_or(0.5)
    }
}

#[derive(Clone)]
struct MockStep {
    status: u16,
    body: String,
    headers: BTreeMap<String, String>,
    error: Option<ErrorKind>,
    elapsed: Duration,
    hang: bool,
    final_url: Option<String>,
}
impl MockStep {
    fn reply(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.into(),
            headers: BTreeMap::new(),
            error: None,
            elapsed: Duration::ZERO,
            hang: false,
            final_url: None,
        }
    }
    fn header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
    fn error(kind: ErrorKind) -> Self {
        let mut step = Self::reply(0, "");
        step.error = Some(kind);
        step
    }
    fn elapsed(mut self, seconds: f64) -> Self {
        self.elapsed = Duration::from_secs_f64(seconds);
        self
    }
    fn hang() -> Self {
        let mut step = Self::reply(0, "");
        step.hang = true;
        step
    }
}
struct Script {
    steps: Mutex<VecDeque<MockStep>>,
    requests: Mutex<Vec<HttpRequest>>,
    clock: Arc<TestClock>,
    connect_timeouts: Mutex<Vec<Option<Duration>>>,
}
impl Script {
    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
    fn drained(&self) {
        assert!(
            self.steps.lock().unwrap().is_empty(),
            "script has unconsumed responses"
        );
    }
}
impl HttpExecutor for Script {
    fn send_with_connect_timeout<'a>(
        &'a self,
        request: HttpRequest,
        timeout: Option<Duration>,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<HttpResponse>> + Send + 'a>> {
        self.connect_timeouts.lock().unwrap().push(timeout);
        self.send(request, cancel)
    }

    fn send<'a>(
        &'a self,
        request: HttpRequest,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<HttpResponse>> + Send + 'a>> {
        Box::pin(async move {
            let url = request.url.clone();
            self.requests.lock().unwrap().push(request);
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected additional HTTP request");
            if step.hang {
                cancel.cancelled().await;
                return Err(ProviderError::cancelled());
            }
            self.clock.advance(step.elapsed);
            if let Some(kind) = step.error {
                return Err(ProviderError::new(kind, "injected I/O failure"));
            }
            Ok(HttpResponse {
                status: step.status,
                body: step.body.into_bytes(),
                headers: step.headers,
                elapsed: step.elapsed,
                final_url: step.final_url.unwrap_or(url),
                set_cookies: Vec::new(),
            })
        })
    }
}

struct Harness {
    config: TranslatorConfig,
    request: TranslationRequest,
    runtime: RetryRuntime,
    clock: Arc<TestClock>,
    random: Arc<TestRandom>,
    http: HttpTransport,
    cancel: CancellationToken,
    script: Option<Arc<Script>>,
}
impl Harness {
    fn config(provider: &str, endpoint: &str) -> TranslatorConfig {
        TranslatorConfig {
            provider: provider.into(),
            endpoint: endpoint.into(),
            api_key: "fixture-key".into(),
            model: "fixture-model".into(),
            timeout_seconds: 1.0,
            lang_out: "zh-tw".into(),
            options: json!({"lang_in":"en"}),
        }
    }
    fn new(provider: &str, steps: Vec<MockStep>) -> Self {
        let clock = Arc::new(TestClock::new());
        let random = Arc::new(TestRandom(Mutex::new(VecDeque::new())));
        let runtime = RetryRuntime::with_sources(clock.clone(), random.clone());
        let script = Arc::new(Script {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(Vec::new()),
            clock: clock.clone(),
            connect_timeouts: Mutex::new(Vec::new()),
        });
        let http = HttpTransport::with_executor(runtime.clone(), script.clone());
        Self {
            config: Self::config(provider, "http://127.0.0.1:12345/translator"),
            request: TranslationRequest {
                id: "fixture".into(),
                text: "source text".into(),
                method: "translate".into(),
                ignore_cache: true,
                rate_limit_params: Value::Null,
                translator: "primary".into(),
            },
            runtime,
            clock,
            random,
            http,
            cancel: CancellationToken::new(),
            script: Some(script),
        }
    }
    fn loopback(provider: &str, endpoint: &str) -> Self {
        let mut harness = Self::new(provider, Vec::new());
        harness.config.endpoint = endpoint.into();
        harness.http = HttpTransport::new(harness.runtime.clone(), None)
            .with_redirect_limit(if provider == "azure" { 0 } else { 30 });
        harness.script = None;
        harness
    }
    fn context(&self) -> ProviderContext<'_> {
        ProviderContext {
            config: &self.config,
            request: &self.request,
            http: &self.http,
            runtime: &self.runtime,
            cancel: &self.cancel,
        }
    }
    async fn run(&self) -> ProviderResult<ProviderOutput> {
        legacy::dispatch(&self.context()).await
    }
    async fn run_service(&self) -> ProviderResult<ProviderOutput> {
        pdf2zh_core::TranslationService::new_with_runtime(self.config.clone(), self.runtime.clone())
            .unwrap()
            .translate_typed(&self.request, &self.cancel)
            .await
    }
    fn requests(&self) -> Vec<HttpRequest> {
        self.script.as_ref().unwrap().requests()
    }
    fn drained(&self) {
        self.script.as_ref().unwrap().drained();
    }
}

fn last_error<'a>(error: &'a ProviderError, layer: &str, attempts: u32) -> &'a ProviderError {
    match &error.kind {
        ErrorKind::RetryExhausted {
            layer: actual,
            attempts: actual_attempts,
            last,
        } => {
            assert_eq!(*actual, layer);
            assert_eq!(*actual_attempts, attempts);
            last
        }
        kind => panic!("expected exhausted retry, got {kind:?}"),
    }
}
fn form_body(request: &HttpRequest) -> BTreeMap<String, String> {
    match &request.body {
        HttpBody::Form(value) => value.iter().cloned().collect(),
        _ => panic!("expected form body"),
    }
}
async fn eventually(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("fixture observation timed out");
}

#[derive(Clone)]
struct Recorded {
    line: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}
#[derive(Clone)]
struct WireStep {
    status: u16,
    body: String,
    headers: Vec<(String, String)>,
    truncate: bool,
    hang: bool,
}
impl WireStep {
    fn reply(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.into(),
            headers: Vec::new(),
            truncate: false,
            hang: false,
        }
    }
    fn header(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }
}
struct Loopback {
    endpoint: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    expected: usize,
}
impl Loopback {
    async fn start(steps: Vec<WireStep>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let recorded = requests.clone();
        let expected = steps.len();
        let task = tokio::spawn(async move {
            for step in steps {
                let (mut stream, _) = tokio::select! { _ = task_stop.cancelled() => return, result = listener.accept() => result.unwrap() };
                let mut bytes = Vec::new();
                let (header_end, content_length) = loop {
                    let mut buffer = [0; 8192];
                    let read = stream.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "HTTP request ended before headers");
                    bytes.extend_from_slice(&buffer[..read]);
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
                while bytes.len() < header_end + content_length {
                    let mut buffer = [0; 8192];
                    let read = stream.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "HTTP request ended before body");
                    bytes.extend_from_slice(&buffer[..read]);
                }
                let header_text = String::from_utf8_lossy(&bytes[..header_end]);
                let mut lines = header_text.lines();
                let line = lines.next().unwrap().to_owned();
                let headers = lines
                    .filter_map(|line| {
                        line.split_once(':')
                            .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_owned()))
                    })
                    .collect();
                recorded.lock().unwrap().push(Recorded {
                    line,
                    headers,
                    body: bytes[header_end..header_end + content_length].to_vec(),
                });
                if step.hang {
                    task_stop.cancelled().await;
                    return;
                }
                let length = step.body.len() + if step.truncate { 100 } else { 0 };
                let mut response = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n",
                    step.status, length
                );
                for (key, value) in step.headers {
                    response.push_str(&format!("{key}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&step.body);
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            task,
            expected,
        }
    }
    fn observed(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
    async fn finish(self) -> Vec<Recorded> {
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
        let requests = self.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), self.expected, "HTTP script was not drained");
        requests
    }
}

#[tokio::test]
async fn google_real_http_query_html_entities_controls_and_sentinel() {
    let server = Loopback::start(vec![
        WireStep::reply(
            200,
            "<div class=\"result-container\">A&amp;B&#x1F600;\n\u{200b}\u{e000}</div>",
        ),
        WireStep::reply(400, "invalid request"),
    ])
    .await;
    let mut harness = Harness::loopback("google", &format!("{}/m", server.endpoint));
    harness.config.lang_out = "zh".into();
    harness.request.text = "😀".repeat(5001);
    assert_eq!(harness.run().await.unwrap().text, "A&B😀");
    assert_eq!(
        harness.run().await.unwrap().text,
        "IRREPARABLE TRANSLATION ERROR"
    );
    assert!(harness.clock.seconds().is_empty());
    let requests = server.finish().await;
    let target = requests[0].line.split_whitespace().nth(1).unwrap();
    let parsed = reqwest::Url::parse(&format!("http://fixture{target}")).unwrap();
    let query: BTreeMap<_, _> = parsed.query_pairs().into_owned().collect();
    assert_eq!(query["q"].chars().count(), 5000);
    assert_eq!(query["tl"], "zh-CN");
    assert_eq!(query["sl"], "en");
    assert!(requests[0].headers["user-agent"].contains("MSIE 6.0"));
}

#[tokio::test]
async fn google_status_timeout_and_parse_failures_have_exact_outer_attempts() {
    let contract: Value =
        serde_json::from_str(include_str!("fixtures/legacy_retry_contract_v1.json")).unwrap();
    for (id, step, expected) in [
        (
            "R24",
            MockStep::reply(429, "limited").header("retry-after", "120"),
            ErrorKind::HttpStatus(429),
        ),
        (
            "R25",
            MockStep::reply(503, "down"),
            ErrorKind::HttpStatus(503),
        ),
        (
            "R26",
            MockStep::error(ErrorKind::ReadTimeout),
            ErrorKind::ReadTimeout,
        ),
    ] {
        let oracle = &contract["behaviors"][id];
        let attempts = oracle["attempts"].as_u64().unwrap() as usize;
        let harness = Harness::new("google", vec![step; attempts]);
        let error = harness.run().await.err().unwrap();
        let cause = last_error(&error, "google.application", attempts as u32);
        assert_eq!(cause.kind, expected);
        let waits: Vec<f64> = oracle["sleeps_seconds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_f64().unwrap())
            .collect();
        assert_eq!(harness.clock.seconds(), waits);
        assert_eq!(harness.requests().len(), attempts);
        assert!(
            harness
                .requests()
                .iter()
                .all(|request| request.timeout.is_none())
        );
        harness.drained();
    }
    let harness = Harness::new(
        "google",
        vec![MockStep::reply(200, "no translation container"); 5],
    );
    let error = harness.run().await.err().unwrap();
    assert_eq!(
        last_error(&error, "google.application", 5).kind,
        ErrorKind::MalformedResponse
    );
    harness.drained();
    let harness = Harness::new("google", vec![MockStep::error(ErrorKind::Interrupt)]);
    assert!(matches!(
        harness.run().await.err().unwrap().kind,
        ErrorKind::Interrupt
    ));
    assert!(harness.clock.seconds().is_empty());
    harness.drained();
}

#[tokio::test]
async fn google_real_http_429_recovery_ignores_retry_after() {
    let server = Loopback::start(vec![
        WireStep::reply(429, "limited").header("Retry-After", "120"),
        WireStep::reply(200, "<div class=\"t0\"> recovered </div>"),
    ])
    .await;
    let harness = Harness::loopback("google", &format!("{}/m", server.endpoint));
    assert_eq!(harness.run().await.unwrap().text, " recovered ");
    assert_eq!(harness.clock.seconds(), vec![1.0]);
    server.finish().await;
}

#[tokio::test]
async fn bing_real_http_redirect_cookie_last_iid_and_unicode_limit() {
    let server = Loopback::start(vec![
        WireStep::reply(302, "")
            .header("Location", "/regional/translator")
            .header("Set-Cookie", "session=fixture; Path=/"),
        WireStep::reply(200, BING_SID),
        WireStep::reply(200, AZURE_OK),
    ])
    .await;
    let mut harness = Harness::loopback("bing", &format!("{}/translator", server.endpoint));
    harness.config.options["lang_in"] = "auto".into();
    harness.request.text = "😀".repeat(1001);
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    let requests = server.finish().await;
    assert!(
        requests[2]
            .line
            .starts_with("POST /regional/ttranslatev3?IG=IG-fixture&IID=IID-fixture ")
    );
    assert!(requests[2].headers["cookie"].contains("session=fixture"));
    let form: BTreeMap<_, _> = reqwest::Url::parse(&format!(
        "http://fixture/?{}",
        String::from_utf8_lossy(&requests[2].body)
    ))
    .unwrap()
    .query_pairs()
    .into_owned()
    .collect();
    assert_eq!(form["text"].chars().count(), 1000);
    assert_eq!(form["fromLang"], "en");
    assert_eq!(form["to"], "zh-Hant");
    assert_eq!(form["token"], "token-fixture");
    assert_eq!(form["key"], "12345");
}

#[tokio::test]
async fn bing_refetches_sid_every_attempt_and_predicate_retries_interrupt() {
    let mut steps = Vec::new();
    for _ in 0..5 {
        steps.push(MockStep::reply(200, BING_SID));
        steps.push(MockStep::reply(429, "limited"));
    }
    let harness = Harness::new("bing", steps);
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "bing.application", 5).kind,
        ErrorKind::HttpStatus(429)
    ));
    assert_eq!(
        harness
            .requests()
            .iter()
            .filter(|request| request.method == reqwest::Method::GET)
            .count(),
        5
    );
    assert_eq!(
        harness
            .requests()
            .iter()
            .filter(|request| request.method == reqwest::Method::POST)
            .count(),
        5
    );
    assert!(
        harness
            .requests()
            .iter()
            .all(|request| request.timeout.is_none())
    );
    assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
    harness.drained();

    let harness = Harness::new(
        "bing",
        vec![
            MockStep::error(ErrorKind::Interrupt),
            MockStep::reply(200, BING_SID),
            MockStep::reply(200, AZURE_OK),
        ],
    );
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert_eq!(harness.requests().len(), 3);
    assert_eq!(harness.clock.seconds(), vec![1.0]);
    let form = form_body(&harness.requests()[2]);
    assert_eq!(form["text"], "source text");
    harness.drained();
}

#[tokio::test]
async fn anythingllm_and_dify_real_http_preserve_distinct_payloads_and_whitespace() {
    for (provider, body, expected) in [
        ("anythingllm", r#"{"textResponse":"  譯文  "}"#, "譯文"),
        (
            "dify",
            r#"{"data":{"outputs":{"text":"  譯文  "}}}"#,
            "  譯文  ",
        ),
    ] {
        let server = Loopback::start(vec![WireStep::reply(200, body)]).await;
        let harness = Harness::loopback(provider, &format!("{}/api/translation", server.endpoint));
        assert_eq!(harness.run().await.unwrap().text, expected);
        let requests = server.finish().await;
        assert_eq!(requests[0].headers["authorization"], "Bearer fixture-key");
        let value: Value = serde_json::from_slice(&requests[0].body).unwrap();
        if provider == "anythingllm" {
            assert_eq!(value["message"][0]["role"], "user");
            assert!(
                value["message"][0]["content"]
                    .as_str()
                    .unwrap()
                    .ends_with("source text")
            );
            assert_eq!(value["mode"], "chat");
            assert_eq!(value["sessionId"], "translation_expert");
        } else {
            assert_eq!(
                value["inputs"],
                json!({"lang_in":"en","lang_out":"zh-tw","text":"source text"})
            );
            assert_eq!(value["response_mode"], "blocking");
            assert_eq!(value["user"], "translator-service");
        }
    }
}

#[tokio::test]
async fn anythingllm_and_dify_retry_http_parse_and_nonstring_output() {
    for provider in ["anythingllm", "dify"] {
        for step in [
            MockStep::reply(503, "down"),
            MockStep::reply(200, "not json"),
            MockStep::reply(200, "{}"),
            MockStep::reply(
                200,
                r#"{"textResponse":null,"data":{"outputs":{"text":[]}}}"#,
            ),
        ] {
            let harness = Harness::new(provider, vec![step; 5]);
            let error = harness.run().await.err().unwrap();
            assert!(matches!(
                last_error(&error, &format!("{provider}.application"), 5).kind,
                ErrorKind::HttpStatus(503) | ErrorKind::MalformedResponse
            ));
            assert_eq!(harness.requests().len(), 5);
            assert!(
                harness
                    .requests()
                    .iter()
                    .all(|request| request.timeout == Some(Duration::from_secs(60)))
            );
            assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
            harness.drained();
        }
    }
}

#[tokio::test]
async fn app_provider_cookies_stay_within_one_redirect_chain() {
    for (provider, body) in [
        ("anythingllm", r#"{"textResponse":"translated"}"#),
        ("dify", r#"{"data":{"outputs":{"text":"translated"}}}"#),
    ] {
        let server = Loopback::start(vec![
            WireStep::reply(302, "")
                .header("Location", "/continued")
                .header("Set-Cookie", "fixture=chain; Path=/"),
            WireStep::reply(200, body),
            WireStep::reply(302, "")
                .header("Location", "/continued")
                .header("Set-Cookie", "fixture=chain; Path=/"),
            WireStep::reply(200, body),
        ])
        .await;
        let harness = Harness::loopback(provider, &format!("{}/start", server.endpoint));
        let service = pdf2zh_core::TranslationService::new_with_runtime(
            harness.config.clone(),
            harness.runtime.clone(),
        )
        .unwrap();
        for _ in 0..2 {
            assert_eq!(
                service
                    .translate_typed(&harness.request, &harness.cancel)
                    .await
                    .unwrap()
                    .text,
                "translated"
            );
        }
        let requests = server.finish().await;
        assert!(!requests[0].headers.contains_key("cookie"));
        assert_eq!(requests[1].headers["cookie"], "fixture=chain");
        assert!(
            !requests[2].headers.contains_key("cookie"),
            "{provider} retained a prior operation's cookie"
        );
        assert_eq!(requests[3].headers["cookie"], "fixture=chain");
    }
}

#[tokio::test]
async fn deepl_prepared_requests_omit_session_cookies_until_redirect() {
    let body = r#"{"translations":[{"text":"translated","billed_characters":1}]}"#;
    let server = Loopback::start(vec![
        WireStep::reply(200, body).header("Set-Cookie", "session=retained; Path=/"),
        WireStep::reply(302, "").header("Location", "/continued"),
        WireStep::reply(200, body),
        WireStep::reply(200, body),
    ])
    .await;
    let harness = Harness::loopback("deepl", &server.endpoint);
    let service = pdf2zh_core::TranslationService::new_with_runtime(
        harness.config.clone(),
        harness.runtime.clone(),
    )
    .unwrap();
    for _ in 0..3 {
        assert_eq!(
            service
                .translate_typed(&harness.request, &harness.cancel)
                .await
                .unwrap()
                .text,
            "translated"
        );
    }
    let requests = server.finish().await;
    assert!(!requests[0].headers.contains_key("cookie"));
    assert!(!requests[1].headers.contains_key("cookie"));
    assert_eq!(requests[2].headers["cookie"], "session=retained");
    assert!(!requests[3].headers.contains_key("cookie"));
}

#[tokio::test]
async fn session_transport_retains_cookies_across_operations() {
    let server = Loopback::start(vec![
        WireStep::reply(200, "first").header("Set-Cookie", "session=retained; Path=/"),
        WireStep::reply(200, "second"),
    ])
    .await;
    let http = HttpTransport::new(RetryRuntime::default(), None);
    for _ in 0..2 {
        http.send(
            HttpRequest {
                method: Method::GET,
                url: server.endpoint.clone(),
                headers: BTreeMap::new(),
                query: vec![],
                body: HttpBody::Empty,
                timeout: Some(Duration::from_secs(2)),
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    }
    let requests = server.finish().await;
    assert!(!requests[0].headers.contains_key("cookie"));
    assert_eq!(requests[1].headers["cookie"], "session=retained");
}

#[tokio::test]
async fn deepl_real_http_auth_json_and_full_language_mapping() {
    for (source, target, expected_source, expected_target) in [
        ("auto", "zh", None, "ZH-HANS"),
        ("zh-tw", "zh-hk", Some("ZH"), "ZH-HANT"),
        ("pt-br", "pt", Some("PT"), "PT-PT"),
        ("no", "en", Some("NB"), "EN-US"),
    ] {
        let server = Loopback::start(vec![WireStep::reply(200, DEEPL_OK)]).await;
        let mut harness = Harness::loopback("deepl", &server.endpoint);
        harness.config.options["lang_in"] = source.into();
        harness.config.lang_out = target.into();
        assert_eq!(harness.run().await.unwrap().text, "譯文");
        let requests = server.finish().await;
        assert!(requests[0].line.starts_with("POST /v2/translate "));
        assert_eq!(
            requests[0].headers["authorization"],
            "DeepL-Auth-Key fixture-key"
        );
        let value: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(value["target_lang"], expected_target);
        assert_eq!(value["source_lang"].as_str(), expected_source);
        assert_eq!(value["text"], json!(["source text"]));
        assert_eq!(value["show_billed_characters"], true);
    }
}

#[tokio::test]
async fn deepl_nested_exhaustion_has_thirty_http_attempts_and_exact_sleeps() {
    let harness = Harness::new(
        "deepl",
        vec![MockStep::reply(429, "limited").header("retry-after", "120"); 30],
    );
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "deepl.application", 5).kind,
        ErrorKind::HttpStatus(429)
    ));
    assert_eq!(harness.requests().len(), 30);
    let sleeps = harness.clock.seconds();
    assert_eq!(sleeps.len(), 29);
    for (actual, expected) in sleeps[..5].iter().zip([1.0, 1.6, 2.56, 4.096, 6.5536]) {
        assert!((actual - expected).abs() < 1e-7);
    }
    assert_eq!(
        [sleeps[5], sleeps[11], sleeps[17], sleeps[23]],
        [1.0, 2.0, 4.0, 8.0]
    );
    assert_eq!(harness.runtime.metrics().snapshot().retry_sleeps, 29);
    // SDK timeout is max(deadline-now, 10), not a fixed ten seconds. With
    // zero HTTP elapsed and centered jitter, the sixth attempt's remaining
    // backoff is 1.6^5 = 10.48576 seconds. Each outer attempt resets it.
    for attempt in harness.requests().as_chunks::<6>().0 {
        for (request, expected) in attempt.iter().zip([10.0, 10.0, 10.0, 10.0, 10.0, 10.48576]) {
            let actual = request.timeout.expect("SDK timeout is set").as_secs_f64();
            assert!(
                (actual - expected).abs() < 1e-7,
                "actual={actual}, expected={expected}"
            );
        }
    }
    harness.drained();
}

#[tokio::test]
async fn deepl_jitter_deadline_consumes_http_time_and_sdk_resets_per_outer() {
    let harness = Harness::new(
        "deepl",
        vec![
            MockStep::reply(503, "down").elapsed(0.75),
            MockStep::error(ErrorKind::ReadTimeout).elapsed(2.0),
            MockStep::reply(200, DEEPL_OK),
        ],
    );
    harness.random.0.lock().unwrap().extend([0.0, 0.999999]);
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert_eq!(harness.clock.seconds(), vec![0.25, 0.0]);
    harness.drained();

    let harness = Harness::new("deepl", vec![MockStep::error(ErrorKind::Connect); 30]);
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "deepl.application", 5).kind,
        ErrorKind::Connect
    ));
    assert_eq!(harness.requests().len(), 30);
    harness.drained();
}

#[tokio::test]
async fn deepl_invalid_mapping_and_malformed_count_are_outer_errors_only() {
    let mut harness = Harness::new("deepl", Vec::new());
    harness.config.lang_out = "unsupported-fixture".into();
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "deepl.application", 5).kind,
        ErrorKind::Value
    ));
    assert!(harness.requests().is_empty());
    assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
    let harness = Harness::new(
        "deepl",
        vec![MockStep::reply(200, r#"{"translations":[{"text":"text"}]}"#); 5],
    );
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "deepl.application", 5).kind,
        ErrorKind::MalformedResponse
    ));
    assert_eq!(harness.requests().len(), 5);
    harness.drained();
}

#[tokio::test]
async fn deepl_actual_truncated_body_is_not_an_inner_connection_retry() {
    let mut step = WireStep::reply(200, "{\"translations\":");
    step.truncate = true;
    let server = Loopback::start(vec![step; 5]).await;
    let harness = Harness::loopback("deepl", &server.endpoint);
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "deepl.application", 5).kind,
        ErrorKind::ReadTransport
    ));
    assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
    server.finish().await;
}

#[tokio::test]
async fn deepl_actual_redirect_loop_is_not_an_inner_connection_retry() {
    // requests follows 30 redirects, then rejects the next redirect response,
    // repeated once per application attempt, with no six-attempt SDK loop.
    let server = Loopback::start(vec![
        WireStep::reply(302, "").header("Location", "/loop");
        31 * 5
    ])
    .await;
    let harness = Harness::loopback("deepl", &server.endpoint);
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "deepl.application", 5).kind,
        ErrorKind::Redirect
    ));
    assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
    server.finish().await;
}

#[tokio::test]
async fn legacy_service_requests_redirect_limit_allows_thirty_hops() {
    let mut steps = vec![WireStep::reply(307, "").header("Location", "/m"); 30];
    steps.push(WireStep::reply(200, "<div class=\"t0\">譯文</div>"));
    let server = Loopback::start(steps).await;
    let harness = Harness::loopback("google", &format!("{}/m", server.endpoint));
    assert_eq!(harness.run_service().await.unwrap().text, "譯文");
    assert!(harness.clock.seconds().is_empty());
    assert_eq!(harness.runtime.metrics().snapshot().admissions, 1);
    assert_eq!(server.finish().await.len(), 31);
}

#[tokio::test]
async fn deepl_jitter_uses_injected_extremes_without_outer_jitter() {
    for unit in [0.0, 0.5, 0.999999] {
        let harness = Harness::new(
            "deepl",
            vec![
                MockStep::reply(503, "down"),
                MockStep::reply(503, "down"),
                MockStep::reply(503, "down"),
                MockStep::reply(200, DEEPL_OK),
            ],
        );
        harness.random.0.lock().unwrap().extend([unit, unit, unit]);
        assert_eq!(harness.run().await.unwrap().text, "譯文");
        let multiplier = 1.0 + 0.23 * (2.0 * unit - 1.0);
        let sleeps = harness.clock.seconds();
        assert_eq!(sleeps.len(), 3);
        for (actual, expected) in sleeps
            .iter()
            .zip([1.0, 1.6 * multiplier, 2.56 * multiplier])
        {
            assert!(
                (actual - expected).abs() < 1e-7,
                "actual={actual}, expected={expected}"
            );
        }
        harness.drained();
    }
}

#[tokio::test]
async fn azure_real_http_wire_contract_and_post_429_without_header() {
    let server = Loopback::start(vec![
        WireStep::reply(429, "limited"),
        WireStep::reply(200, AZURE_OK),
    ])
    .await;
    let mut harness = Harness::loopback("azure", &server.endpoint);
    harness.config.lang_out = "zh".into();
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert_eq!(harness.clock.seconds(), vec![1.0]);
    let requests = server.finish().await;
    assert!(
        requests[0]
            .line
            .starts_with("POST /translate?api-version=3.0&from=en&to=zh-Hans ")
    );
    assert_eq!(
        requests[0].headers["ocp-apim-subscription-key"],
        "fixture-key"
    );
    assert_eq!(
        requests[0].headers["ocp-apim-subscription-region"],
        "chinaeast2"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[0].body).unwrap(),
        json!(["source text"])
    );
}

#[tokio::test]
async fn azure_sdk_status_budget_and_retry_after_have_distinct_counters() {
    let harness = Harness::new("azure", vec![MockStep::reply(503, "down"); 20]);
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "azure.application", 5).kind,
        ErrorKind::HttpStatus(503)
    ));
    assert_eq!(harness.requests().len(), 20);
    let mut expected = Vec::new();
    for outer in 0..5 {
        expected.extend([1.6, 3.2]);
        if outer < 4 {
            expected.push([1.0, 2.0, 4.0, 8.0][outer]);
        }
    }
    assert_eq!(harness.clock.seconds(), expected);
    harness.drained();

    for header in ["retry-after", "retry-after-ms", "x-ms-retry-after-ms"] {
        // Only Retry-After enables the POST429 SDK retry; millisecond aliases
        // control sleep after an otherwise retryable POST503.
        let status = if header == "retry-after" { 429 } else { 503 };
        let delay = if header == "retry-after" { "7" } else { "7000" };
        let harness = Harness::new(
            "azure",
            vec![
                MockStep::reply(status, "retry").header(header, delay),
                MockStep::reply(200, AZURE_OK),
            ],
        );
        assert_eq!(harness.run().await.unwrap().text, "譯文");
        assert_eq!(harness.clock.seconds(), vec![7.0]);
        harness.drained();
    }
}

#[tokio::test]
async fn azure_post_network_errors_do_not_consume_sdk_retry_budgets() {
    for kind in [
        ErrorKind::Connect,
        ErrorKind::ConnectTimeout,
        ErrorKind::ReadTimeout,
        ErrorKind::ReadTransport,
    ] {
        let harness = Harness::new("azure", vec![MockStep::error(kind.clone()); 5]);
        let error = harness.run().await.err().unwrap();
        assert_eq!(last_error(&error, "azure.application", 5).kind, kind);
        assert_eq!(harness.requests().len(), 5);
        assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
        assert!(
            harness
                .requests()
                .iter()
                .all(|request| request.timeout == Some(Duration::from_secs(300)))
        );
        harness.drained();
    }
}

#[tokio::test]
async fn azure_retry_after_date_zero_invalid_and_post429_ms_alias() {
    let date = httpdate::fmt_http_date(UNIX_EPOCH + Duration::from_secs(1_700_000_011));
    let harness = Harness::new(
        "azure",
        vec![
            MockStep::reply(429, "limited").header("retry-after", &date),
            MockStep::reply(200, AZURE_OK),
        ],
    );
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert_eq!(harness.clock.seconds(), vec![11.0]);
    harness.drained();

    let harness = Harness::new(
        "azure",
        vec![
            MockStep::reply(429, "limited").header("retry-after", "0"),
            MockStep::reply(200, AZURE_OK),
        ],
    );
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert!(harness.clock.seconds().is_empty());
    harness.drained();

    let harness = Harness::new(
        "azure",
        vec![MockStep::reply(429, "limited").header("retry-after", "invalid"); 5],
    );
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "azure.application", 5).kind,
        ErrorKind::Value
    ));
    assert_eq!(harness.requests().len(), 5);
    assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
    harness.drained();

    // Millisecond aliases affect delay but do not enable POST429 retries.
    let harness = Harness::new(
        "azure",
        vec![
            MockStep::reply(429, "limited").header("retry-after-ms", "7000"),
            MockStep::reply(200, AZURE_OK),
        ],
    );
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert_eq!(harness.clock.seconds(), vec![1.0]);
    harness.drained();
}

#[tokio::test]
async fn azure_service_post_301_302_does_not_follow_redirect() {
    for status in [301, 302] {
        let server = Loopback::start(vec![
            WireStep::reply(status, "")
                .header("Location", "/must-not-follow");
            5
        ])
        .await;
        let harness = Harness::loopback("azure", &server.endpoint);
        let error = harness.run_service().await.err().unwrap();
        assert_eq!(
            last_error(&error, "azure.application", 5).kind,
            ErrorKind::HttpStatus(status)
        );
        assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
        let requests = server.finish().await;
        assert!(
            requests
                .iter()
                .all(|request| request.line.starts_with("POST /translate?"))
        );
    }
}

#[tokio::test]
async fn azure_redirect_roots_relative_location_and_resets_each_targets_sdk_budget() {
    let server = Loopback::start(vec![
        WireStep::reply(503, "down"),
        WireStep::reply(503, "down"),
        WireStep::reply(307, "").header("Location", "child"),
        WireStep::reply(503, "down"),
        WireStep::reply(503, "down"),
        WireStep::reply(503, "down"),
        WireStep::reply(200, AZURE_OK),
    ])
    .await;
    let harness = Harness::loopback("azure", &format!("{}/prefix", server.endpoint));
    assert_eq!(harness.run_service().await.unwrap().text, "譯文");
    assert_eq!(harness.clock.seconds(), vec![1.6, 1.6, 3.2]);
    assert_eq!(harness.runtime.metrics().snapshot().admissions, 1);
    let requests = server.finish().await;
    assert!(
        requests[..3]
            .iter()
            .all(|request| request.line.starts_with("POST /prefix/translate?"))
    );
    assert!(
        requests[3..]
            .iter()
            .all(|request| request.line == "POST /child HTTP/1.1")
    );
    assert!(
        requests
            .iter()
            .all(|request| request.headers["ocp-apim-subscription-key"] == "fixture-key")
    );
    assert!(requests.iter().all(
        |request| serde_json::from_slice::<Value>(&request.body).unwrap() == json!(["source text"])
    ));
}

#[tokio::test]
async fn azure_303_preserves_body_while_get_uses_its_own_status_selector() {
    let server = Loopback::start(vec![
        WireStep::reply(303, "").header("Location", "/redirected"),
        WireStep::reply(429, "limited"),
        WireStep::reply(200, AZURE_OK),
    ])
    .await;
    let harness = Harness::loopback("azure", &server.endpoint);
    assert_eq!(harness.run_service().await.unwrap().text, "譯文");
    let requests = server.finish().await;
    assert!(requests[0].line.starts_with("POST /translate?"));
    assert!(
        requests[1..]
            .iter()
            .all(|request| request.line == "GET /redirected HTTP/1.1")
    );
    assert!(requests.iter().all(
        |request| serde_json::from_slice::<Value>(&request.body).unwrap() == json!(["source text"])
    ));
    assert!(harness.clock.seconds().is_empty());
    for status in [408, 502, 506, 998] {
        let harness = Harness::new(
            "azure",
            vec![
                MockStep::reply(303, "").header("location", "/redirected"),
                MockStep::reply(status, "retry"),
                MockStep::reply(200, AZURE_OK),
            ],
        );
        assert_eq!(harness.run().await.unwrap().text, "譯文");
        assert_eq!(harness.requests().len(), 3);
        assert_eq!(harness.requests()[1].method, reqwest::Method::GET);
        assert!(harness.clock.seconds().is_empty());
        harness.drained();
    }
}

#[tokio::test]
async fn azure_deadline_limits_connect_only_and_tracks_last_error_kind() {
    let harness = Harness::new(
        "azure",
        vec![
            MockStep::reply(303, "").header("location", "/redirected"),
            MockStep::error(ErrorKind::Connect).elapsed(604600.0),
            MockStep::reply(200, AZURE_OK),
        ],
    );
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert_eq!(
        harness.requests()[2].timeout,
        Some(Duration::from_secs(300))
    );
    assert_eq!(
        harness
            .script
            .as_ref()
            .unwrap()
            .connect_timeouts
            .lock()
            .unwrap()[2],
        Some(Duration::from_secs(200))
    );
    harness.drained();

    for (injected, expected) in [
        (ErrorKind::Connect, ErrorKind::ConnectTimeout),
        (ErrorKind::ReadTimeout, ErrorKind::ReadTimeout),
    ] {
        let mut steps = Vec::new();
        for _ in 0..5 {
            steps.push(MockStep::reply(303, "").header("location", "/redirected"));
            steps.push(MockStep::error(injected.clone()).elapsed(604800.0));
        }
        let harness = Harness::new("azure", steps);
        let error = match harness.run().await {
            Err(error) => error,
            Ok(_) => panic!("expected Azure operation timeout"),
        };
        match error.kind {
            ErrorKind::RetryExhausted { last, .. } => assert_eq!(last.kind, expected),
            kind => panic!("unexpected error: {kind:?}"),
        }
        harness.drained();
    }
}

#[tokio::test]
async fn azure_303_get_connect_read_and_status_budgets_are_independent() {
    let mut steps = vec![MockStep::reply(303, "").header("location", "/redirected")];
    steps.extend(vec![MockStep::error(ErrorKind::Connect); 3]);
    steps.extend(vec![MockStep::error(ErrorKind::ReadTimeout); 3]);
    steps.extend(vec![MockStep::reply(503, "down"); 3]);
    steps.push(MockStep::reply(200, AZURE_OK));
    let harness = Harness::new("azure", steps);
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    assert_eq!(harness.requests().len(), 11);
    assert!(
        harness.requests()[1..]
            .iter()
            .all(|request| request.method == reqwest::Method::GET
                && request.url.ends_with("/redirected")
                && request.query.is_empty())
    );
    assert_eq!(
        harness.clock.seconds(),
        vec![1.6, 3.2, 6.4, 12.8, 25.6, 51.2, 102.4, 120.0]
    );
    harness.drained();
}

#[tokio::test]
async fn azure_303_get_truncated_bodies_use_status_budget_without_backoff_history() {
    let mut truncated = WireStep::reply(200, "{\"translations\":");
    truncated.truncate = true;
    let server = Loopback::start(vec![
        WireStep::reply(303, "").header("Location", "/body"),
        truncated.clone(),
        truncated.clone(),
        truncated,
        WireStep::reply(200, AZURE_OK),
    ])
    .await;
    let harness = Harness::loopback("azure", &server.endpoint);
    assert_eq!(harness.run_service().await.unwrap().text, "譯文");
    assert!(harness.clock.seconds().is_empty());
    let requests = server.finish().await;
    assert_eq!(requests.len(), 5);
    assert!(
        requests[1..]
            .iter()
            .all(|request| request.line == "GET /body HTTP/1.1")
    );
}

#[tokio::test]
async fn azure_303_get_body_errors_share_status_budget_with_http_errors() {
    let mut steps = Vec::new();
    for _ in 0..5 {
        steps.extend([
            MockStep::reply(303, "").header("location", "/body"),
            MockStep::error(ErrorKind::ReadTransport),
            MockStep::reply(503, "down"),
            MockStep::error(ErrorKind::ReadTransport),
            MockStep::reply(503, "down"),
        ]);
    }
    let harness = Harness::new("azure", steps);
    let error = harness.run().await.err().unwrap();
    assert_eq!(
        last_error(&error, "azure.application", 5).kind,
        ErrorKind::HttpStatus(503)
    );
    assert_eq!(harness.requests().len(), 25);
    for attempt in harness.requests().as_chunks::<5>().0 {
        assert_eq!(attempt[0].method, reqwest::Method::POST);
        assert!(attempt[1..].iter().all(
            |request| request.method == reqwest::Method::GET && request.url.ends_with("/body")
        ));
    }
    assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
    harness.drained();
}

#[tokio::test]
async fn azure_redirect_loop_has_thirty_hops_per_outer_attempt() {
    let server = Loopback::start(vec![
        WireStep::reply(307, "").header("Location", "/loop");
        31 * 5
    ])
    .await;
    let harness = Harness::loopback("azure", &server.endpoint);
    let error = harness.run_service().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "azure.application", 5).kind,
        ErrorKind::Redirect
    ));
    assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
    assert_eq!(server.finish().await.len(), 155);
}

#[tokio::test]
async fn azure_cross_origin_redirect_does_not_forward_configured_credentials() {
    for status in [307, 308] {
        let destination = Loopback::start(vec![WireStep::reply(200, AZURE_OK)]).await;
        let source = Loopback::start(vec![
            WireStep::reply(status, "")
                .header("Location", &format!("{}/target", destination.endpoint)),
        ])
        .await;
        let harness = Harness::loopback("azure", &source.endpoint);
        assert_eq!(harness.run_service().await.unwrap().text, "譯文");
        let original = source.finish().await;
        let redirected = destination.finish().await;
        assert_eq!(
            original[0].headers["ocp-apim-subscription-key"],
            "fixture-key"
        );
        assert!(
            !redirected[0]
                .headers
                .contains_key("ocp-apim-subscription-key")
        );
        assert!(
            !redirected[0]
                .headers
                .contains_key("ocp-apim-subscription-region")
        );
        assert!(!redirected[0].headers.contains_key("authorization"));
        assert_eq!(
            serde_json::from_slice::<Value>(&redirected[0].body).unwrap(),
            json!(["source text"])
        );
    }
}

#[tokio::test]
async fn tencent_real_http_signing_and_response_without_sdk_retry() {
    let server = Loopback::start(vec![WireStep::reply(
        200,
        r#"{"Response":{"TargetText":" 譯文 ","RequestId":"fixture"}}"#,
    )])
    .await;
    let mut harness = Harness::loopback("tencent", &server.endpoint);
    harness.config.options["tencentcloud_secret_id"] = "fixture-id".into();
    harness.config.options["tencentcloud_token"] = "fixture-token".into();
    harness.request.text = "繁😀".into();
    assert_eq!(harness.run().await.unwrap().text, " 譯文 ");
    let requests = server.finish().await;
    let request = &requests[0];
    assert!(request.line.starts_with("POST / "));
    assert_eq!(request.headers["x-tc-action"], "TextTranslate");
    assert_eq!(request.headers["x-tc-region"], "ap-beijing");
    assert_eq!(request.headers["x-tc-version"], "2018-03-21");
    assert_eq!(request.headers["x-tc-token"], "fixture-token");
    assert_eq!(request.headers["x-tc-timestamp"], "1700000000");
    assert!(request.headers["authorization"].starts_with("TC3-HMAC-SHA256 Credential=fixture-id/2023-11-14/tmt/tc3_request, SignedHeaders=content-type;host, Signature="));
    assert_eq!(
        String::from_utf8(request.body.clone()).unwrap(),
        r#"{"SourceText": "\u7e41\ud83d\ude00", "Source": "en", "Target": "zh-TW", "ProjectId": 0}"#
    );
}

#[tokio::test]
async fn tencent_outer_predicate_retries_http_provider_error_and_interrupt() {
    for step in [
        MockStep::reply(500, "down"),
        MockStep::reply(
            200,
            r#"{"Response":{"Error":{"Code":"InvalidParameter","Message":"fixture-key fixture-id source text https://private.invalid/?key=fixture-key"}}}"#,
        ),
        MockStep::error(ErrorKind::Interrupt),
    ] {
        let mut harness = Harness::new("tencent", vec![step; 5]);
        harness.config.options["tencentcloud_secret_id"] = "fixture-id".into();
        let error = harness.run().await.err().unwrap();
        let cause = last_error(&error, "tencent.application", 5);
        assert!(matches!(
            cause.kind,
            ErrorKind::HttpStatus(500) | ErrorKind::Provider | ErrorKind::Interrupt
        ));
        assert!(!error.message.contains("fixture-key"));
        assert!(!error.message.contains("fixture-id"));
        assert!(!error.message.contains("source text"));
        assert!(!error.message.contains("https://"));
        assert_eq!(harness.requests().len(), 5);
        assert_eq!(harness.clock.seconds(), vec![1.0, 2.0, 4.0, 8.0]);
        harness.drained();
    }
}

#[tokio::test]
async fn every_legacy_http_provider_cancels_inflight_without_retry() {
    for provider in [
        "google",
        "bing",
        "anythingllm",
        "dify",
        "deepl",
        "azure",
        "tencent",
    ] {
        let mut harness = Harness::new(provider, vec![MockStep::hang()]);
        harness.config.options["tencentcloud_secret_id"] = "fixture-id".into();
        let harness = Arc::new(harness);
        let task_harness = harness.clone();
        let task = tokio::spawn(async move { task_harness.run().await });
        eventually(|| harness.requests().len() == 1).await;
        harness.cancel.cancel();
        let error = task.await.unwrap().err().unwrap();
        assert_eq!(error.kind, ErrorKind::Cancelled);
        assert_eq!(harness.requests().len(), 1);
        assert!(harness.clock.seconds().is_empty());
        harness.drained();
    }
}

#[tokio::test]
async fn google_real_http_without_timeout_is_cancellable() {
    let mut step = WireStep::reply(200, "");
    step.hang = true;
    let server = Loopback::start(vec![step]).await;
    let harness = Arc::new(Harness::loopback(
        "google",
        &format!("{}/m", server.endpoint),
    ));
    let task_harness = harness.clone();
    let task = tokio::spawn(async move { task_harness.run().await });
    eventually(|| server.observed().len() == 1).await;
    harness.cancel.cancel();
    assert_eq!(
        task.await.unwrap().err().unwrap().kind,
        ErrorKind::Cancelled
    );
    assert!(harness.clock.seconds().is_empty());
    server.finish().await;
}

#[tokio::test]
async fn cancel_during_sdk_and_application_backoff_leaves_no_background_retry() {
    for (provider, step, first_delay) in [
        ("google", MockStep::reply(429, "limited"), 1.0),
        ("deepl", MockStep::reply(503, "down"), 1.0),
        (
            "azure",
            MockStep::reply(503, "down").header("retry-after", "9"),
            9.0,
        ),
    ] {
        let harness = Arc::new(Harness::new(provider, vec![step]));
        harness.clock.block_sleep.store(true, Ordering::SeqCst);
        let task_harness = harness.clone();
        let task = tokio::spawn(async move { task_harness.run().await });
        eventually(|| !harness.clock.seconds().is_empty()).await;
        harness.cancel.cancel();
        assert_eq!(
            task.await.unwrap().err().unwrap().kind,
            ErrorKind::Cancelled
        );
        assert_eq!(harness.clock.seconds(), vec![first_delay]);
        assert_eq!(harness.requests().len(), 1);
        harness.drained();
    }
}

// A native fixture is necessary for Claude's fixed -p/--model argv. It only
// uses std and is compiled once with the already-installed rustc into a test
// temporary directory. No shell, Python application imports or real CLI run.
const FAKE_EXECUTABLE_SOURCE: &str =
    include_str!("../src/providers/legacy/fixtures/offline_cli.rs");

struct CompiledFixture {
    _directory: tempfile::TempDir,
    executable: std::path::PathBuf,
}
fn compiled_fixture() -> &'static CompiledFixture {
    static FIXTURE: std::sync::OnceLock<CompiledFixture> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("fake_cli.rs");
        let executable = directory.path().join(if cfg!(windows) {
            "fixture.exe"
        } else {
            "fixture"
        });
        std::fs::write(&source, FAKE_EXECUTABLE_SOURCE).unwrap();
        let mut compiler = std::process::Command::new("rustc");
        compiler.args(["--edition=2024", "--crate-name", "offline_cli_fixture"]);
        // Direct rustc does not read .cargo/config.toml. Match the workspace's
        // self-contained linker instead of the machine's older MinGW linker.
        #[cfg(all(windows, target_env = "gnu"))]
        compiler.args([
            "-C",
            "linker=rust-lld",
            "-C",
            "link-self-contained=yes",
            "-C",
            "linker-flavor=ld.lld",
        ]);
        let result = compiler
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("installed rustc");
        assert!(
            result.status.success(),
            "native fixture compilation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        CompiledFixture {
            _directory: directory,
            executable,
        }
    })
}
struct NativeFixture {
    directory: tempfile::TempDir,
    executable: std::path::PathBuf,
}
impl NativeFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory
            .path()
            .join(if cfg!(windows) { "fake.exe" } else { "fake" });
        std::fs::copy(&compiled_fixture().executable, &executable).unwrap();
        Self {
            directory,
            executable,
        }
    }
    fn command(&self, mode: &str) -> String {
        // Python shlex treats backslash as an escape even on Windows.
        format!(
            "'{}' {mode}",
            self.executable.to_string_lossy().replace('\'', "'\"'\"'")
        )
    }
    fn calls(&self) -> Vec<(u32, String)> {
        std::fs::read_to_string(self.directory.path().join("calls.log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (pid, mode) = line.split_once(' ')?;
                Some((pid.parse().ok()?, mode.to_owned()))
            })
            .collect()
    }
    fn input(&self, pid: u32) -> Vec<u8> {
        std::fs::read(self.directory.path().join(format!("input-{pid}.bin"))).unwrap()
    }
    fn pid(&self, name: &str) -> Option<u32> {
        std::fs::read_to_string(self.directory.path().join(name))
            .ok()
            .and_then(|text| text.parse().ok())
    }
    fn cli(&self, mode: &str) -> Harness {
        let mut harness = Harness::new("clitranslator", Vec::new());
        harness.config.options["clitranslator_command"] = self.command(mode).into();
        harness
    }
    fn claude(&self, mode: &str) -> Harness {
        let mut harness = Harness::new("claudecode", Vec::new());
        harness.config.options["claude_code_path"] =
            self.executable.to_string_lossy().to_string().into();
        harness.config.options["claude_code_model"] = mode.into();
        harness
    }
}

#[tokio::test]
async fn cli_real_process_stdin_postprocess_and_failure_restarts_translation() {
    let fixture = NativeFixture::new();
    let mut harness = fixture.cli("echo");
    harness.request.text = "  hello\nworld  ".into();
    harness.config.options["clitranslator_postprocess_command"] = fixture.command("post").into();
    legacy::preflight(&harness.context()).await.unwrap();
    assert_eq!(harness.run().await.unwrap().text, "HELLO\nWORLD");
    let calls = fixture.calls();
    assert_eq!(
        calls
            .iter()
            .map(|(_, mode)| mode.as_str())
            .collect::<Vec<_>>(),
        vec!["echo", "post"]
    );
    #[cfg(windows)]
    assert_eq!(fixture.input(calls[0].0), b"  hello\r\nworld  ");
    #[cfg(not(windows))]
    assert_eq!(fixture.input(calls[0].0), b"  hello\nworld  ");
    assert!(harness.clock.seconds().is_empty());

    let fixture = NativeFixture::new();
    let mut harness = fixture.cli("echo");
    harness.config.options["clitranslator_postprocess_command"] = fixture.command("fail").into();
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "clitranslator.application", 3).kind,
        ErrorKind::ProcessExit
    ));
    assert_eq!(
        fixture
            .calls()
            .iter()
            .map(|(_, mode)| mode.as_str())
            .collect::<Vec<_>>(),
        vec!["echo", "fail", "echo", "fail", "echo", "fail"]
    );
    assert_eq!(harness.clock.seconds(), vec![2.0, 4.0]);
    assert!(!error.message.contains("fixture-key"));
    assert!(!error.message.contains("source text"));
    assert!(!error.message.contains("https://"));
}

#[tokio::test]
async fn cli_real_process_nonzero_recovers_and_empty_success_remains_success() {
    let fixture = NativeFixture::new();
    let harness = fixture.cli("flaky");
    assert_eq!(harness.run().await.unwrap().text, "source text");
    assert_eq!(fixture.calls().len(), 2);
    assert_eq!(harness.clock.seconds(), vec![2.0]);
    let fixture = NativeFixture::new();
    let mut harness = fixture.cli("echo");
    harness.request.text.clear();
    assert_eq!(harness.run().await.unwrap().text, "");
    assert_eq!(fixture.calls().len(), 1);
}

#[tokio::test]
async fn cli_real_process_timeout_retries_three_times_and_reaps() {
    let fixture = NativeFixture::new();
    let mut harness = fixture.cli("sleep");
    harness.config.options["clitranslator_timeout"] = 1.into();
    let harness = Arc::new(harness);
    let task_harness = harness.clone();
    let task = tokio::spawn(async move { task_harness.run().await });
    #[cfg(windows)]
    let mut observed = Vec::new();
    #[cfg(windows)]
    for attempt in 1..=3 {
        eventually(|| fixture.calls().len() >= attempt).await;
        let child = windows_process::ObservedProcess::from_pid(fixture.calls()[attempt - 1].0);
        assert!(child.running());
        observed.push(child);
    }
    let error = task.await.unwrap().err().unwrap();
    assert!(matches!(
        last_error(&error, "clitranslator.application", 3).kind,
        ErrorKind::ProcessTimeout
    ));
    assert_eq!(fixture.calls().len(), 3);
    assert_eq!(harness.clock.seconds(), vec![2.0, 4.0]);
    #[cfg(windows)]
    for child in observed {
        assert!(!child.running(), "timed out child remains alive");
    }
}

#[tokio::test]
async fn claude_real_process_stream_json_contract_and_startup_is_cached() {
    let fixture = NativeFixture::new();
    let harness = fixture.claude("claude-success");
    let service = pdf2zh_core::TranslationService::new_with_runtime(
        harness.config.clone(),
        harness.runtime.clone(),
    )
    .unwrap();
    service.prepare(&harness.cancel).await.unwrap();
    service.prepare(&harness.cancel).await.unwrap();
    assert_eq!(harness.run().await.unwrap().text, "譯文");
    let calls = fixture.calls();
    assert_eq!(
        calls
            .iter()
            .map(|(_, mode)| mode.as_str())
            .collect::<Vec<_>>(),
        vec!["version", "claude-success"]
    );
    let input: Value = serde_json::from_slice(&fixture.input(calls[1].0)).unwrap();
    assert_eq!(input["type"], "user");
    assert_eq!(input["message"]["role"], "user");
    assert!(
        input["message"]["content"]
            .as_str()
            .unwrap()
            .ends_with("source text")
    );
    let args = std::fs::read_to_string(
        fixture
            .directory
            .path()
            .join(format!("args-{}.txt", calls[1].0)),
    )
    .unwrap();
    assert!(args.contains("-p\n--model\nclaude-success\n--max-turns\n1"));
    assert!(args.contains(
        "--input-format\nstream-json\n--output-format\nstream-json\n--verbose\n--disallowedTools"
    ));
    assert!(args.contains("Task Bash Glob Grep LS exit_plan_mode Read Edit MultiEdit Write NotebookRead NotebookEdit TodoRead TodoWrite"));
}

#[tokio::test]
async fn claude_real_process_nonzero_retries_but_missing_translation_does_not() {
    let fixture = NativeFixture::new();
    let harness = fixture.claude("claude-fail");
    let error = harness.run().await.err().unwrap();
    assert!(matches!(
        last_error(&error, "claudecode.application", 3).kind,
        ErrorKind::ProcessExit
    ));
    assert_eq!(fixture.calls().len(), 3);
    assert_eq!(harness.clock.seconds(), vec![2.0, 4.0]);
    let fixture = NativeFixture::new();
    let harness = fixture.claude("claude-empty");
    assert!(matches!(
        harness.run().await.err().unwrap().kind,
        ErrorKind::Value
    ));
    assert_eq!(fixture.calls().len(), 1);
    assert!(harness.clock.seconds().is_empty());
}

#[tokio::test]
async fn claude_real_process_timeout_converts_to_value_without_retry_and_reaps() {
    let fixture = NativeFixture::new();
    let harness = Arc::new(fixture.claude("claude-sleep"));
    let task_harness = harness.clone();
    let task = tokio::spawn(async move { task_harness.run().await });
    eventually(|| fixture.pid("child.pid").is_some()).await;
    #[cfg(windows)]
    let child = windows_process::ObservedProcess::from_pid(fixture.pid("child.pid").unwrap());
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(121)).await;
    tokio::time::resume();
    let error = task.await.unwrap().err().unwrap();
    assert!(matches!(error.kind, ErrorKind::Value));
    assert!(error.message.contains("timed out"));
    assert_eq!(fixture.calls().len(), 1);
    assert!(harness.clock.seconds().is_empty());
    #[cfg(windows)]
    assert!(!child.running());
}

#[tokio::test]
async fn cli_and_claude_real_process_cancel_reaps_before_return() {
    for provider in ["clitranslator", "claudecode"] {
        let fixture = NativeFixture::new();
        let harness = Arc::new(if provider == "clitranslator" {
            fixture.cli("sleep")
        } else {
            fixture.claude("claude-sleep")
        });
        let task_harness = harness.clone();
        let task = tokio::spawn(async move { task_harness.run().await });
        eventually(|| fixture.pid("child.pid").is_some()).await;
        #[cfg(windows)]
        let child = windows_process::ObservedProcess::from_pid(fixture.pid("child.pid").unwrap());
        harness.cancel.cancel();
        assert_eq!(
            task.await.unwrap().err().unwrap().kind,
            ErrorKind::Cancelled
        );
        assert_eq!(fixture.calls().len(), 1);
        assert!(harness.clock.seconds().is_empty());
        #[cfg(windows)]
        assert!(!child.running());
    }
}

#[cfg(windows)]
mod windows_process {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    pub struct ObservedProcess(HANDLE);
    impl ObservedProcess {
        pub fn from_pid(pid: u32) -> Self {
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            assert!(
                !handle.is_null(),
                "cannot open controlled fixture process: {}",
                std::io::Error::last_os_error()
            );
            Self(handle)
        }
        pub fn running(&self) -> bool {
            let mut code = 0;
            let ok = unsafe { GetExitCodeProcess(self.0, &mut code) };
            assert_ne!(ok, 0, "cannot inspect controlled fixture process");
            code == STILL_ACTIVE as u32
        }
    }
    impl Drop for ObservedProcess {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
#[tokio::test]
async fn windows_job_cancellation_reaps_child_and_descendant_with_inherited_pipes() {
    let fixture = NativeFixture::new();
    let harness = Arc::new(fixture.cli("tree"));
    let task_harness = harness.clone();
    let task = tokio::spawn(async move { task_harness.run().await });
    eventually(|| fixture.pid("child.pid").is_some() && fixture.pid("descendant.pid").is_some())
        .await;
    let child = windows_process::ObservedProcess::from_pid(fixture.pid("child.pid").unwrap());
    let descendant =
        windows_process::ObservedProcess::from_pid(fixture.pid("descendant.pid").unwrap());
    assert!(child.running());
    assert!(descendant.running());
    harness.cancel.cancel();
    assert_eq!(
        task.await.unwrap().err().unwrap().kind,
        ErrorKind::Cancelled
    );
    assert!(!child.running());
    assert!(!descendant.running());
}
