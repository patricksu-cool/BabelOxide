use std::fmt;

use serde::Serialize;

pub type ProviderResult<T> = Result<T, ProviderError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "details", rename_all = "snake_case")]
pub enum ErrorKind {
    Transport,
    Redirect,
    Connect,
    ConnectTimeout,
    ReadTimeout,
    ReadTransport,
    Timeout,
    HttpStatus(u16),
    MalformedResponse,
    InvalidConfiguration,
    ProcessExit,
    ProcessTimeout,
    Value,
    Interrupt,
    Runtime,
    Provider,
    OllamaResponse(u16),
    FreeRateLimit,
    Cancelled,
    RetryExhausted {
        layer: &'static str,
        attempts: u32,
        last: Box<ProviderError>,
    },
}

/// Messages must be safe for logs. Build provider-originated errors with
/// `ProviderContext::error`, which removes credentials, URLs and source text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "translation cancelled")
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self.kind, ErrorKind::Cancelled)
    }

    pub fn is_transport(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Transport
                | ErrorKind::Redirect
                | ErrorKind::Connect
                | ErrorKind::ConnectTimeout
                | ErrorKind::ReadTimeout
                | ErrorKind::ReadTransport
                | ErrorKind::Timeout
        )
    }

    pub fn exhausted(layer: &'static str, attempts: u32, last: Self) -> Self {
        Self {
            message: format!("{layer} retry exhausted after {attempts} attempts: {last}"),
            kind: ErrorKind::RetryExhausted {
                layer,
                attempts,
                last: Box::new(last),
            },
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProviderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::RetryExhausted { last, .. } => Some(last.as_ref()),
            _ => None,
        }
    }
}
