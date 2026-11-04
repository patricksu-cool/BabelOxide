//! One transport operation, with ordinary HTTP redirects but no automatic retries.

use std::{
    collections::BTreeMap,
    error::Error,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use reqwest::{Client, Method, Url};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{ErrorKind, ProviderError, ProviderResult, RetryRuntime};

pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CACHED_CLIENTS: usize = 32;
type TimeoutPair = (Option<Duration>, Option<Duration>);

#[derive(Clone)]
pub enum HttpBody {
    Empty,
    Json(Value),
    Form(Vec<(String, String)>),
    Bytes(Vec<u8>),
}

#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub query: Vec<(String, String)>,
    pub body: HttpBody,
    /// Connect/read idle timeout, not a whole-translation deadline.
    /// `None` preserves providers whose SDK did not set a timeout.
    pub timeout: Option<Duration>,
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub elapsed: Duration,
    pub final_url: String,
    pub set_cookies: Vec<String>,
}

/// Deterministic request executor for characterization tests or embedded hosts.
/// Production HTTP always uses the ordinary single-attempt transport below.
pub trait HttpExecutor: Send + Sync {
    fn send<'a>(
        &'a self,
        request: HttpRequest,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<HttpResponse>> + Send + 'a>>;

    /// An optional connection-only override for SDKs with separate budgets.
    fn send_with_connect_timeout<'a>(
        &'a self,
        request: HttpRequest,
        _connect_timeout: Option<Duration>,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<HttpResponse>> + Send + 'a>> {
        self.send(request, cancel)
    }
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
    pub fn json(&self) -> ProviderResult<Value> {
        serde_json::from_slice(&self.body).map_err(|_| {
            ProviderError::new(
                ErrorKind::MalformedResponse,
                "translation provider returned invalid JSON",
            )
        })
    }
    pub fn text(&self) -> ProviderResult<&str> {
        std::str::from_utf8(&self.body).map_err(|_| {
            ProviderError::new(
                ErrorKind::MalformedResponse,
                "translation provider returned invalid UTF-8",
            )
        })
    }
}

pub struct HttpTransport {
    pub state: tokio::sync::Mutex<BTreeMap<String, Value>>,
    runtime: RetryRuntime,
    connect_timeout: Option<Duration>,
    clients: Mutex<BTreeMap<TimeoutPair, Client>>,
    cookies: Arc<reqwest::cookie::Jar>,
    request_scoped_cookies: bool,
    prepared_initial_request: bool,
    executor: Option<Arc<dyn HttpExecutor>>,
    redirect_limit: usize,
    diagnostics: bool,
}

impl HttpTransport {
    #[cfg(test)]
    pub(crate) fn configured_connect_timeout(&self) -> Option<Duration> {
        self.connect_timeout
    }
    pub fn new(runtime: RetryRuntime, connect_timeout: Option<Duration>) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            state: tokio::sync::Mutex::new(BTreeMap::new()),
            runtime,
            connect_timeout,
            clients: Mutex::new(BTreeMap::new()),
            cookies: Arc::new(reqwest::cookie::Jar::default()),
            request_scoped_cookies: false,
            prepared_initial_request: false,
            executor: None,
            redirect_limit: 20,
            diagnostics: false,
        }
    }

    pub fn with_executor(runtime: RetryRuntime, executor: Arc<dyn HttpExecutor>) -> Self {
        let mut transport = Self::new(runtime, None);
        transport.executor = Some(executor);
        transport
    }
    /// Zero disables following redirects while preserving the received response.
    pub fn with_redirect_limit(mut self, limit: usize) -> Self {
        self.redirect_limit = limit;
        self
    }

    pub fn with_diagnostics(mut self, enabled: bool) -> Self {
        self.diagnostics = enabled;
        self
    }

    /// Match top-level requests calls: keep cookies within one redirect chain,
    /// but create a fresh session for each attempt or logical translation.
    pub fn with_request_scoped_cookies(mut self) -> Self {
        self.request_scoped_cookies = true;
        self
    }

    /// DeepL prepares its initial request without merging the session cookie jar.
    /// Redirects still use that jar, including cookies from previous responses.
    pub fn with_prepared_initial_request(mut self) -> Self {
        self.prepared_initial_request = true;
        self
    }

    fn client(
        &self,
        timeout: Option<Duration>,
        connect: Option<Duration>,
    ) -> ProviderResult<Client> {
        if self.request_scoped_cookies {
            return self.build_client(timeout, connect, Arc::new(reqwest::cookie::Jar::default()));
        }
        let mut clients = self.clients.lock().map_err(|_| {
            ProviderError::new(ErrorKind::Transport, "translation HTTP client lock failed")
        })?;
        if let Some(client) = clients.get(&(timeout, connect)) {
            return Ok(client.clone());
        }
        let client = self.build_client(timeout, connect, self.cookies.clone())?;
        if clients.len() >= MAX_CACHED_CLIENTS
            && let Some(victim_key) = clients.keys().next().copied()
        {
            clients.remove(&victim_key);
        }
        clients.insert((timeout, connect), client.clone());
        Ok(client)
    }

    fn build_client(
        &self,
        timeout: Option<Duration>,
        connect: Option<Duration>,
        cookies: Arc<reqwest::cookie::Jar>,
    ) -> ProviderResult<Client> {
        self.build_client_with_cookies(timeout, connect, Some(cookies))
    }

    fn build_client_with_cookies(
        &self,
        timeout: Option<Duration>,
        connect: Option<Duration>,
        cookies: Option<Arc<reqwest::cookie::Jar>>,
    ) -> ProviderResult<Client> {
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never());
        if let Some(cookies) = cookies {
            builder = builder.cookie_provider(cookies);
        }
        if let Some(timeout) = timeout {
            builder = builder.read_timeout(timeout);
        }
        if let Some(connect) = connect.or(self.connect_timeout).or(timeout) {
            builder = builder.connect_timeout(connect);
        }
        builder.build().map_err(|_| {
            ProviderError::new(
                ErrorKind::Transport,
                "could not initialize the translation HTTP client",
            )
        })
    }

    pub async fn send(
        &self,
        request: HttpRequest,
        cancel: &CancellationToken,
    ) -> ProviderResult<HttpResponse> {
        self.send_with_connect_timeout(request, None, cancel).await
    }

    pub async fn send_with_connect_timeout(
        &self,
        request: HttpRequest,
        connect_timeout: Option<Duration>,
        cancel: &CancellationToken,
    ) -> ProviderResult<HttpResponse> {
        if let Some(executor) = &self.executor {
            return tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(ProviderError::cancelled()),
                result = async {
                    self.runtime.metrics().http();
                    executor.send_with_connect_timeout(request, connect_timeout, cancel).await
                } => result,
            };
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ProviderError::cancelled()),
            result = self.execute(request, connect_timeout) => result,
        }
    }

    async fn execute(
        &self,
        mut request: HttpRequest,
        connect_timeout: Option<Duration>,
    ) -> ProviderResult<HttpResponse> {
        let start = self.runtime.now();
        let mut url = validated_url(&request.url)?;
        if !request.query.is_empty() {
            url.query_pairs_mut().extend_pairs(
                request
                    .query
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            );
        }
        let client = self.client(request.timeout, connect_timeout)?;
        let initial_client = if self.prepared_initial_request {
            Some(self.build_client_with_cookies(request.timeout, connect_timeout, None)?)
        } else {
            None
        };
        for redirects in 0..=self.redirect_limit {
            let active_client = if redirects == 0 {
                initial_client.as_ref().unwrap_or(&client)
            } else {
                &client
            };
            let mut builder = active_client.request(request.method.clone(), url.clone());
            for (key, value) in &request.headers {
                builder = builder.header(key, value);
            }
            builder = match &request.body {
                HttpBody::Empty => builder,
                HttpBody::Json(value) => builder.json(value),
                HttpBody::Form(value) => builder.form(value),
                HttpBody::Bytes(value) => builder.body(value.clone()),
            };
            self.runtime.metrics().http();
            let mut response = builder.send().await.map_err(classify_http_error)?;
            if redirects == 0 && self.prepared_initial_request {
                for cookie in response.headers().get_all(reqwest::header::SET_COOKIE) {
                    if let Ok(cookie) = cookie.to_str() {
                        self.cookies.add_cookie_str(cookie, &url);
                    }
                }
            }
            let status = response.status().as_u16();
            if self.diagnostics {
                // Status only: never log a URL, body, Header or API key.
                eprintln!("BabelOxide: translation HTTP status {status}");
            }
            if self.redirect_limit > 0
                && matches!(status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = response.headers().get(reqwest::header::LOCATION)
            {
                if redirects == self.redirect_limit {
                    return Err(ProviderError::new(
                        ErrorKind::Redirect,
                        "translation HTTP redirect limit exceeded",
                    ));
                }
                let location = location.to_str().map_err(|_| {
                    ProviderError::new(ErrorKind::Redirect, "invalid translation HTTP redirect")
                })?;
                let next = url.join(location).map_err(|_| {
                    ProviderError::new(ErrorKind::Redirect, "invalid translation HTTP redirect")
                })?;
                let next = validated_url(next.as_str())?;
                if url.origin() != next.origin() {
                    // Custom headers can carry credentials under arbitrary
                    // names. Forward only standard nonsecret representation
                    // headers when following a different origin.
                    request.headers.retain(|key, _| {
                        matches!(
                            key.to_ascii_lowercase().as_str(),
                            "accept"
                                | "accept-encoding"
                                | "accept-language"
                                | "content-type"
                                | "content-length"
                                | "user-agent"
                        )
                    });
                }
                if (matches!(status, 301 | 302) && request.method == Method::POST)
                    || (status == 303 && request.method != Method::HEAD)
                {
                    request.method = Method::GET;
                    request.body = HttpBody::Empty;
                    request.headers.retain(|key, _| {
                        !matches!(
                            key.to_ascii_lowercase().as_str(),
                            "content-length" | "content-type" | "transfer-encoding"
                        )
                    });
                }
                url = next;
                continue;
            }
            let headers = response
                .headers()
                .iter()
                .filter_map(|(key, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (key.as_str().to_ascii_lowercase(), value.to_owned()))
                })
                .collect();
            let set_cookies = response
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok().map(str::to_owned))
                .collect();
            if response
                .content_length()
                .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
            {
                return Err(oversized());
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(classify_http_error)? {
                if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
                    return Err(oversized());
                }
                body.extend_from_slice(&chunk);
            }
            return Ok(HttpResponse {
                status,
                headers,
                body,
                elapsed: self.runtime.now().duration_since(start),
                final_url: url.to_string(),
                set_cookies,
            });
        }
        unreachable!("bounded redirect loop returns")
    }
}

fn validated_url(value: &str) -> ProviderResult<Url> {
    let mut url = Url::parse(value).map_err(|_| {
        ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "invalid translation provider URL",
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "translation provider URL must be HTTP(S) without embedded credentials",
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

fn oversized() -> ProviderError {
    ProviderError::new(
        ErrorKind::MalformedResponse,
        "translation response exceeded the 4 MiB limit",
    )
}

fn classify_http_error(error: reqwest::Error) -> ProviderError {
    let kind = if error.is_connect() {
        if error.is_timeout() {
            ErrorKind::ConnectTimeout
        } else {
            ErrorKind::Connect
        }
    } else if error.is_timeout() {
        ErrorKind::ReadTimeout
    } else if error.is_body() || error.is_decode() {
        ErrorKind::ReadTransport
    } else {
        ErrorKind::Transport
    };
    let message = if matches!(kind, ErrorKind::ConnectTimeout | ErrorKind::ReadTimeout) {
        "translation request timed out"
    } else {
        "translation HTTP request failed"
    };
    // Never format reqwest's URL, headers or arbitrary source messages. The
    // typed I/O cause still distinguishes socket permissions, refusal and DNS.
    let mut detail = format!("{message} ({kind:?}");
    let mut source = error.source();
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            detail.push_str(&format!(", {:?}", io.kind()));
            if let Some(code) = io.raw_os_error() {
                detail.push_str(&format!(", OS error {code}"));
            }
            break;
        }
        if cause.is::<rustls::Error>() {
            detail.push_str(", TLS handshake/certificate verification failed");
            break;
        }
        source = cause.source();
    }
    detail.push(')');
    ProviderError::new(kind, detail)
}

#[cfg(test)]
mod client_cache_tests {
    use super::*;

    #[test]
    fn changing_connection_budgets_do_not_grow_client_cache_without_bound() {
        let transport = HttpTransport::new(RetryRuntime::default(), None);
        for seconds in 1..=100 {
            transport
                .client(
                    Some(Duration::from_secs(300)),
                    Some(Duration::from_secs(seconds)),
                )
                .unwrap();
        }
        assert!(transport.clients.lock().unwrap().len() <= MAX_CACHED_CLIENTS);
    }
}
