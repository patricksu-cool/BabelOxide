//! A persistent runtime owns tasks and disk I/O. The UI only sends intentions;
//! it never aborts a task future or waits for native shutdown while rendering.

use anyhow::{Context, Result};
use babeloxide_bridge::WorkerOptions;
use babeloxide_config::{ConfigStore, Settings};
use babeloxide_core::{CacheBackend, CacheStore};
use babeloxide_pdf_preview::PreviewRenderer;
use babeloxide_tasks::{TaskHandle, TaskManager, task_from_settings};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use tokio::sync::{Notify, mpsc};

use crate::model::validate_result_path;

pub(crate) enum Command {
    Translate(Settings),
    Save(Settings),
    Reload,
    Download {
        url: String,
        cancel: tokio_util::sync::CancellationToken,
    },
    ExportZip {
        sources: Vec<PathBuf>,
        destination: PathBuf,
    },
    Copy {
        source: PathBuf,
        destination: PathBuf,
    },
    Open(PathBuf),
}
pub(crate) enum Event {
    Submitted {
        handle: TaskHandle,
        inputs: Vec<PathBuf>,
    },
    Saved(Settings),
    Reloaded(Settings),
    ReloadFailed,
    Downloaded(PathBuf),
    DownloadFailed(String),
    Exported(PathBuf),
    ExportFailed(String),
    SaveFailed,
    SubmissionFailed(String),
    Copied(PathBuf),
    Warning(&'static str),
    Failed(String),
}

struct Shutdown {
    requested: AtomicBool,
    done: AtomicBool,
    notify: Notify,
    preview: Mutex<Option<PreviewRenderer>>,
    repaint: Mutex<Option<eframe::egui::Context>>,
}
impl Shutdown {
    fn wake(&self) {
        if let Some(context) = self
            .repaint
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            context.request_repaint();
        }
    }
}

#[derive(Clone)]
pub(crate) struct Control {
    shutdown: Arc<Shutdown>,
}
impl Control {
    pub fn close(&self, preview: Option<PreviewRenderer>) {
        if let Some(preview) = preview {
            *self
                .shutdown
                .preview
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(preview);
        }
        self.shutdown.requested.store(true, Ordering::Release);
        self.shutdown.notify.notify_one();
    }
    pub fn is_closing(&self) -> bool {
        self.shutdown.requested.load(Ordering::Acquire)
    }
    pub fn is_done(&self) -> bool {
        self.shutdown.done.load(Ordering::Acquire)
    }
}

pub(crate) struct Background {
    commands: mpsc::Sender<Command>,
    pub events: mpsc::Receiver<Event>,
    pub control: Control,
}
pub(crate) struct BackgroundThread {
    join: Option<JoinHandle<()>>,
    control: Control,
}
impl BackgroundThread {
    /// Called after the native event loop returns, never from App::ui.
    pub fn finish(mut self) {
        self.control.close(None);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl Drop for BackgroundThread {
    fn drop(&mut self) {
        self.control.close(None);
        // This owner stays outside App and therefore outlives the native event
        // loop. It also reaps cleanup when launch unwinds before finish().
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Background {
    pub fn start(
        store: ConfigStore,
        worker: WorkerOptions,
        cache_file: Option<PathBuf>,
    ) -> Result<(Self, BackgroundThread)> {
        let (commands, receiver) = mpsc::channel(16);
        let (events, event_receiver) = mpsc::channel(32);
        let shutdown = Arc::new(Shutdown {
            requested: AtomicBool::new(false),
            done: AtomicBool::new(false),
            notify: Notify::new(),
            preview: Mutex::new(None),
            repaint: Mutex::new(None),
        });
        let control = Control {
            shutdown: shutdown.clone(),
        };
        let join = std::thread::Builder::new()
            .name("babeloxide-application".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(run(
                        receiver,
                        events,
                        shutdown.clone(),
                        store,
                        worker,
                        cache_file,
                    )),
                    Err(_) => {
                        let _ = events.try_send(Event::Failed(
                            "Cannot start the application task runtime".into(),
                        ));
                        shutdown.wake();
                        // Retain the cleanup owner even if Tokio could not be
                        // created. The UI can still hand us its renderer.
                        while !shutdown.requested.load(Ordering::Acquire) {
                            std::thread::park_timeout(std::time::Duration::from_millis(25));
                        }
                    }
                }
                let preview = shutdown
                    .preview
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                if let Some(mut preview) = preview {
                    let _ = preview.close();
                }
                shutdown.done.store(true, Ordering::Release);
                shutdown.wake();
            })
            .context("Cannot start the application background thread")?;
        Ok((
            Self {
                commands,
                events: event_receiver,
                control: control.clone(),
            },
            BackgroundThread {
                join: Some(join),
                control,
            },
        ))
    }
    pub fn attach_repaint(&self, context: eframe::egui::Context) {
        *self
            .control
            .shutdown
            .repaint
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(context);
    }
    pub fn send(&self, command: Command) -> Result<()> {
        anyhow::ensure!(!self.control.is_closing(), "The application is closing");
        self.commands
            .try_send(command)
            .map_err(|_| anyhow::anyhow!("The application is busy; try again shortly"))
    }
}

async fn emit(sender: &mpsc::Sender<Event>, shutdown: &Shutdown, event: Event) {
    if shutdown.requested.load(Ordering::Acquire) {
        return;
    }
    tokio::select! {biased; _=shutdown.notify.notified()=>{}, _=sender.send(event)=>{shutdown.wake();}}
}

async fn run(
    mut commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<Event>,
    shutdown: Arc<Shutdown>,
    store: ConfigStore,
    worker: WorkerOptions,
    cache_file: Option<PathBuf>,
) {
    let manager = match TaskManager::new(1) {
        Ok(manager) => manager,
        Err(_) => {
            let _ = events.try_send(Event::Failed("Cannot create the task manager".into()));
            return;
        }
    };
    let mut downloads = Vec::new();
    loop {
        if shutdown.requested.load(Ordering::Acquire) {
            break;
        }
        let command = tokio::select! {biased; _=shutdown.notify.notified()=>continue, command=commands.recv()=>match command {Some(command)=>command,None=>break}};
        match command {
            Command::Reload => {
                let source = store.clone();
                match tokio::task::spawn_blocking(move || source.load()).await {
                    Ok(Ok(settings)) => emit(&events, &shutdown, Event::Reloaded(settings)).await,
                    _ => emit(&events, &shutdown, Event::ReloadFailed).await,
                }
            }
            Command::Download { url, cancel } => {
                let future = crate::transfers::download_pdf(&url, &cancel);
                tokio::pin!(future);
                let downloaded = tokio::select! {
                    result = &mut future => result,
                    _ = shutdown.notify.notified() => { cancel.cancel(); future.await }
                };
                if shutdown.requested.load(Ordering::Acquire) {
                    break;
                }
                match downloaded {
                    Ok(downloaded) => {
                        let path = downloaded.path;
                        downloads.push(downloaded.directory);
                        emit(&events, &shutdown, Event::Downloaded(path)).await;
                    }
                    Err(error) => {
                        emit(&events, &shutdown, Event::DownloadFailed(error.to_string())).await
                    }
                }
            }
            Command::ExportZip {
                sources,
                destination,
            } => {
                let output = destination.clone();
                match tokio::task::spawn_blocking(move || {
                    crate::transfers::export_zip(&sources, &destination)
                })
                .await
                {
                    Ok(Ok(())) => emit(&events, &shutdown, Event::Exported(output)).await,
                    Ok(Err(error)) => {
                        emit(&events, &shutdown, Event::ExportFailed(error.to_string())).await
                    }
                    Err(_) => {
                        emit(
                            &events,
                            &shutdown,
                            Event::ExportFailed("Cannot export batch ZIP".into()),
                        )
                        .await
                    }
                }
            }
            Command::Translate(settings) => {
                let inputs = settings
                    .basic()
                    .map(|basic| basic.input_files.into_iter().map(PathBuf::from).collect())
                    .unwrap_or_default();
                let cache = if settings
                    .translation()
                    .is_ok_and(|settings| settings.ignore_cache)
                {
                    None
                } else {
                    let path = cache_file
                        .clone()
                        .unwrap_or_else(|| store.app_root().join("cache/translations.v2.db"));
                    match tokio::task::spawn_blocking(move || -> Result<CacheStore> {
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        Ok(CacheStore::open(path)?)
                    })
                    .await
                    {
                        Ok(Ok(cache)) => Some(Arc::new(cache) as Arc<dyn CacheBackend>),
                        _ => {
                            emit(
                                &events,
                                &shutdown,
                                Event::Warning(
                                    "Translation cache is unavailable; continuing without it",
                                ),
                            )
                            .await;
                            None
                        }
                    }
                };
                // Shutdown may have arrived during cache open. Never start a
                // new worker after accepting the close request.
                if shutdown.requested.load(Ordering::Acquire) {
                    break;
                }
                let prepared = match task_from_settings(&settings, worker.clone(), cache) {
                    Ok(mut spec) => tokio::task::spawn_blocking(move || -> Result<_> {
                        let mut guards =
                            crate::output_paths::isolate_colliding_outputs(&mut spec.jobs)?;
                        guards.extend(crate::output_paths::preserve_existing_outputs(
                            &mut spec.jobs,
                        )?);
                        Ok((spec, guards))
                    })
                    .await
                    .context("Cannot prepare batch output folders")
                    .and_then(|result| result),
                    Err(error) => Err(error),
                };
                // Preparation can involve filesystem I/O. Dropping its guards
                // cleans the private folders if closing won the race.
                if shutdown.requested.load(Ordering::Acquire) {
                    break;
                }
                let submitted = prepared.and_then(|(spec, guards)| {
                    let handle = manager.submit(spec)?;
                    let isolated = !guards.is_empty();
                    for guard in guards {
                        let _ = guard.keep();
                    }
                    Ok((handle, isolated))
                });
                match submitted {
                    Ok((handle, isolated)) => {
                        let mut updates = handle.subscribe();
                        let wake = shutdown.clone();
                        tokio::spawn(async move {
                            loop {
                                let terminal = updates.borrow_and_update().status.is_terminal();
                                wake.wake();
                                if terminal || updates.changed().await.is_err() {
                                    break;
                                }
                            }
                        });
                        emit(&events, &shutdown, Event::Submitted { handle, inputs }).await;
                        if isolated {
                            emit(
                                &events,
                                &shutdown,
                                Event::Warning(
                                    "PDFs with matching names use separate output subfolders",
                                ),
                            )
                            .await;
                        }
                    }
                    Err(error) => {
                        emit(
                            &events,
                            &shutdown,
                            Event::SubmissionFailed(error.to_string()),
                        )
                        .await
                    }
                }
            }
            Command::Save(settings) => {
                let saved = settings.clone();
                let destination = store.clone();
                let result =
                    tokio::task::spawn_blocking(move || destination.save_user_defaults(&saved))
                        .await;
                match result {
                    Ok(Ok(())) => emit(&events, &shutdown, Event::Saved(settings)).await,
                    _ => emit(&events, &shutdown, Event::SaveFailed).await,
                }
            }
            Command::Copy {
                source,
                destination,
            } => {
                let output = destination.clone();
                let copied =
                    tokio::task::spawn_blocking(move || copy_result(&source, &destination)).await;
                match copied {
                    Ok(Ok(())) => emit(&events, &shutdown, Event::Copied(output)).await,
                    _ => {
                        emit(
                            &events,
                            &shutdown,
                            Event::Failed(
                                "Cannot save this result to the selected destination".into(),
                            ),
                        )
                        .await
                    }
                }
            }
            Command::Open(path) => {
                if !matches!(
                    tokio::task::spawn_blocking(move || open_result(&path)).await,
                    Ok(Ok(()))
                ) {
                    emit(
                        &events,
                        &shutdown,
                        Event::Failed(
                            "Cannot open this result with the default application".into(),
                        ),
                    )
                    .await;
                }
            }
        }
    }
    // Do not select away cleanup: tasks can still own native children or a
    // SQLite writer that must roll back before the application exits.
    manager.shutdown().await;
    drop(downloads);
}

pub(crate) fn copy_result(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    use std::io::Write;
    validate_result_path(source)?;
    anyhow::ensure!(destination.is_absolute(), "Choose an absolute destination");
    anyhow::ensure!(
        destination
            .extension()
            .zip(source.extension())
            .is_some_and(|(destination, source)| destination.eq_ignore_ascii_case(source)),
        "Keep the result file type"
    );
    let source = source.canonicalize().context("Cannot resolve result")?;
    if destination.canonicalize().is_ok_and(|path| path == source) {
        return Ok(());
    }
    let parent = destination
        .parent()
        .context("Choose a destination folder")?;
    let mut input = std::fs::File::open(source).context("Cannot read result")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("Cannot create result temporary file")?;
    std::io::copy(&mut input, &mut temporary).context("Cannot copy result")?;
    temporary.flush().context("Cannot flush result")?;
    temporary
        .as_file()
        .sync_all()
        .context("Cannot sync result")?;
    // Replace only after a complete copy. Failed I/O or publication keeps the
    // previous destination intact; never fall back to deleting it first.
    temporary
        .persist(destination)
        .map_err(|_| anyhow::anyhow!("Cannot publish result atomically"))?;
    Ok(())
}

#[cfg(windows)]
fn open_result(path: &std::path::Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    if path.is_dir() {
        anyhow::ensure!(path.is_absolute(), "Choose an absolute output folder");
    } else {
        validate_result_path(path)?;
    }
    let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    // No command line or shell expansion: this opens one validated result path
    // through its registered desktop application.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            filename.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!((result as isize) > 32, "Cannot open result");
    Ok(())
}
#[cfg(target_os = "macos")]
fn open_result(path: &std::path::Path) -> Result<()> {
    if path.is_dir() {
        anyhow::ensure!(path.is_absolute(), "Choose an absolute output folder");
    } else {
        validate_result_path(path)?;
    }
    let status = std::process::Command::new("/usr/bin/open")
        .arg("--")
        .arg(path)
        .status()
        .context("Cannot open the result")?;
    anyhow::ensure!(status.success(), "Cannot open the result");
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn open_result(_path: &std::path::Path) -> Result<()> {
    anyhow::bail!("Opening results is currently available on Windows")
}

#[cfg(test)]
mod tests {
    use super::*;
    mod http_fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-support/loopback.rs"
        ));
    }
    #[test]
    fn result_copy_uses_exact_paths_without_command_interpretation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("a & b.pdf");
        let destination = temp.path().join("saved.pdf");
        std::fs::write(&source, b"synthetic PDF").unwrap();
        copy_result(&source, &destination).unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"synthetic PDF");
        assert!(copy_result(&source, &temp.path().join("wrong.exe")).is_err());
    }
    #[test]
    fn result_copy_to_an_alias_of_itself_preserves_the_source() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("result.pdf");
        std::fs::write(&source, b"complete original PDF").unwrap();
        copy_result(&source, &temp.path().join(".").join("result.pdf")).unwrap();
        assert_eq!(std::fs::read(source).unwrap(), b"complete original PDF");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn result_copy_publication_failure_preserves_old_destination() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("result.pdf");
        let destination = temp.path().join("saved.pdf");
        std::fs::write(&source, b"new complete PDF").unwrap();
        std::fs::write(&destination, b"previous saved PDF").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .unwrap();
        assert!(copy_result(&source, &destination).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"previous saved PDF");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
        drop(locked);
        copy_result(&source, &destination).unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"new complete PDF");
    }
    #[cfg(windows)]
    #[test]
    fn result_copy_read_failure_does_not_touch_existing_destination() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("result.pdf");
        let destination = temp.path().join("saved.pdf");
        std::fs::write(&source, b"new complete PDF").unwrap();
        std::fs::write(&destination, b"previous saved PDF").unwrap();
        let _locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&source)
            .unwrap();
        assert!(copy_result(&source, &destination).is_err());
        assert_eq!(std::fs::read(destination).unwrap(), b"previous saved PDF");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
    }
    #[test]
    fn empty_background_shutdown_creates_no_worker_or_settings() {
        let temp = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(temp.path().into()).unwrap();
        let worker = WorkerOptions {
            python: temp.path().join("must-not-start.exe"),
            script: temp.path().join("must-not-read.py"),
            ..WorkerOptions::default()
        };
        let (background, thread) = Background::start(store, worker, None).unwrap();
        background.control.close(None);
        thread.finish();
        assert!(background.control.is_done());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn explicit_save_preserves_unknown_and_sensitive_settings_but_not_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(temp.path().into()).unwrap();
        let mut settings = Settings::default();
        settings
            .set(
                "gui_settings.disable_config_auto_save",
                serde_json::json!(true),
            )
            .unwrap();
        settings
            .set("future_section", serde_json::json!({"keep":42}))
            .unwrap();
        settings
            .set(
                "openai_detail.openai_api_key",
                serde_json::json!("synthetic-password-do-not-log"),
            )
            .unwrap();
        settings
            .set(
                "basic.input_files",
                serde_json::json!([temp.path().join("input.pdf")]),
            )
            .unwrap();
        let (mut background, thread) =
            Background::start(store.clone(), WorkerOptions::default(), None).unwrap();
        assert!(!store.path().exists());
        background.send(Command::Save(settings)).unwrap();
        let event =
            tokio::time::timeout(std::time::Duration::from_secs(10), background.events.recv())
                .await
                .unwrap()
                .unwrap();
        assert!(matches!(event, Event::Saved(_)));
        let saved = store.load().unwrap();
        assert_eq!(
            saved.get("future_section.keep"),
            Some(&serde_json::json!(42))
        );
        assert_eq!(
            saved.get("openai_detail.openai_api_key"),
            Some(&serde_json::json!("synthetic-password-do-not-log"))
        );
        assert!(saved.basic().unwrap().input_files.is_empty());
        assert!(saved.gui().unwrap().disable_config_auto_save);
        background.control.close(None);
        thread.finish();
    }

    struct WorkerFixture {
        directory: tempfile::TempDir,
        worker: WorkerOptions,
        settings: Settings,
        server: http_fixture::LoopbackProvider,
    }
    impl WorkerFixture {
        fn new(scenario: &str) -> Self {
            let server = http_fixture::LoopbackProvider::new().unwrap();
            let directory = tempfile::tempdir().unwrap();
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .unwrap();
            let python = std::env::var_os("BABELOXIDE_TEST_PYTHON")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    root.join(if cfg!(windows) {
                        ".venv/Scripts/python.exe"
                    } else {
                        ".venv/bin/python"
                    })
                });
            assert!(
                python.is_file(),
                "The transport fixture requires project Python or BABELOXIDE_TEST_PYTHON"
            );
            let script = directory.path().join("controlled_worker.py");
            std::fs::write(
                &script,
                include_str!("../../babeldoc-bridge/tests/fixtures/fake_worker.py"),
            )
            .unwrap();
            std::fs::write(
                directory.path().join("scenario.json"),
                serde_json::to_vec(&serde_json::json!({"scenario":scenario})).unwrap(),
            )
            .unwrap();
            let input = directory.path().join("input.pdf");
            std::fs::write(&input, b"%PDF-1.4\n%%EOF").unwrap();
            let worker = WorkerOptions {
                python,
                script,
                home: Some(directory.path().join("isolated-home")),
                asset_manifest: None,
                startup_timeout: std::time::Duration::from_secs(5),
                idle_timeout: std::time::Duration::from_secs(30),
                shutdown_timeout: std::time::Duration::from_millis(150),
            };
            let mut settings = Settings::default();
            for (path, value) in [
                ("openai", serde_json::json!(true)),
                (
                    "openai_detail.openai_model",
                    serde_json::json!("unused-fixture"),
                ),
                (
                    "openai_detail.openai_api_key",
                    serde_json::json!("synthetic-unused"),
                ),
                (
                    "openai_detail.openai_base_url",
                    serde_json::json!(server.endpoint()),
                ),
                ("basic.input_files", serde_json::json!([input])),
                (
                    "translation.output",
                    serde_json::json!(directory.path().join("output")),
                ),
                ("translation.ignore_cache", serde_json::json!(true)),
                (
                    "translation.no_auto_extract_glossary",
                    serde_json::json!(true),
                ),
            ] {
                settings.set(path, value).unwrap();
            }
            Self {
                directory,
                worker,
                settings,
                server,
            }
        }
        fn background(&self) -> (Background, BackgroundThread) {
            Background::start(
                ConfigStore::new(self.directory.path().join("app")).unwrap(),
                self.worker.clone(),
                None,
            )
            .unwrap()
        }
    }
    async fn submitted(background: &mut Background) -> TaskHandle {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                match background.events.recv().await.unwrap() {
                    Event::Submitted { handle, .. } => return handle,
                    Event::Warning(_) => {}
                    _ => panic!("Expected controlled task submission"),
                }
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn submit_initializes_services_and_exposes_real_result_paths_over_loopback() {
        let fixture = WorkerFixture::new("finished_then_exit");
        let (mut background, thread) = fixture.background();
        assert!(!fixture.directory.path().join("worker.pid").exists());
        background
            .send(Command::Translate(fixture.settings.clone()))
            .unwrap();
        let handle = submitted(&mut background).await;
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(10), handle.wait())
            .await
            .unwrap();
        assert_eq!(snapshot.status, babeloxide_tasks::TaskStatus::Succeeded);
        assert_eq!(snapshot.completed_files, 1);
        assert_eq!(fixture.server.requests().len(), 2);
        let result = PathBuf::from(snapshot.results[0]["mono_pdf_path"].as_str().unwrap());
        assert!(result.is_file());
        assert!(!fixture.directory.path().join("app/cache").exists());
        assert!(!fixture.directory.path().join("app/config.toml").exists());
        background.control.close(None);
        thread.finish();
    }
    #[tokio::test]
    async fn closing_waits_for_the_shared_task_cancel_before_reporting_done() {
        let fixture = WorkerFixture::new("cancel_hang");
        let (mut background, thread) = fixture.background();
        background
            .send(Command::Translate(fixture.settings.clone()))
            .unwrap();
        let handle = submitted(&mut background).await;
        let mut updates = handle.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let snapshot = updates.borrow_and_update().clone();
                assert!(!snapshot.status.is_terminal());
                if snapshot.progress.is_some() {
                    break;
                }
                updates.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        background.control.close(None);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !background.control.is_done() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            handle.snapshot().status,
            babeloxide_tasks::TaskStatus::Cancelled
        );
        assert!(
            background
                .send(Command::Translate(fixture.settings.clone()))
                .is_err()
        );
        thread.finish();
    }
}
