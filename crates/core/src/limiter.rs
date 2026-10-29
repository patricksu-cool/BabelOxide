use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::{ErrorKind, ProviderError, ProviderResult, RetryRuntime};

struct State {
    next: Instant,
    interval: Duration,
}

/// Admission spacing only. A cancelled waiter does not reserve a future slot.
pub struct RateLimiter {
    state: Mutex<State>,
}

impl RateLimiter {
    pub fn new(qps: u32) -> Self {
        Self::with_now(qps, Instant::now())
    }
    pub(crate) fn with_now(qps: u32, now: Instant) -> Self {
        assert!(qps > 0, "qps must be positive");
        Self {
            state: Mutex::new(State {
                next: now,
                interval: Duration::from_secs_f64(1.0 / f64::from(qps)),
            }),
        }
    }
    pub async fn set_max_qps(&self, qps: u32) -> ProviderResult<()> {
        if qps == 0 {
            return Err(ProviderError::new(
                ErrorKind::InvalidConfiguration,
                "qps must be positive",
            ));
        }
        self.state.lock().await.interval = Duration::from_secs_f64(1.0 / f64::from(qps));
        Ok(())
    }
    pub async fn wait(&self, cancel: &CancellationToken) -> ProviderResult<()> {
        self.wait_with_runtime(&RetryRuntime::default(), cancel)
            .await
    }
    pub(crate) async fn wait_with_runtime(
        &self,
        runtime: &RetryRuntime,
        cancel: &CancellationToken,
    ) -> ProviderResult<()> {
        let mut state = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ProviderError::cancelled()),
            state = self.state.lock() => state,
        };
        let now = runtime.now();
        if state.next > now {
            runtime.sleep(state.next - now, cancel).await?;
        }
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        state.next = state.next.max(runtime.now()) + state.interval;
        Ok(())
    }
}
