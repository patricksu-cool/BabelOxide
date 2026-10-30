use super::*;
use std::{
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};
use tempfile::TempDir;

const LIMIT: Duration = Duration::from_secs(3);

fn submit(renderer: &PreviewRenderer, request: PreviewRequest) -> PreviewTicket {
    let deadline = Instant::now() + LIMIT;
    loop {
        match renderer.request(request.clone()) {
            Ok(ticket) => return ticket,
            Err(error) if error.kind == PreviewErrorKind::Busy => {
                assert!(Instant::now() < deadline, "request remained busy");
                thread::yield_now();
            }
            Err(error) => panic!("request failed: {error}"),
        }
    }
}
fn result(renderer: &PreviewRenderer) -> PreviewResult {
    let deadline = Instant::now() + LIMIT;
    loop {
        if let Some(value) = renderer.try_take_result() {
            return value;
        }
        assert!(Instant::now() < deadline, "renderer produced no result");
        thread::sleep(Duration::from_millis(1));
    }
}
fn request(directory: &TempDir, page: u32) -> PreviewRequest {
    let mut request = PreviewRequest::new(directory.path().join("synthetic.pdf"), page);
    request.document_key = "document-one".into();
    request.generation = 1;
    request
}
fn frame(request: &PreviewRequest, ticket: PreviewTicket) -> PreviewFrame {
    PreviewFrame {
        ticket,
        document_key: request.document_key.clone(),
        generation: request.generation,
        path: request.path.clone(),
        page_one_based: request.page_one_based.clamp(1, 200),
        page_count: 200,
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 255],
    }
}

struct GatedBackend {
    entered: Sender<PreviewRequest>,
    release: Receiver<()>,
    closed: Sender<thread::ThreadId>,
    trace: Arc<Mutex<Vec<u32>>>,
}
impl RenderBackend for GatedBackend {
    fn render(
        &mut self,
        request: &PreviewRequest,
        ticket: PreviewTicket,
    ) -> Result<PreviewFrame, PreviewError> {
        self.trace.lock().unwrap().push(request.page_one_based);
        self.entered.send(request.clone()).unwrap();
        self.release
            .recv_timeout(LIMIT)
            .map_err(|_| PreviewError::new(PreviewErrorKind::RenderFailed))?;
        Ok(frame(request, ticket))
    }
    fn close(&mut self) -> Result<(), PreviewError> {
        let _ = self.closed.send(thread::current().id());
        Ok(())
    }
}

#[test]
fn queued_replacement_and_stale_inflight_frame_keep_only_the_latest_identity() {
    let directory = TempDir::new().unwrap();
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let (closed_tx, closed) = mpsc::channel();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let calls = trace.clone();
    let mut renderer = PreviewRenderer::spawn(
        move || {
            Ok(Box::new(GatedBackend {
                entered: entered_tx,
                release: release_rx,
                closed: closed_tx,
                trace: calls,
            }))
        },
        None,
    )
    .unwrap();
    let first = submit(&renderer, request(&directory, 1));
    assert_eq!(entered.recv_timeout(LIMIT).unwrap().page_one_based, 1);
    let mut latest = first;
    for page in 2..=100 {
        let mut request = request(&directory, page);
        request.document_key = "replacement-at-same-path".into();
        request.generation = 2;
        latest = submit(&renderer, request);
    }
    assert!(renderer.shared.pending.lock().unwrap().request.is_some());
    release.send(()).unwrap();
    let actual = entered.recv_timeout(LIMIT).unwrap();
    assert_eq!(actual.page_one_based, 100);
    assert_eq!(actual.generation, 2);
    assert!(renderer.try_take_result().is_none());
    release.send(()).unwrap();
    let PreviewResult::Frame(frame) = result(&renderer) else {
        panic!("expected frame");
    };
    assert_eq!(frame.ticket, latest);
    assert_eq!(frame.generation, 2);
    assert_eq!(frame.document_key, "replacement-at-same-path");
    assert_eq!(*trace.lock().unwrap(), vec![1, 100]);
    assert!(renderer.try_take_result().is_none());
    renderer.close().unwrap();
    assert_ne!(closed.recv_timeout(LIMIT).unwrap(), thread::current().id());
}

#[test]
fn request_is_nonblocking_during_factory_initialization_and_keeps_one_pending_job() {
    let directory = TempDir::new().unwrap();
    let (factory_entered_tx, factory_entered) = mpsc::channel();
    let (factory_release, factory_rx) = mpsc::channel();
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let (closed_tx, _closed) = mpsc::channel();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let calls = trace.clone();
    let mut renderer = PreviewRenderer::spawn(
        move || {
            factory_entered_tx.send(thread::current().id()).unwrap();
            factory_rx.recv_timeout(LIMIT).unwrap();
            Ok(Box::new(GatedBackend {
                entered: entered_tx,
                release: release_rx,
                closed: closed_tx,
                trace: calls,
            }))
        },
        None,
    )
    .unwrap();
    assert_ne!(
        factory_entered.recv_timeout(LIMIT).unwrap(),
        thread::current().id()
    );
    let mut last = PreviewTicket(0);
    for page in 1..=40 {
        last = submit(&renderer, request(&directory, page));
    }
    assert!(renderer.try_take_result().is_none());
    assert_eq!(
        renderer
            .shared
            .pending
            .lock()
            .unwrap()
            .request
            .as_ref()
            .unwrap()
            .request
            .page_one_based,
        40
    );
    factory_release.send(()).unwrap();
    assert_eq!(entered.recv_timeout(LIMIT).unwrap().page_one_based, 40);
    release.send(()).unwrap();
    assert_eq!(result(&renderer).ticket(), Some(last));
    assert_eq!(*trace.lock().unwrap(), vec![40]);
    renderer.close().unwrap();
}

#[test]
fn busy_request_never_waits_for_the_slot_or_consumes_a_ticket() {
    let directory = TempDir::new().unwrap();
    let (release, rx) = mpsc::channel();
    let mut renderer = PreviewRenderer::spawn(
        move || {
            rx.recv_timeout(LIMIT).unwrap();
            Err(PreviewError::new(PreviewErrorKind::DllMissing))
        },
        None,
    )
    .unwrap();
    let held = renderer.shared.pending.lock().unwrap();
    assert_eq!(
        renderer.request(request(&directory, 1)).unwrap_err().kind,
        PreviewErrorKind::Busy
    );
    assert_eq!(held.sequence, 0);
    drop(held);
    release.send(()).unwrap();
    let _ = result(&renderer);
    let _ = renderer.close();
}

struct ImmediateBackend;
impl RenderBackend for ImmediateBackend {
    fn render(
        &mut self,
        request: &PreviewRequest,
        ticket: PreviewTicket,
    ) -> Result<PreviewFrame, PreviewError> {
        Ok(frame(request, ticket))
    }
}

#[test]
fn unread_results_are_replaced_and_wakeup_runs_after_publication() {
    let directory = TempDir::new().unwrap();
    let (wake_tx, wake) = mpsc::channel();
    let mut renderer = PreviewRenderer::spawn(
        || Ok(Box::new(ImmediateBackend)),
        Some(Arc::new(move || {
            let _ = wake_tx.send(());
        })),
    )
    .unwrap();
    submit(&renderer, request(&directory, 1));
    wake.recv_timeout(LIMIT).unwrap();
    let ticket = submit(&renderer, request(&directory, 2));
    wake.recv_timeout(LIMIT).unwrap();
    let PreviewResult::Frame(frame) = result(&renderer) else {
        panic!("expected frame");
    };
    assert_eq!(frame.ticket, ticket);
    assert_eq!(frame.page_one_based, 2);
    assert!(renderer.try_take_result().is_none());
    renderer.close().unwrap();
}

#[test]
fn close_waits_for_the_native_equivalent_and_drops_queued_work_before_returning() {
    let directory = TempDir::new().unwrap();
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let (closed_tx, closed) = mpsc::channel();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let calls = trace.clone();
    let mut renderer = PreviewRenderer::spawn(
        move || {
            Ok(Box::new(GatedBackend {
                entered: entered_tx,
                release: release_rx,
                closed: closed_tx,
                trace: calls,
            }))
        },
        None,
    )
    .unwrap();
    submit(&renderer, request(&directory, 1));
    entered.recv_timeout(LIMIT).unwrap();
    submit(&renderer, request(&directory, 2));
    let shared = renderer.shared.clone();
    let (done_tx, done) = mpsc::channel();
    let closer = thread::spawn(move || {
        let outcome = renderer.close();
        done_tx.send(outcome).unwrap();
    });
    let deadline = Instant::now() + LIMIT;
    while !shared.closing.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(done.try_recv().is_err(), "close detached an active render");
    release.send(()).unwrap();
    done.recv_timeout(LIMIT).unwrap().unwrap();
    closer.join().unwrap();
    closed.recv_timeout(LIMIT).unwrap();
    assert_eq!(*trace.lock().unwrap(), vec![1]);
    assert!(shared.result.lock().unwrap().is_none());
}

#[test]
fn initialization_failure_survives_a_queued_request_and_rejects_new_requests() {
    let directory = TempDir::new().unwrap();
    let (release, rx) = mpsc::channel();
    let mut renderer = PreviewRenderer::spawn(
        move || {
            rx.recv_timeout(LIMIT).unwrap();
            Err(PreviewError::new(PreviewErrorKind::DllMissing))
        },
        None,
    )
    .unwrap();
    submit(&renderer, request(&directory, 1));
    release.send(()).unwrap();
    let PreviewResult::Error(failure) = result(&renderer) else {
        panic!("expected startup failure");
    };
    assert_eq!(failure.error.kind, PreviewErrorKind::DllMissing);
    assert!(failure.ticket.is_none());
    assert_eq!(
        renderer.request(request(&directory, 2)).unwrap_err().kind,
        PreviewErrorKind::Closed
    );
    assert_eq!(
        renderer.close().unwrap_err().kind,
        PreviewErrorKind::DllMissing
    );
    renderer.close().unwrap();
}

#[test]
fn invalid_pixels_and_rust_panics_are_errors_without_passing_a_bad_frame_to_ui() {
    struct Invalid;
    impl RenderBackend for Invalid {
        fn render(
            &mut self,
            request: &PreviewRequest,
            ticket: PreviewTicket,
        ) -> Result<PreviewFrame, PreviewError> {
            let mut frame = frame(request, ticket);
            frame.rgba.clear();
            Ok(frame)
        }
    }
    struct Panics;
    impl RenderBackend for Panics {
        fn render(
            &mut self,
            _: &PreviewRequest,
            _: PreviewTicket,
        ) -> Result<PreviewFrame, PreviewError> {
            panic!("synthetic backend panic")
        }
    }
    let directory = TempDir::new().unwrap();
    let mut renderer = PreviewRenderer::spawn(|| Ok(Box::new(Invalid)), None).unwrap();
    let ticket = submit(&renderer, request(&directory, 1));
    let PreviewResult::Error(failure) = result(&renderer) else {
        panic!("bad pixels escaped");
    };
    assert_eq!(failure.ticket, Some(ticket));
    assert_eq!(failure.error.kind, PreviewErrorKind::InvalidPixels);
    renderer.close().unwrap();
    let mut renderer = PreviewRenderer::spawn(|| Ok(Box::new(Panics)), None).unwrap();
    submit(&renderer, request(&directory, 1));
    let PreviewResult::Error(failure) = result(&renderer) else {
        panic!("panic was not reported");
    };
    assert_eq!(failure.error.kind, PreviewErrorKind::ThreadFailed);
    assert_eq!(
        renderer.close().unwrap_err().kind,
        PreviewErrorKind::ThreadFailed
    );
}

#[test]
fn renderer_is_send_and_backend_is_created_rendered_and_destroyed_on_one_thread() {
    fn require_send<T: Send>() {}
    require_send::<PreviewRenderer>();
    type TraceRecords = Arc<Mutex<Vec<(&'static str, thread::ThreadId)>>>;
    struct Traced(TraceRecords);
    impl RenderBackend for Traced {
        fn render(
            &mut self,
            request: &PreviewRequest,
            ticket: PreviewTicket,
        ) -> Result<PreviewFrame, PreviewError> {
            self.0
                .lock()
                .unwrap()
                .push(("render", thread::current().id()));
            Ok(frame(request, ticket))
        }
    }
    impl Drop for Traced {
        fn drop(&mut self) {
            self.0
                .lock()
                .unwrap()
                .push(("drop", thread::current().id()));
        }
    }
    let trace = Arc::new(Mutex::new(Vec::new()));
    let records = trace.clone();
    let directory = TempDir::new().unwrap();
    let mut renderer = PreviewRenderer::spawn(
        move || {
            records
                .lock()
                .unwrap()
                .push(("factory", thread::current().id()));
            Ok(Box::new(Traced(records)))
        },
        None,
    )
    .unwrap();
    submit(&renderer, request(&directory, 1));
    let _ = result(&renderer);
    renderer.close().unwrap();
    let events = trace.lock().unwrap();
    assert_eq!(
        events.iter().map(|e| e.0).collect::<Vec<_>>(),
        vec!["factory", "render", "drop"]
    );
    let thread_id = events[0].1;
    assert_ne!(thread_id, thread::current().id());
    assert!(events.iter().all(|e| e.1 == thread_id));
}
