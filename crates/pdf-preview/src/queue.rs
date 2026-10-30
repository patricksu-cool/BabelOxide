use crate::{
    PreviewError, PreviewErrorKind, PreviewFailure, PreviewFrame, PreviewRequest, PreviewResult,
    PreviewTicket,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

pub type Wakeup = Arc<dyn Fn() + Send + Sync + 'static>;

pub(crate) trait RenderBackend {
    fn render(
        &mut self,
        request: &PreviewRequest,
        ticket: PreviewTicket,
    ) -> Result<PreviewFrame, PreviewError>;
    fn close(&mut self) -> Result<(), PreviewError> {
        Ok(())
    }
}

struct Queued {
    request: PreviewRequest,
    ticket: PreviewTicket,
}
#[derive(Default)]
struct Pending {
    request: Option<Queued>,
    sequence: u64,
}
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
    result: Mutex<Option<PreviewResult>>,
    closing: AtomicBool,
    latest: AtomicU64,
    wakeup: Option<Wakeup>,
}

/// At most one native render, one pending request, and one pending result.
/// No PDFium handle or borrowed bitmap is exposed to callers.
pub struct PreviewRenderer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<Result<(), PreviewError>>>,
}

impl std::fmt::Debug for PreviewRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewRenderer")
            .field("closed", &self.shared.closing.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl PreviewRenderer {
    pub fn new(app_root: PathBuf) -> Result<Self, PreviewError> {
        Self::new_inner(app_root, None)
    }
    pub fn new_with_wakeup(app_root: PathBuf, wakeup: Wakeup) -> Result<Self, PreviewError> {
        Self::new_inner(app_root, Some(wakeup))
    }
    fn new_inner(app_root: PathBuf, wakeup: Option<Wakeup>) -> Result<Self, PreviewError> {
        if !app_root.is_absolute() {
            return Err(PreviewError::new(PreviewErrorKind::InvalidPath));
        }
        crate::platform::configure_search_policy()?;
        Self::spawn(move || crate::native::create(app_root), wakeup)
    }
    pub(crate) fn spawn(
        factory: impl FnOnce() -> Result<Box<dyn RenderBackend>, PreviewError> + Send + 'static,
        wakeup: Option<Wakeup>,
    ) -> Result<Self, PreviewError> {
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending::default()),
            wake: Condvar::new(),
            result: Mutex::new(None),
            closing: AtomicBool::new(false),
            latest: AtomicU64::new(0),
            wakeup,
        });
        let worker = shared.clone();
        let handle = thread::Builder::new()
            .name("pdf2zh-pdf-preview".into())
            .spawn(move || {
                // This only handles Rust panics. It does not isolate native crashes
                // or interrupt an in-progress synchronous PDFium call.
                let result = catch_unwind(AssertUnwindSafe(|| run(factory, &worker)))
                    .unwrap_or_else(|_| Err(PreviewError::new(PreviewErrorKind::ThreadFailed)));
                if let Err(error) = result {
                    worker.fail(error);
                }
                result
            })
            .map_err(|_| PreviewError::new(PreviewErrorKind::ThreadFailed))?;
        Ok(Self {
            shared,
            thread: Some(handle),
        })
    }

    /// Never waits for native rendering or a slot to become available. Busy
    /// means retry the newest UI request next frame; no ticket was consumed.
    pub fn request(&self, request: PreviewRequest) -> Result<PreviewTicket, PreviewError> {
        request.validate()?;
        if self.shared.closing.load(Ordering::Acquire) {
            return Err(PreviewError::new(PreviewErrorKind::Closed));
        }
        let mut pending = match self.shared.pending.try_lock() {
            Ok(pending) => pending,
            Err(TryLockError::WouldBlock) => return Err(PreviewError::new(PreviewErrorKind::Busy)),
            Err(TryLockError::Poisoned(_)) => {
                return Err(PreviewError::new(PreviewErrorKind::ThreadFailed));
            }
        };
        if self.shared.closing.load(Ordering::Acquire) {
            return Err(PreviewError::new(PreviewErrorKind::Closed));
        }
        let sequence = pending
            .sequence
            .checked_add(1)
            .ok_or_else(|| PreviewError::new(PreviewErrorKind::TicketExhausted))?;
        let ticket = PreviewTicket(sequence);
        pending.sequence = sequence;
        self.shared.latest.store(sequence, Ordering::Release);
        pending.request = Some(Queued { request, ticket });
        if let Ok(mut result) = self.shared.result.try_lock() {
            *result = None;
        }
        drop(pending);
        self.shared.wake.notify_one();
        Ok(ticket)
    }

    /// Nonblocking; stale generations are discarded even if they were queued
    /// just before a newer request. The UI should also compare its own ticket.
    pub fn try_take_result(&self) -> Option<PreviewResult> {
        let result = self.shared.result.try_lock().ok()?.take()?;
        if result
            .ticket()
            .is_some_and(|ticket| ticket.0 != self.shared.latest.load(Ordering::Acquire))
        {
            None
        } else {
            Some(result)
        }
    }

    /// Clears queued work/results and waits for the current native call to
    /// return. There is deliberately no detach/unsafe force-stop fallback.
    pub fn close(&mut self) -> Result<(), PreviewError> {
        self.shared.closing.store(true, Ordering::Release);
        self.shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request = None;
        *self.shared.result.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.shared.wake.notify_all();
        if let Some(handle) = self.thread.take() {
            handle
                .join()
                .map_err(|_| PreviewError::new(PreviewErrorKind::ThreadFailed))??;
        }
        Ok(())
    }
}

impl Drop for PreviewRenderer {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl Shared {
    fn publish(&self, result: PreviewResult) {
        if self.closing.load(Ordering::Acquire) {
            return;
        }
        let mut slot = self.result.lock().unwrap_or_else(|e| e.into_inner());
        if self.closing.load(Ordering::Acquire)
            || result
                .ticket()
                .is_some_and(|ticket| ticket.0 != self.latest.load(Ordering::Acquire))
        {
            return;
        }
        *slot = Some(result);
        drop(slot);
        if let Some(wakeup) = &self.wakeup {
            wakeup();
        }
    }
    fn fail(&self, error: PreviewError) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if self.closing.swap(true, Ordering::AcqRel) {
            return;
        }
        pending.request = None;
        drop(pending);
        // Publish after closing admissions, so a racing request cannot clear
        // the only initialization error and leave the GUI waiting forever.
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(PreviewResult::Error(PreviewFailure {
                ticket: None,
                request: None,
                error,
            }));
        self.wake.notify_all();
        if let Some(wakeup) = &self.wakeup {
            wakeup();
        }
    }
}

fn run(
    factory: impl FnOnce() -> Result<Box<dyn RenderBackend>, PreviewError>,
    shared: &Shared,
) -> Result<(), PreviewError> {
    if shared.closing.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut backend = factory()?;
    loop {
        let queued = {
            let mut pending = shared.pending.lock().unwrap_or_else(|e| e.into_inner());
            while pending.request.is_none() && !shared.closing.load(Ordering::Acquire) {
                pending = shared.wake.wait(pending).unwrap_or_else(|e| e.into_inner());
            }
            if shared.closing.load(Ordering::Acquire) {
                return backend.close();
            }
            pending
                .request
                .take()
                .expect("a notified request is present")
        };
        if queued.ticket.0 != shared.latest.load(Ordering::Acquire) {
            continue;
        }
        let outcome = backend
            .render(&queued.request, queued.ticket)
            .and_then(|frame| {
                frame.validate()?;
                if frame.ticket != queued.ticket
                    || frame.path != queued.request.path
                    || frame.generation != queued.request.generation
                    || frame.document_key != queued.request.document_key
                {
                    return Err(PreviewError::new(PreviewErrorKind::InvalidPixels));
                }
                Ok(frame)
            });
        let result = match outcome {
            Ok(frame) => PreviewResult::Frame(frame),
            Err(error) => PreviewResult::Error(PreviewFailure {
                ticket: Some(queued.ticket),
                request: Some(queued.request),
                error,
            }),
        };
        shared.publish(result);
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
