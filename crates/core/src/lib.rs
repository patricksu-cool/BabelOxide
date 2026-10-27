//! Rust-owned translation requests for the BabelDOC bridge proof of concept.
//!
//! Only OpenAI-compatible chat completions are implemented. Persistent caches,
//! token accounting, provider-specific retries, and other providers are deferred.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Provider configuration remains in Rust and is never sent to the PDF worker.
#[derive(Clone)]
pub struct TranslatorConfig {
    pub provider: String,
    /// OpenAI-compatible base URL, for example `https://api.openai.com/v1`.
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub timeout_seconds: f64,
    pub lang_out: String,
    /// Operational options: `qps`, `max_concurrency`, `enable_json_mode`.
    /// Request options: `temperature`, `reasoning_effort`, `extra_body`.
    pub options: Value,
}

impl fmt::Debug for TranslatorConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TranslatorConfig")
            .field("provider", &self.provider)
            .field("endpoint", &"[REDACTED]")
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("lang_out", &self.lang_out)
            .field("options", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PdfJob {
    pub input_file: PathBuf,
    pub output_dir: PathBuf,
    pub lang_in: String,
    pub lang_out: String,
    pub pages: Option<String>,
    pub qps: u32,
    pub pool_max_workers: u32,
    pub no_auto_extract_glossary: bool,
    pub watermark_output_mode: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Text is deliberately excluded from Debug output.
#[derive(Clone, Deserialize)]
pub struct TranslationRequest {
    pub id: String,
    pub text: String,
    pub method: String,
    pub ignore_cache: bool,
    pub rate_limit_params: Value,
    pub translator: String,
}

struct RateLimiter {
    next_start: Mutex<Instant>,
    interval: Duration,
}

impl RateLimiter {
    fn new(qps: u32) -> Self {
        Self {
            next_start: Mutex::new(Instant::now()),
            interval: Duration::from_secs_f64(1.0 / f64::from(qps)),
        }
    }

    async fn wait(&self, cancel: &CancellationToken) -> Result<()> {
        let start = {
            let mut next = tokio::select! {
                biased;
                _ = cancel.cancelled() => bail!("translation cancelled"),
                guard = self.next_start.lock() => guard,
            };
            let start = (*next).max(Instant::now());
            *next = start + self.interval;
            start
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => bail!("translation cancelled"),
            _ = tokio::time::sleep_until(start) => Ok(()),
        }
    }
}

/// Shared by all PDF-worker callbacks; HTTP, concurrency and QPS stay in Rust.
pub struct TranslationService {
    config: TranslatorConfig,
    endpoint: Url,
    client: Client,
    timeout: Duration,
    concurrency: Semaphore,
    rate_limiter: RateLimiter,
}

impl TranslationService {
    pub fn new(config: TranslatorConfig) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        if !matches!(config.provider.as_str(), "openai" | "openai-compatible") {
            bail!("the Rust proof of concept supports only OpenAI-compatible translation");
        }
        if config.api_key.trim().is_empty() {
            bail!("an OpenAI-compatible API key is required");
        }
        if config.model.trim().is_empty() {
            bail!("an OpenAI-compatible model is required");
        }
        if !config.options.is_null() && !config.options.is_object() {
            bail!("translator options must be an object");
        }
        let timeout = Duration::try_from_secs_f64(config.timeout_seconds)
            .ok()
            .filter(|value| !value.is_zero())
            .ok_or_else(|| anyhow!("translation timeout must be finite and positive"))?;
        let qps = positive_option(&config.options, "qps", 4)?;
        let max_concurrency = positive_option(&config.options, "max_concurrency", qps)?;
        if max_concurrency as usize > Semaphore::MAX_PERMITS {
            bail!("max_concurrency exceeds the supported limit");
        }
        let mut endpoint = Url::parse(&config.endpoint)
            .map_err(|_| anyhow!("invalid OpenAI-compatible base URL"))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            bail!(
                "OpenAI-compatible base URL must be HTTP(S) without credentials, query or fragment"
            );
        }
        let path = endpoint.path().trim_end_matches('/');
        if !path.ends_with("/chat/completions") {
            endpoint.set_path(&format!("{path}/chat/completions"));
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .map_err(|_| anyhow!("could not initialize the translation HTTP client"))?;
        Ok(Self {
            config,
            endpoint,
            client,
            timeout,
            concurrency: Semaphore::new(max_concurrency as usize),
            rate_limiter: RateLimiter::new(qps),
        })
    }

    pub fn name(&self) -> &str {
        "openai"
    }

    pub fn is_llm(&self) -> bool {
        true
    }

    pub fn lang_out(&self) -> &str {
        &self.config.lang_out
    }

    /// No cache or retry is performed in this proof of concept. `ignore_cache`
    /// is carried by the protocol for the later cache implementation.
    pub async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: &CancellationToken,
    ) -> Result<String> {
        let _permit = tokio::select! {
            biased;
            _ = cancel.cancelled() => bail!("translation cancelled"),
            permit = self.concurrency.acquire() => {
                permit.map_err(|_| anyhow!("translation service closed"))?
            }
        };
        self.rate_limiter.wait(cancel).await?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => bail!("translation cancelled"),
            result = tokio::time::timeout(self.timeout, self.execute(request)) => {
                result.map_err(|_| anyhow!("translation request timed out"))?
            }
        }
    }

    async fn execute(&self, request: &TranslationRequest) -> Result<String> {
        let content = match request.method.as_str() {
            "translate" | "do_translate" => legacy_prompt(&request.text, &self.config.lang_out),
            "llm_translate" | "do_llm_translate" => request.text.clone(),
            _ => bail!("unsupported translation request method"),
        };
        let mut body = Map::new();
        body.insert("model".into(), json!(self.config.model));
        body.insert(
            "messages".into(),
            json!([{ "role": "user", "content": content }]),
        );
        for key in ["temperature", "reasoning_effort"] {
            if let Some(value) = self.config.options.get(key) {
                body.insert(key.into(), value.clone());
            }
        }
        if let Some(extra) = self.config.options.get("extra_body") {
            let extra = extra
                .as_object()
                .ok_or_else(|| anyhow!("extra_body must be an object"))?;
            for (key, value) in extra {
                if matches!(
                    key.as_str(),
                    "model" | "messages" | "stream" | "response_format"
                ) {
                    bail!("extra_body cannot replace reserved chat completion fields");
                }
                body.insert(key.clone(), value.clone());
            }
        }
        if self
            .config
            .options
            .get("enable_json_mode")
            .and_then(Value::as_bool)
            == Some(true)
            && request
                .rate_limit_params
                .get("request_json_mode")
                .and_then(Value::as_bool)
                == Some(true)
        {
            body.insert("response_format".into(), json!({ "type": "json_object" }));
        }
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(safe_http_error)?;
        if !response.status().is_success() {
            // Provider bodies and reqwest errors may contain URLs, keys or input.
            bail!(
                "translation provider returned HTTP {}",
                response.status().as_u16()
            );
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            bail!("translation response exceeded the 4 MiB limit");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(safe_http_error)? {
            if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
                bail!("translation response exceeded the 4 MiB limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: ChatCompletion = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("translation provider returned invalid chat completion JSON"))?;
        let content = response
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| anyhow!("translation provider returned no message content"))?;
        let content = content.trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '\u{001c}'..='\u{001f}')
        });
        Ok(remove_think(content).to_owned())
    }
}

fn positive_option(options: &Value, key: &str, default: u32) -> Result<u32> {
    match options.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("{key} must be a positive 32-bit integer")),
    }
}

fn safe_http_error(error: reqwest::Error) -> anyhow::Error {
    if error.is_timeout() {
        anyhow!("translation request timed out")
    } else {
        anyhow!("translation HTTP request failed")
    }
}

fn legacy_prompt(text: &str, lang_out: &str) -> String {
    format!(
        "You are a professional,authentic machine translation engine.\n\n;; Treat next line as plain text input and translate it into {lang_out}, output translation ONLY. If translation is unnecessary (e.g. proper nouns, codes, {{{{1}}}}, etc. ), return the original text. NO explanations. NO notes. Input:\n\n{text}"
    )
}

fn remove_think(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("<think>") {
        // Python's ^<think>.+?</think> requires a nonempty thought.
        let mut offset = 0;
        while let Some(index) = rest[offset..].find("</think>") {
            let index = offset + index;
            if index > 0 {
                return &rest[index + "</think>".len()..];
            }
            offset = "</think>".len();
        }
    }
    content
}

#[derive(Deserialize)]
struct ChatCompletion {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;

    fn config(endpoint: String) -> TranslatorConfig {
        TranslatorConfig {
            provider: "openai".into(),
            endpoint,
            api_key: "synthetic-private-key".into(),
            model: "synthetic-model".into(),
            timeout_seconds: 2.0,
            lang_out: "zh-TW".into(),
            options: json!({ "qps": 1000, "max_concurrency": 2 }),
        }
    }

    fn request(method: &str) -> TranslationRequest {
        TranslationRequest {
            id: "request-1".into(),
            text: "A formula {v0} and <style id='1'>text</style>.".into(),
            method: method.into(),
            ignore_cache: true,
            rate_limit_params: Value::Null,
            translator: "openai".into(),
        }
    }

    async fn read_request(stream: &mut TcpStream) -> (String, Value) {
        let mut data = Vec::new();
        let header_end = loop {
            let mut chunk = [0; 1024];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "client closed before request headers");
            data.extend_from_slice(&chunk[..count]);
            if let Some(index) = data.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8(data[..header_end].to_vec()).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while data.len() < header_end + length {
            let mut chunk = [0; 1024];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "client closed before request body");
            data.extend_from_slice(&chunk[..count]);
        }
        let body = serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
        (headers, body)
    }

    async fn server(status: &str, body: String) -> (String, oneshot::Receiver<(String, Value)>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (send, receive) = oneshot::channel();
        let status = status.to_owned();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = send.send(request);
        });
        (endpoint, receive)
    }

    #[tokio::test]
    async fn translate_posts_legacy_prompt_and_removes_only_the_initial_thought() {
        let (endpoint, received) = server("200 OK", json!({"choices":[{"message":{"content":"  <think>private reasoning</think>\n譯文  "}}]}).to_string()).await;
        let service = TranslationService::new(config(endpoint)).unwrap();
        let request = request("translate");
        assert_eq!(
            service
                .translate(&request, &CancellationToken::new())
                .await
                .unwrap(),
            "\n譯文"
        );
        let (headers, body) = received.await.unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(
            headers
                .to_lowercase()
                .contains("authorization: bearer synthetic-private-key")
        );
        assert_eq!(body["model"], "synthetic-model");
        assert_eq!(
            body["messages"],
            json!([{ "role": "user", "content": format!("You are a professional,authentic machine translation engine.\n\n;; Treat next line as plain text input and translate it into zh-TW, output translation ONLY. If translation is unnecessary (e.g. proper nouns, codes, {{{{1}}}}, etc. ), return the original text. NO explanations. NO notes. Input:\n\n{}", request.text) }])
        );
        assert!(body.get("response_format").is_none());
    }

    #[tokio::test]
    async fn llm_translation_preserves_raw_prompt_and_requests_json_when_enabled() {
        let (endpoint, received) = server(
            "200 OK",
            json!({"choices":[{"message":{"content":"{\"result\":true}"}}]}).to_string(),
        )
        .await;
        let mut config = config(endpoint);
        config.options["enable_json_mode"] = json!(true);
        config.options["temperature"] = json!(0.0);
        config.options["extra_body"] = json!({ "seed": 7 });
        let service = TranslationService::new(config).unwrap();
        let mut request = request("llm_translate");
        request.rate_limit_params = json!({ "request_json_mode": true });
        assert_eq!(
            service
                .translate(&request, &CancellationToken::new())
                .await
                .unwrap(),
            "{\"result\":true}"
        );
        let (_, body) = received.await.unwrap();
        assert_eq!(
            body["messages"],
            json!([{ "role": "user", "content": request.text }])
        );
        assert_eq!(body["response_format"], json!({"type":"json_object"}));
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["seed"], 7);
    }

    #[tokio::test]
    async fn errors_do_not_expose_provider_body_or_credentials() {
        let (endpoint, received) = server(
            "401 Unauthorized",
            "synthetic-private-key private-source-text endpoint-secret".into(),
        )
        .await;
        let service = TranslationService::new(config(endpoint)).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "translation provider returned HTTP 401"
        );
        received.await.unwrap();
    }

    #[tokio::test]
    async fn malformed_empty_and_null_responses_fail_without_echoing_content() {
        for body in [
            "private-source-text".into(),
            json!({"choices":[]}).to_string(),
            json!({"choices":[{"message":{"content":null}}]}).to_string(),
            json!({"choices":[{"message":{}}]}).to_string(),
        ] {
            let (endpoint, received) = server("200 OK", body).await;
            let service = TranslationService::new(config(endpoint)).unwrap();
            let error = service
                .translate(&request("translate"), &CancellationToken::new())
                .await
                .unwrap_err();
            assert!(!format!("{error:#}").contains("private-source-text"));
            received.await.unwrap();
        }
    }

    #[tokio::test]
    async fn oversized_response_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                        MAX_RESPONSE_BYTES + 1
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let service = TranslationService::new(config(endpoint)).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "translation response exceeded the 4 MiB limit"
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn oversized_body_without_content_length_is_also_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            // The client is allowed to close the connection as soon as the cap is reached.
            let _ = stream.write_all(&vec![b'x'; MAX_RESPONSE_BYTES + 1]).await;
        });
        let service = TranslationService::new(config(endpoint)).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "translation response exceeded the 4 MiB limit"
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_interrupts_in_flight_http() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (started, received) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        let service = TranslationService::new(config(endpoint)).unwrap();
        let cancel = CancellationToken::new();
        let request = request("translate");
        let translation = service.translate(&request, &cancel);
        tokio::pin!(translation);
        tokio::select! {
            result = &mut translation => panic!("request completed before cancellation: {}", result.is_ok()),
            _ = received => {},
        }
        cancel.cancel();
        let error = tokio::time::timeout(Duration::from_millis(200), translation)
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.to_string(), "translation cancelled");
        task.abort();
    }

    #[tokio::test]
    async fn request_timeout_is_bounded_and_safe() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            std::future::pending::<()>().await;
        });
        let mut config = config(endpoint);
        config.timeout_seconds = 0.05;
        let service = TranslationService::new(config).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "translation request timed out");
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_waits_in_rust_and_can_be_cancelled() {
        let limiter = RateLimiter::new(2);
        let cancel = CancellationToken::new();
        let start = Instant::now();
        limiter.wait(&cancel).await.unwrap();
        limiter.wait(&cancel).await.unwrap();
        assert_eq!(Instant::now() - start, Duration::from_millis(500));
        cancel.cancel();
        assert_eq!(
            limiter.wait(&cancel).await.unwrap_err().to_string(),
            "translation cancelled"
        );
        assert_eq!(Instant::now() - start, Duration::from_millis(500));
    }

    #[test]
    fn thought_removal_matches_the_legacy_nonempty_anchored_pattern() {
        assert_eq!(
            remove_think("<think></think>answer"),
            "<think></think>answer"
        );
        assert_eq!(remove_think("<think></think>x</think>answer"), "answer");
        assert_eq!(
            remove_think("prefix<think>x</think>answer"),
            "prefix<think>x</think>answer"
        );
        assert_eq!(remove_think("<think>one\ntwo</think>\nanswer"), "\nanswer");
    }
}
