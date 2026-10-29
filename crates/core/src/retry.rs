use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::{ErrorKind, ProviderError, ProviderResult, metrics::Metrics};

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn wall_time(&self) -> SystemTime;
    fn sleep<'a>(
        &'a self,
        duration: Duration,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<()>> + Send + 'a>>;
}

pub trait RandomSource: Send + Sync {
    fn unit(&self) -> f64;
}

struct TokioClock;
impl Clock for TokioClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn wall_time(&self) -> SystemTime {
        SystemTime::now()
    }
    fn sleep<'a>(
        &'a self,
        duration: Duration,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<()>> + Send + 'a>> {
        Box::pin(async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(ProviderError::cancelled()),
                _ = tokio::time::sleep(duration) => Ok(()),
            }
        })
    }
}

/// This is only backoff jitter, not a credential, nonce or cryptographic source.
struct Jitter(AtomicU64);
impl RandomSource for Jitter {
    fn unit(&self) -> f64 {
        let mut old = self.0.load(Ordering::Relaxed);
        loop {
            let mut next = old;
            next ^= next << 13;
            next ^= next >> 7;
            next ^= next << 17;
            match self
                .0
                .compare_exchange_weak(old, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return (next >> 11) as f64 / ((1_u64 << 53) as f64),
                Err(actual) => old = actual,
            }
        }
    }
}

#[derive(Clone)]
pub struct RetryRuntime {
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    metrics: Arc<Metrics>,
}

impl Default for RetryRuntime {
    fn default() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos() as u64)
            .unwrap_or(1)
            | 1;
        Self::with_sources(Arc::new(TokioClock), Arc::new(Jitter(AtomicU64::new(seed))))
    }
}

impl RetryRuntime {
    pub fn with_sources(clock: Arc<dyn Clock>, random: Arc<dyn RandomSource>) -> Self {
        Self {
            clock,
            random,
            metrics: Arc::new(Metrics::default()),
        }
    }
    pub fn now(&self) -> Instant {
        self.clock.now()
    }
    pub fn wall_time(&self) -> SystemTime {
        self.clock.wall_time()
    }
    pub fn random_unit(&self) -> f64 {
        let value = self.random.unit();
        if value.is_finite() {
            value.clamp(0.0, 1.0 - f64::EPSILON)
        } else {
            0.0
        }
    }
    pub async fn sleep(
        &self,
        duration: Duration,
        cancel: &CancellationToken,
    ) -> ProviderResult<()> {
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ProviderError::cancelled()),
            result = self.clock.sleep(duration, cancel) => result,
        }
    }
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }
    pub fn record_retry_sleep(&self) {
        self.metrics.retry_sleep();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetrySelector {
    AllExceptions,
    Predicate,
    OpenAiRateLimit,
    OllamaResponse,
    Http,
    FreeRateLimit,
    Runtime,
    Process,
}

impl RetrySelector {
    pub fn matches(self, error: &ProviderError) -> bool {
        if error.is_cancelled() {
            return false;
        }
        match self {
            Self::AllExceptions => !matches!(error.kind, ErrorKind::Interrupt),
            Self::Predicate => true,
            Self::OpenAiRateLimit => matches!(error.kind, ErrorKind::HttpStatus(429)),
            Self::OllamaResponse => matches!(error.kind, ErrorKind::OllamaResponse(_)),
            Self::Http => error.is_transport() || matches!(error.kind, ErrorKind::HttpStatus(_)),
            Self::FreeRateLimit => matches!(error.kind, ErrorKind::FreeRateLimit),
            Self::Runtime => matches!(error.kind, ErrorKind::Runtime),
            Self::Process => matches!(
                error.kind,
                ErrorKind::ProcessExit | ErrorKind::ProcessTimeout
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitPolicy {
    E,
    P,
    H,
    F,
}
impl WaitPolicy {
    pub fn delay(self, failed_attempt: u32) -> Duration {
        let exponent = 2_f64.powi(failed_attempt.saturating_sub(1).min(30) as i32);
        let seconds = match self {
            Self::E => exponent.clamp(1.0, 15.0),
            Self::P => (2.0 * exponent).clamp(2.0, 15.0),
            Self::H => exponent.clamp(30.0, 60.0),
            Self::F => exponent.clamp(4.0, 120.0),
        };
        Duration::from_secs_f64(seconds)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub layer: &'static str,
    pub max_attempts: u32,
    pub selector: RetrySelector,
    pub wait: WaitPolicy,
    pub wrap_exhaustion: bool,
}

pub async fn run<T, F, Fut>(
    runtime: &RetryRuntime,
    cancel: &CancellationToken,
    policy: RetryPolicy,
    mut operation: F,
) -> ProviderResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ProviderResult<T>>,
{
    if policy.max_attempts == 0 {
        return Err(ProviderError::new(
            ErrorKind::InvalidConfiguration,
            "retry attempts must be positive",
        ));
    }
    for attempt in 1..=policy.max_attempts {
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        // Process operations cooperatively kill and reap their job tree before
        // returning Cancelled. Dropping them here would skip that completion.
        let result = if policy.selector == RetrySelector::Process {
            operation().await
        } else {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(ProviderError::cancelled()),
                result = operation() => result,
            }
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) if !policy.selector.matches(&error) => return Err(error),
            Err(error) if attempt == policy.max_attempts => {
                return Err(if policy.wrap_exhaustion {
                    ProviderError::exhausted(policy.layer, attempt, error)
                } else {
                    error
                });
            }
            Err(_) => {
                runtime.record_retry_sleep();
                runtime.sleep(policy.wait.delay(attempt), cancel).await?;
            }
        }
    }
    unreachable!("a positive bounded retry loop returns")
}
