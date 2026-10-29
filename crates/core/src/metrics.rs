use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub total_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_hit_prompt_tokens: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MetricsSnapshot {
    pub calls: u64,
    pub cache_hits: u64,
    pub admissions: u64,
    pub http_attempts: u64,
    pub retry_sleeps: u64,
    pub inflight: u64,
    pub peak_inflight: u64,
    #[serde(flatten)]
    pub usage: Usage,
}

#[derive(Default)]
pub struct Metrics {
    calls: AtomicU64,
    cache_hits: AtomicU64,
    admissions: AtomicU64,
    http_attempts: AtomicU64,
    retry_sleeps: AtomicU64,
    inflight: AtomicU64,
    peak_inflight: AtomicU64,
    total_tokens: AtomicU64,
    prompt_tokens: AtomicU64,
    completion_tokens: AtomicU64,
    cache_hit_prompt_tokens: AtomicU64,
}

impl Metrics {
    pub fn snapshot(&self) -> MetricsSnapshot {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        MetricsSnapshot {
            calls: load(&self.calls),
            cache_hits: load(&self.cache_hits),
            admissions: load(&self.admissions),
            http_attempts: load(&self.http_attempts),
            retry_sleeps: load(&self.retry_sleeps),
            inflight: load(&self.inflight),
            peak_inflight: load(&self.peak_inflight),
            usage: Usage {
                total_tokens: load(&self.total_tokens),
                prompt_tokens: load(&self.prompt_tokens),
                completion_tokens: load(&self.completion_tokens),
                cache_hit_prompt_tokens: load(&self.cache_hit_prompt_tokens),
            },
        }
    }

    pub(crate) fn call(&self) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn admit(&self) {
        self.admissions.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn http(&self) {
        self.http_attempts.fetch_add(1, Ordering::Relaxed);
    }
    pub fn retry_sleep(&self) {
        self.retry_sleeps.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_usage(&self, usage: Usage) {
        self.total_tokens
            .fetch_add(usage.total_tokens, Ordering::Relaxed);
        self.prompt_tokens
            .fetch_add(usage.prompt_tokens, Ordering::Relaxed);
        self.completion_tokens
            .fetch_add(usage.completion_tokens, Ordering::Relaxed);
        self.cache_hit_prompt_tokens
            .fetch_add(usage.cache_hit_prompt_tokens, Ordering::Relaxed);
    }

    pub(crate) fn enter(&self) -> InflightGuard<'_> {
        let current = self.inflight.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak_inflight.fetch_max(current, Ordering::Relaxed);
        InflightGuard(self)
    }
}

pub(crate) struct InflightGuard<'a>(&'a Metrics);
impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.0.inflight.fetch_sub(1, Ordering::Relaxed);
    }
}
