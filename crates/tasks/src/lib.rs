//! Shared GUI/CLI batch ownership. Dropping a view never aborts a worker future.

mod settings;
pub use settings::task_from_settings;

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result, ensure};
use babeldoc_bridge::{WorkerOptions, translate_pdf_with_services};
use pdf2zh_core::{PdfJob, ProviderError, TranslationService};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Starting,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskFailure {
    pub message: String,
    pub provider: Option<ProviderError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskSnapshot {
    pub id: u64,
    pub status: TaskStatus,
    pub total_files: usize,
    pub completed_files: usize,
    pub current_file: Option<usize>,
    pub progress: Option<Value>,
    pub results: Vec<Value>,
    pub failure: Option<TaskFailure>,
}

pub struct TaskSpec {
    pub worker: WorkerOptions,
    pub jobs: Vec<PdfJob>,
    pub translator: Arc<TranslationService>,
    pub term_translator: Option<Arc<TranslationService>>,
}

struct TaskState {
    cancel: CancellationToken,
    updates: watch::Sender<TaskSnapshot>,
}

impl TaskState {
    fn cancel(&self) {
        self.updates.send_modify(|snapshot| {
            if !snapshot.status.is_terminal() {
                self.cancel.cancel();
                snapshot.status = TaskStatus::Cancelling;
            }
        });
    }

    fn finish(&self, status: TaskStatus, failure: Option<TaskFailure>) {
        self.updates.send_modify(|snapshot| {
            if !snapshot.status.is_terminal() {
                // Cancellation and normal completion linearize under this
                // same watch lock. An accepted cancel cannot turn into success.
                snapshot.status = if self.cancel.is_cancelled() {
                    TaskStatus::Cancelled
                } else {
                    status
                };
                snapshot.current_file = None;
                snapshot.failure = if snapshot.status == TaskStatus::Cancelled {
                    None
                } else {
                    failure
                };
            }
        });
    }

    fn unexpected_exit(&self) {
        self.updates.send_modify(|snapshot| {
            if !snapshot.status.is_terminal() {
                self.cancel.cancel();
                snapshot.status = TaskStatus::Failed;
                snapshot.current_file = None;
                snapshot.failure = Some(TaskFailure {
                    message: "Translation task ended unexpectedly".into(),
                    provider: None,
                });
            }
        });
    }
}

#[derive(Clone)]
pub struct TaskHandle {
    state: Arc<TaskState>,
}

impl TaskHandle {
    pub fn id(&self) -> u64 {
        self.state.updates.borrow().id
    }

    pub fn cancel(&self) {
        self.state.cancel();
    }

    /// One latest snapshot per subscriber, including reliable terminal state.
    pub fn subscribe(&self) -> watch::Receiver<TaskSnapshot> {
        self.state.updates.subscribe()
    }

    pub fn snapshot(&self) -> TaskSnapshot {
        self.state.updates.borrow().clone()
    }

    pub async fn wait(&self) -> TaskSnapshot {
        let mut updates = self.subscribe();
        loop {
            let snapshot = updates.borrow_and_update().clone();
            if snapshot.status.is_terminal() {
                return snapshot;
            }
            if updates.changed().await.is_err() {
                return self.snapshot();
            }
        }
    }
}

struct ManagerState {
    slots: Arc<Semaphore>,
    next_id: AtomicU64,
    closed: AtomicBool,
    tasks: Mutex<BTreeMap<u64, Weak<TaskState>>>,
}

/// Limits PDF batches separately from a provider's text-request concurrency.
/// Completed history is held by callers; the manager keeps only weak entries.
pub struct TaskManager {
    state: Arc<ManagerState>,
}

impl TaskManager {
    pub fn new(max_running_batches: usize) -> Result<Self> {
        ensure!(
            (1..=Semaphore::MAX_PERMITS).contains(&max_running_batches),
            "Batch concurrency must be positive and within the supported limit"
        );
        Ok(Self {
            state: Arc::new(ManagerState {
                slots: Arc::new(Semaphore::new(max_running_batches)),
                next_id: AtomicU64::new(1),
                closed: AtomicBool::new(false),
                tasks: Mutex::new(BTreeMap::new()),
            }),
        })
    }

    pub fn submit(&self, spec: TaskSpec) -> Result<TaskHandle> {
        let runtime = tokio::runtime::Handle::try_current()
            .context("Task submission needs a Tokio runtime")?;
        ensure!(!spec.jobs.is_empty(), "A batch needs at least one PDF");
        let mut tasks = self
            .state
            .tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Task manager lock failed"))?;
        // The same lock protects submission against shutdown's final inventory.
        ensure!(
            !self.state.closed.load(Ordering::Acquire),
            "Task manager is shutting down"
        );
        tasks.retain(|_, task| task.strong_count() > 0);
        let id = self.state.next_id.fetch_add(1, Ordering::Relaxed);
        let (updates, _) = watch::channel(TaskSnapshot {
            id,
            status: TaskStatus::Queued,
            total_files: spec.jobs.len(),
            completed_files: 0,
            current_file: None,
            progress: None,
            results: Vec::new(),
            failure: None,
        });
        let state = Arc::new(TaskState {
            cancel: CancellationToken::new(),
            updates,
        });
        tasks.insert(id, Arc::downgrade(&state));
        let handle = TaskHandle {
            state: state.clone(),
        };
        let slots = self.state.slots.clone();
        // Construct before spawning: even an unpolled future dropped during
        // runtime shutdown must publish a terminal state for its handle.
        let completion = CompletionGuard(state.clone());
        runtime.spawn(async move {
            // A panic must still publish a terminal state to waiting views.
            let _completion = completion;
            execute_batch(spec, state, slots).await;
        });
        Ok(handle)
    }

    fn live_tasks(&self) -> Vec<TaskHandle> {
        self.state
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .filter_map(Weak::upgrade)
            .map(|state| TaskHandle { state })
            .collect()
    }

    pub fn cancel_all(&self) {
        for task in self.live_tasks() {
            task.cancel();
        }
    }

    pub async fn shutdown(&self) {
        // Submission holds the tasks lock while testing closed. Any task which
        // passed that test is therefore included in this final inventory.
        self.state.closed.store(true, Ordering::Release);
        let tasks = self.live_tasks();
        for task in &tasks {
            task.cancel();
        }
        for task in tasks {
            task.wait().await;
        }
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.state.closed.store(true, Ordering::Release);
        self.cancel_all();
    }
}

struct CompletionGuard(Arc<TaskState>);
impl Drop for CompletionGuard {
    fn drop(&mut self) {
        self.0.unexpected_exit();
    }
}

async fn execute_batch(spec: TaskSpec, state: Arc<TaskState>, slots: Arc<Semaphore>) {
    let _permit = tokio::select! {
        biased;
        _ = state.cancel.cancelled() => {
            state.finish(TaskStatus::Cancelled, None);
            return;
        },
        permit = slots.acquire_owned() => match permit {
            Ok(permit) => permit,
            Err(_) => { state.finish(TaskStatus::Cancelled, None); return; },
        },
    };
    for (index, job) in spec.jobs.into_iter().enumerate() {
        if state.cancel.is_cancelled() {
            state.finish(TaskStatus::Cancelled, None);
            return;
        }
        state.updates.send_modify(|snapshot| {
            if !state.cancel.is_cancelled() {
                snapshot.status = TaskStatus::Starting;
            }
            snapshot.current_file = Some(index);
            snapshot.progress = None;
        });
        let (progress, mut received) = mpsc::channel(32);
        let operation = translate_pdf_with_services(
            &spec.worker,
            job,
            spec.translator.clone(),
            spec.term_translator.clone(),
            state.cancel.clone(),
            Some(progress),
        );
        tokio::pin!(operation);
        let mut progress_closed = false;
        let result = loop {
            tokio::select! {
                result = &mut operation => break result,
                event = received.recv(), if !progress_closed => match event {
                    Some(event) => state.updates.send_modify(|snapshot| {
                        if !snapshot.status.is_terminal() {
                            if !state.cancel.is_cancelled() { snapshot.status = TaskStatus::Running; }
                            snapshot.progress = Some(event);
                        }
                    }),
                    None => progress_closed = true,
                },
            }
        };
        match result {
            Ok(result) => state.updates.send_modify(|snapshot| {
                snapshot.completed_files += 1;
                snapshot.results.push(result);
            }),
            Err(error) => {
                let provider = error.downcast_ref::<ProviderError>().cloned();
                let cancelled = state.cancel.is_cancelled()
                    || provider.as_ref().is_some_and(ProviderError::is_cancelled);
                state.finish(
                    if cancelled {
                        TaskStatus::Cancelled
                    } else {
                        TaskStatus::Failed
                    },
                    (!cancelled).then(|| TaskFailure {
                        message: format!("{error:#}"),
                        provider,
                    }),
                );
                return;
            }
        }
    }
    state.finish(
        if state.cancel.is_cancelled() {
            TaskStatus::Cancelled
        } else {
            TaskStatus::Succeeded
        },
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> TaskState {
        let (updates, _) = watch::channel(TaskSnapshot {
            id: 1,
            status: TaskStatus::Running,
            total_files: 1,
            completed_files: 0,
            current_file: Some(0),
            progress: None,
            results: Vec::new(),
            failure: None,
        });
        TaskState {
            cancel: CancellationToken::new(),
            updates,
        }
    }

    #[test]
    fn accepted_cancel_wins_over_a_precomputed_normal_outcome() {
        for outcome in [TaskStatus::Succeeded, TaskStatus::Failed] {
            let state = state();
            state.cancel();
            state.finish(
                outcome,
                Some(TaskFailure {
                    message: "precomputed failure".into(),
                    provider: None,
                }),
            );
            assert_eq!(state.updates.borrow().status, TaskStatus::Cancelled);
            assert!(state.updates.borrow().failure.is_none());
        }
    }

    #[test]
    fn normal_completion_wins_over_late_cancel_and_guard_drop() {
        let state = state();
        state.finish(TaskStatus::Succeeded, None);
        state.cancel();
        state.unexpected_exit();
        assert_eq!(state.updates.borrow().status, TaskStatus::Succeeded);
        assert!(!state.cancel.is_cancelled());
    }
}
