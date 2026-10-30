use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use babeldoc_bridge::WorkerOptions;
use pdf2zh_core::{PdfJob, TranslationService, TranslatorConfig};
use pdf2zh_tasks::{TaskHandle, TaskManager, TaskSnapshot, TaskSpec, TaskStatus};
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

const DEADLINE: Duration = Duration::from_secs(10);

struct Fixture {
    directory: TempDir,
    worker: WorkerOptions,
    job: PdfJob,
}
impl Fixture {
    fn new(scenario: &str) -> Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let python = std::env::var_os("PDF2ZH_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                root.join(if cfg!(windows) {
                    ".venv/Scripts/python.exe"
                } else {
                    ".venv/bin/python"
                })
            });
        ensure!(
            python.is_file(),
            "Need the project Python or PDF2ZH_TEST_PYTHON for transport fixtures"
        );
        let directory = tempfile::tempdir()?;
        let script = directory.path().join("fake_worker.py");
        fs::write(
            &script,
            include_str!("../../babeldoc-bridge/tests/fixtures/fake_worker.py"),
        )?;
        fs::write(
            directory.path().join("scenario.json"),
            serde_json::to_vec(&json!({"scenario":scenario}))?,
        )?;
        let input = directory.path().join("input.pdf");
        fs::write(&input, b"%PDF-1.4\n%%EOF\n")?;
        let worker = WorkerOptions {
            python,
            script,
            home: Some(directory.path().join("home")),
            startup_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
            shutdown_timeout: Duration::from_millis(250),
        };
        let job = PdfJob {
            input_file: input,
            output_dir: directory.path().join("output"),
            lang_in: "en".into(),
            lang_out: "zh".into(),
            pages: None,
            qps: 4,
            pool_max_workers: 4,
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

    fn spec(&self, count: usize) -> Result<TaskSpec> {
        let translator = Arc::new(TranslationService::new(TranslatorConfig {
            provider: "openai".into(),
            endpoint: "http://127.0.0.1:9/v1".into(),
            api_key: "synthetic-unused-key".into(),
            model: "unused".into(),
            timeout_seconds: 1.0,
            lang_out: "zh".into(),
            options: json!({"qps":4}),
        })?);
        Ok(TaskSpec {
            worker: self.worker.clone(),
            jobs: vec![self.job.clone(); count],
            translator,
            term_translator: None,
        })
    }
}

async fn progress(task: &TaskHandle) -> Result<TaskSnapshot> {
    let mut updates = task.subscribe();
    timeout(DEADLINE, async {
        loop {
            let snapshot = updates.borrow_and_update().clone();
            ensure!(
                !snapshot.status.is_terminal(),
                "task ended before progress: {snapshot:?}"
            );
            if snapshot.progress.is_some() {
                return Ok(snapshot);
            }
            updates.changed().await.context("task snapshot closed")?;
        }
    })
    .await
    .context("task never reported progress")?
}

#[tokio::test]
async fn a_batch_retains_all_file_results_and_terminal_for_late_subscribers() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let manager = TaskManager::new(1)?;
    let task = manager.submit(fixture.spec(2)?)?;
    let snapshot = timeout(DEADLINE, task.wait()).await?;
    ensure!(snapshot.status == TaskStatus::Succeeded);
    ensure!(snapshot.completed_files == 2 && snapshot.results.len() == 2);
    ensure!(task.subscribe().borrow().status == TaskStatus::Succeeded);
    ensure!(task.wait().await.status == TaskStatus::Succeeded);
    task.cancel();
    ensure!(
        task.snapshot().status == TaskStatus::Succeeded,
        "late cancel changed a terminal result"
    );
    Ok(())
}

#[tokio::test]
async fn a_cancelled_queued_batch_never_spawns_a_worker_and_releases_its_slot() -> Result<()> {
    let active = Fixture::new("cancel_hang")?;
    let queued = Fixture::new("finished_then_exit")?;
    let manager = TaskManager::new(1)?;
    let first = manager.submit(active.spec(1)?)?;
    progress(&first).await?;
    let second = manager.submit(queued.spec(1)?)?;
    ensure!(second.snapshot().status == TaskStatus::Queued);
    second.cancel();
    ensure!(timeout(DEADLINE, second.wait()).await?.status == TaskStatus::Cancelled);
    ensure!(!queued.directory.path().join("worker.pid").exists());
    first.cancel();
    ensure!(timeout(DEADLINE, first.wait()).await?.status == TaskStatus::Cancelled);
    let third = manager.submit(queued.spec(1)?)?;
    ensure!(timeout(DEADLINE, third.wait()).await?.status == TaskStatus::Succeeded);
    Ok(())
}

#[tokio::test]
async fn shutdown_cancels_running_and_queued_batches_before_returning() -> Result<()> {
    let active = Fixture::new("cancel_hang")?;
    let queued = Fixture::new("finished_then_exit")?;
    let manager = TaskManager::new(1)?;
    let first = manager.submit(active.spec(1)?)?;
    progress(&first).await?;
    let second = manager.submit(queued.spec(1)?)?;
    timeout(DEADLINE, manager.shutdown()).await?;
    ensure!(first.snapshot().status == TaskStatus::Cancelled);
    ensure!(second.snapshot().status == TaskStatus::Cancelled);
    ensure!(!queued.directory.path().join("worker.pid").exists());
    ensure!(manager.submit(queued.spec(1)?).is_err());
    Ok(())
}

#[tokio::test]
async fn dropping_a_manager_cancels_but_does_not_abort_worker_cleanup() -> Result<()> {
    let fixture = Fixture::new("cancel_hang")?;
    let manager = TaskManager::new(1)?;
    let task = manager.submit(fixture.spec(1)?)?;
    progress(&task).await?;
    drop(manager);
    ensure!(timeout(DEADLINE, task.wait()).await?.status == TaskStatus::Cancelled);
    Ok(())
}

#[tokio::test]
async fn worker_failure_stops_the_remaining_batch_files() -> Result<()> {
    let fixture = Fixture::new("startup_crash")?;
    let manager = TaskManager::new(1)?;
    let task = manager.submit(fixture.spec(3)?)?;
    let snapshot = timeout(DEADLINE, task.wait()).await?;
    ensure!(snapshot.status == TaskStatus::Failed);
    ensure!(snapshot.completed_files == 0 && snapshot.results.is_empty());
    ensure!(snapshot.failure.is_some());
    ensure!(
        fs::read_to_string(fixture.directory.path().join("worker-starts.log"))?
            .lines()
            .count()
            == 1,
        "a failed batch must not start the remaining PDFs"
    );
    Ok(())
}

#[test]
fn an_unpolled_task_is_terminal_after_its_runtime_is_dropped() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let manager = TaskManager::new(1)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let task = {
        let _entered = runtime.enter();
        manager.submit(fixture.spec(1)?)?
    };
    ensure!(task.snapshot().status == TaskStatus::Queued);
    drop(runtime);
    ensure!(task.snapshot().status == TaskStatus::Failed);
    ensure!(!fixture.directory.path().join("worker.pid").exists());
    Ok(())
}

#[tokio::test]
async fn independent_batches_can_run_concurrently_with_separate_cancellation() -> Result<()> {
    let first_fixture = Fixture::new("cancel_hang")?;
    let second_fixture = Fixture::new("cancel_hang")?;
    let manager = TaskManager::new(2)?;
    let first = manager.submit(first_fixture.spec(1)?)?;
    let second = manager.submit(second_fixture.spec(1)?)?;
    timeout(DEADLINE, async {
        tokio::try_join!(progress(&first), progress(&second))
    })
    .await??;
    first.cancel();
    ensure!(timeout(DEADLINE, first.wait()).await?.status == TaskStatus::Cancelled);
    ensure!(!second.snapshot().status.is_terminal());
    second.cancel();
    ensure!(timeout(DEADLINE, second.wait()).await?.status == TaskStatus::Cancelled);
    Ok(())
}
