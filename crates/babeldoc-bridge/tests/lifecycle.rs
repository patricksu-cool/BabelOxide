//! Real child processes exercise the IPC lifecycle without importing BabelDOC.
//! No fixture sends translation requests, so the synthetic provider is unused.

use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use babeldoc_bridge::{WorkerOptions, translate_pdf};
use pdf2zh_core::{PdfJob, TranslationService, TranslatorConfig};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{sync::mpsc, time::timeout};
use tokio_util::sync::CancellationToken;

const DEADLINE: Duration = Duration::from_secs(10);

struct Fixture {
    directory: TempDir,
    worker: WorkerOptions,
    job: PdfJob,
}

impl Fixture {
    fn new(scenario: &str) -> Result<Self> {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let python = std::env::var_os("PDF2ZH_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                repository.join(if cfg!(windows) {
                    ".venv/Scripts/python.exe"
                } else {
                    ".venv/bin/python"
                })
            });
        ensure!(
            python.is_file(),
            "Lifecycle tests need the existing local Python or PDF2ZH_TEST_PYTHON"
        );
        let directory = tempfile::tempdir()?;
        let script = directory.path().join("fake_worker.py");
        fs::write(&script, include_str!("fixtures/fake_worker.py"))?;
        fs::write(
            directory.path().join("scenario.json"),
            serde_json::to_vec(&json!({"scenario": scenario}))?,
        )?;
        let input = directory.path().join("input.pdf");
        fs::write(&input, b"%PDF-1.4\n% transport fixture only\n%%EOF\n")?;
        let output = directory.path().join("output");
        fs::create_dir(&output)?;
        let worker = WorkerOptions {
            python,
            script,
            home: Some(directory.path().join("worker-home")),
            startup_timeout: Duration::from_secs(5),
            // Failure tests must finish far before this timeout.
            idle_timeout: Duration::from_secs(60),
            shutdown_timeout: Duration::from_millis(250),
        };
        let job = PdfJob {
            input_file: input,
            output_dir: output,
            lang_in: "en".into(),
            lang_out: "zh-TW".into(),
            pages: None,
            qps: 1,
            pool_max_workers: 1,
            no_auto_extract_glossary: true,
            watermark_output_mode: "no_watermark".into(),
            extra: BTreeMap::new(),
        };
        Ok(Self {
            directory,
            worker,
            job,
        })
    }

    fn service(&self) -> Result<Arc<TranslationService>> {
        Ok(Arc::new(TranslationService::new(TranslatorConfig {
            provider: "openai".into(),
            endpoint: "http://127.0.0.1:9/v1".into(),
            api_key: "synthetic-unused-fixture-key".into(),
            model: "unused-fixture-model".into(),
            timeout_seconds: 1.0,
            lang_out: "zh-TW".into(),
            options: json!({"qps": 1, "max_concurrency": 1}),
        })?))
    }

    async fn run(&self) -> Result<Value> {
        timeout(
            DEADLINE,
            translate_pdf(
                &self.worker,
                self.job.clone(),
                self.service()?,
                CancellationToken::new(),
                None,
            ),
        )
        .await
        .context("Fixture exceeded lifecycle deadline instead of completing cleanup")?
    }
}

#[tokio::test]
async fn startup_crash_is_reported_without_waiting_for_startup_timeout() -> Result<()> {
    let mut fixture = Fixture::new("startup_crash")?;
    fixture.worker.startup_timeout = Duration::from_secs(60);
    let error = fixture.run().await.expect_err("crash must not succeed");
    ensure!(
        !format!("{error:#}").contains("Fixture exceeded"),
        "startup crash was only noticed by the test deadline: {error:#}"
    );
    Ok(())
}

#[tokio::test]
async fn exit_zero_without_terminal_is_not_success() -> Result<()> {
    let fixture = Fixture::new("exit_without_terminal")?;
    let error = fixture
        .run()
        .await
        .expect_err("exit 0 is not a task result");
    ensure!(
        !format!("{error:#}").contains("Fixture exceeded"),
        "{error:#}"
    );
    Ok(())
}

#[tokio::test]
async fn buffered_finished_before_exit_zero_is_accepted() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let result = fixture.run().await?;
    for key in ["mono_pdf_path", "dual_pdf_path"] {
        let path = result[key]
            .as_str()
            .context("missing expected output path")?;
        ensure!(PathBuf::from(path).is_file());
    }
    Ok(())
}

#[tokio::test]
async fn finished_rejects_each_missing_requested_output() -> Result<()> {
    for (scenario, expected) in [
        ("missing_mono", "monolingual"),
        ("missing_dual", "bilingual"),
    ] {
        let fixture = Fixture::new(scenario)?;
        let error = fixture
            .run()
            .await
            .expect_err("both outputs were requested");
        ensure!(
            format!("{error:#}").contains(expected),
            "{scenario} must report the missing output, got {error:#}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn malformed_oversized_partial_and_wrong_identity_frames_fail() -> Result<()> {
    for (scenario, expected) in [
        ("malformed", "Invalid worker JSON"),
        ("oversize", "frame exceeds limit"),
        ("partial_eof", "incomplete frame"),
        ("wrong_session", "session or task mismatch"),
        ("wrong_task", "session or task mismatch"),
    ] {
        let fixture = Fixture::new(scenario)?;
        let error = fixture.run().await.expect_err("invalid frame must fail");
        ensure!(
            format!("{error:#}").contains(expected),
            "{scenario} must fail for its protocol violation, got {error:#}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn cancellation_force_kills_worker_that_ignores_stdin() -> Result<()> {
    let fixture = Fixture::new("cancel_hang")?;
    let cancel = CancellationToken::new();
    let (events, mut received) = mpsc::unbounded_channel();
    let operation = translate_pdf(
        &fixture.worker,
        fixture.job.clone(),
        fixture.service()?,
        cancel.clone(),
        Some(events),
    );
    tokio::pin!(operation);
    let event = tokio::select! {
        event = timeout(DEADLINE, received.recv()) => event?.context("fixture progress closed")?,
        result = &mut operation => bail!("worker ended before cancellation: {result:?}"),
    };
    ensure!(event["stage"] == "worker_ready");
    #[cfg(windows)]
    let worker = windows_process::ObservedProcess::from_pid_file(
        fixture.directory.path().join("worker.pid"),
    )?;
    cancel.cancel();
    let error = timeout(DEADLINE, &mut operation)
        .await
        .context("cancellation did not force worker cleanup")?
        .expect_err("cancel must not succeed");
    ensure!(format!("{error:#}").contains("cancelled"));
    #[cfg(windows)]
    worker.assert_exited().await?;
    Ok(())
}

#[cfg(windows)]
mod windows_process {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    /// Keep the original process handle, so PID reuse cannot falsify the check.
    pub struct ObservedProcess(HANDLE);

    impl ObservedProcess {
        pub fn from_pid_file(path: PathBuf) -> Result<Self> {
            let pid: u32 = fs::read_to_string(path)?.trim().parse()?;
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            ensure!(
                !handle.is_null(),
                "fixture process was not alive for observation"
            );
            let process = Self(handle);
            ensure!(
                process.running()?,
                "fixture process exited before its gate opened"
            );
            Ok(process)
        }

        fn running(&self) -> Result<bool> {
            let mut code = 0;
            let result = unsafe { GetExitCodeProcess(self.0, &mut code) };
            ensure!(result != 0, "GetExitCodeProcess failed");
            Ok(code == STILL_ACTIVE as u32)
        }

        pub async fn assert_exited(&self) -> Result<()> {
            timeout(Duration::from_secs(3), async {
                while self.running()? {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await
            .context("contained fixture process survived bridge cleanup")??;
            Ok(())
        }
    }

    impl Drop for ObservedProcess {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(windows)]
#[tokio::test]
async fn crash_is_observed_while_descendant_keeps_stdout_open() -> Result<()> {
    let fixture = Fixture::new("crash_with_descendant")?;
    let (events, mut received) = mpsc::unbounded_channel();
    let operation = translate_pdf(
        &fixture.worker,
        fixture.job.clone(),
        fixture.service()?,
        CancellationToken::new(),
        Some(events),
    );
    tokio::pin!(operation);
    let event = tokio::select! {
        event = timeout(DEADLINE, received.recv()) => event?.context("fixture progress closed")?,
        result = &mut operation => bail!("worker ended before crash gate: {result:?}"),
    };
    ensure!(event["stage"] == "descendant_ready");
    let worker = windows_process::ObservedProcess::from_pid_file(
        fixture.directory.path().join("worker.pid"),
    )?;
    let descendant = windows_process::ObservedProcess::from_pid_file(
        fixture.directory.path().join("descendant.pid"),
    )?;
    fs::write(
        fixture.directory.path().join("release-worker"),
        b"crash now",
    )?;
    let error = timeout(DEADLINE, &mut operation)
        .await
        .context("worker exit was hidden by descendant's open stdout")?
        .expect_err("worker crash must fail");
    ensure!(format!("{error:#}").contains("crashed"), "{error:#}");
    worker.assert_exited().await?;
    descendant.assert_exited().await?;
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn cancellation_force_kills_the_entire_contained_tree() -> Result<()> {
    let fixture = Fixture::new("cancel_hang_with_descendant")?;
    let cancel = CancellationToken::new();
    let (events, mut received) = mpsc::unbounded_channel();
    let operation = translate_pdf(
        &fixture.worker,
        fixture.job.clone(),
        fixture.service()?,
        cancel.clone(),
        Some(events),
    );
    tokio::pin!(operation);
    let event = tokio::select! {
        event = timeout(DEADLINE, received.recv()) => event?.context("fixture progress closed")?,
        result = &mut operation => bail!("worker ended before cancellation: {result:?}"),
    };
    ensure!(event["stage"] == "descendant_ready");
    let worker = windows_process::ObservedProcess::from_pid_file(
        fixture.directory.path().join("worker.pid"),
    )?;
    let descendant = windows_process::ObservedProcess::from_pid_file(
        fixture.directory.path().join("descendant.pid"),
    )?;
    cancel.cancel();
    let error = timeout(DEADLINE, &mut operation)
        .await
        .context("cancelled process tree did not finish cleanup")?
        .expect_err("cancel must not succeed");
    ensure!(format!("{error:#}").contains("cancelled"), "{error:#}");
    worker.assert_exited().await?;
    descendant.assert_exited().await?;
    Ok(())
}
