use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use babeldoc_bridge::WorkerOptions;
use pdf2zh_config::Settings;
use pdf2zh_core::{PdfJob, TranslationService, TranslatorConfig};
use pdf2zh_tasks::{
    TaskHandle, TaskManager, TaskSnapshot, TaskSpec, TaskStatus, task_from_settings,
};
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

const DEADLINE: Duration = Duration::from_secs(10);
mod http_fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-support/loopback.rs"
    ));
}
use http_fixture::LoopbackProvider;

#[test]
fn loopback_fixture_waits_for_fragmented_request() -> Result<()> {
    use std::io::{Read, Write};
    let server = LoopbackProvider::free()?;
    let address = server
        .endpoint()
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut stream = std::net::TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.write_all(b"POST /v1/ch")?;
    std::thread::sleep(Duration::from_millis(50));
    stream.write_all(
        b"eck HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    ensure!(response.starts_with("HTTP/1.1 200"), "{response}");
    ensure!(server.requests().len() == 1);
    ensure!(server.requests()[0]["_path"] == "/v1/check");
    Ok(())
}

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
            asset_manifest: None,
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
            initialization: None,
        })
    }

    fn application_settings(
        &self,
        endpoint: &str,
        count: usize,
        term_qps: Option<u32>,
    ) -> Result<Settings> {
        let inputs = (0..count)
            .map(|index| {
                let input = self
                    .directory
                    .path()
                    .join(format!("factory-input-{index}.pdf"));
                fs::write(&input, b"%PDF-1.4\n%%EOF\n")?;
                Ok(input)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut settings = Settings::default();
        for (path, value) in [
            ("openai", json!(true)),
            (
                "openai_detail.openai_api_key",
                json!("synthetic-factory-key"),
            ),
            ("openai_detail.openai_base_url", json!(endpoint)),
            ("openai_detail.openai_model", json!("factory-main")),
            ("basic.input_files", json!(inputs)),
            ("translation.output", json!(self.job.output_dir)),
            ("translation.qps", json!(200)),
            ("translation.term_qps", json!(term_qps)),
            ("translation.ignore_cache", json!(true)),
            ("translation.no_auto_extract_glossary", json!(true)),
        ] {
            settings.set(path, value)?;
        }
        Ok(settings)
    }
}

#[tokio::test]
async fn default_raw_term_qps_initializes_two_services_per_pdf_even_without_glossary() -> Result<()>
{
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::new()?;
    let settings = fixture.application_settings(server.endpoint(), 2, None)?;
    let spec = task_from_settings(&settings, fixture.worker.clone(), None)?;
    let prototype = spec.translator.clone();
    ensure!(
        server.requests().is_empty(),
        "Constructing a task must be pure"
    );
    ensure!(!fixture.directory.path().join("worker.pid").exists());
    let manager = TaskManager::new(1)?;
    let result = timeout(DEADLINE, manager.submit(spec)?.wait()).await?;
    ensure!(result.status == TaskStatus::Succeeded, "{result:?}");
    ensure!(server.requests().len() == 4);
    for request in server.requests() {
        ensure!(
            request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("Hello") && text != "Hello"))
        );
    }
    for result in result.results {
        ensure!(result["translation_metrics"]["main"]["calls"] == 1);
        ensure!(result["translation_metrics"]["main"]["admissions"] == 1);
        // Real wire connections may use the SDK's existing connect retries.
        // Exact attempt/QPS counts use the controlled executor in core tests.
        ensure!(
            result["translation_metrics"]["main"]["http_attempts"]
                .as_u64()
                .unwrap()
                >= 1
        );
        ensure!(result["translation_metrics"]["main"]["total_tokens"] == 5);
        ensure!(result["translation_metrics"]["term"]["calls"] == 1);
        ensure!(result["translation_metrics"]["shared_service"] == false);
    }
    ensure!(
        prototype.metrics().calls == 0,
        "Per-document counters must not leak into the prototype"
    );
    Ok(())
}

#[tokio::test]
async fn explicit_equal_raw_term_qps_shares_one_health_check_per_pdf() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::new()?;
    let settings = fixture.application_settings(server.endpoint(), 2, Some(200))?;
    let manager = TaskManager::new(1)?;
    let result = timeout(
        DEADLINE,
        manager
            .submit(task_from_settings(&settings, fixture.worker.clone(), None)?)?
            .wait(),
    )
    .await?;
    ensure!(result.status == TaskStatus::Succeeded, "{result:?}");
    ensure!(server.requests().len() == 2);
    for result in result.results {
        ensure!(result["translation_metrics"]["main"]["calls"] == 1);
        ensure!(result["translation_metrics"]["term"].is_null());
        ensure!(result["translation_metrics"]["shared_service"] == true);
    }
    Ok(())
}

#[tokio::test]
async fn unequal_raw_term_qps_does_not_share_matching_engines() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::new()?;
    let settings = fixture.application_settings(server.endpoint(), 1, Some(100))?;
    let manager = TaskManager::new(1)?;
    let result = timeout(
        DEADLINE,
        manager
            .submit(task_from_settings(&settings, fixture.worker.clone(), None)?)?
            .wait(),
    )
    .await?;
    ensure!(result.status == TaskStatus::Succeeded, "{result:?}");
    ensure!(server.requests().len() == 2);
    ensure!(result.results[0]["translation_metrics"]["shared_service"] == false);
    Ok(())
}

#[tokio::test]
async fn different_term_engine_settings_prevent_sharing_even_with_equal_qps() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::new()?;
    let mut settings = fixture.application_settings(server.endpoint(), 1, Some(200))?;
    for (path, value) in [
        ("term_openai", json!(true)),
        (
            "term_openai_detail.term_openai_api_key",
            json!("synthetic-factory-key"),
        ),
        (
            "term_openai_detail.term_openai_base_url",
            json!(server.endpoint()),
        ),
        (
            "term_openai_detail.term_openai_model",
            json!("factory-term"),
        ),
    ] {
        settings.set(path, value)?;
    }
    let manager = TaskManager::new(1)?;
    let result = timeout(
        DEADLINE,
        manager
            .submit(task_from_settings(&settings, fixture.worker.clone(), None)?)?
            .wait(),
    )
    .await?;
    ensure!(result.status == TaskStatus::Succeeded, "{result:?}");
    let requests = server.requests();
    ensure!(
        requests.len() == 2
            && requests[0]["model"] == "factory-main"
            && requests[1]["model"] == "factory-term"
    );
    Ok(())
}

#[tokio::test]
async fn failed_main_health_never_calls_term_or_starts_python() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::rejecting()?;
    let settings = fixture.application_settings(server.endpoint(), 1, None)?;
    let manager = TaskManager::new(1)?;
    let result = timeout(
        DEADLINE,
        manager
            .submit(task_from_settings(&settings, fixture.worker.clone(), None)?)?
            .wait(),
    )
    .await?;
    ensure!(result.status == TaskStatus::Failed, "{result:?}");
    ensure!(
        result
            .failure
            .as_ref()
            .and_then(|f| f.provider.as_ref())
            .is_some(),
        "Typed cause must survive task failure"
    );
    ensure!(server.requests().len() == 1);
    ensure!(!fixture.directory.path().join("worker-starts.log").exists());
    Ok(())
}

#[tokio::test]
async fn cancelling_health_returns_before_python_start_or_term_initialization() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::stalled()?;
    let settings = fixture.application_settings(server.endpoint(), 1, None)?;
    let manager = TaskManager::new(1)?;
    let task = manager.submit(task_from_settings(&settings, fixture.worker.clone(), None)?)?;
    timeout(DEADLINE, async {
        while server.requests().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await?;
    task.cancel();
    let result = timeout(Duration::from_secs(2), task.wait()).await?;
    ensure!(result.status == TaskStatus::Cancelled, "{result:?}");
    ensure!(server.requests().len() == 1);
    ensure!(!fixture.directory.path().join("worker-starts.log").exists());
    Ok(())
}

#[tokio::test]
async fn invalid_input_is_rejected_before_health_or_worker_start() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::new()?;
    let settings = fixture.application_settings(server.endpoint(), 1, None)?;
    let spec = task_from_settings(&settings, fixture.worker.clone(), None)?;
    fs::remove_file(&spec.jobs[0].input_file)?;
    let manager = TaskManager::new(1)?;
    let result = timeout(DEADLINE, manager.submit(spec)?.wait()).await?;
    ensure!(result.status == TaskStatus::Failed, "{result:?}");
    ensure!(server.requests().is_empty());
    ensure!(!fixture.directory.path().join("worker-starts.log").exists());
    Ok(())
}

#[tokio::test]
async fn non_llm_without_term_has_one_ordinary_health_translation() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit")?;
    let server = LoopbackProvider::google()?;
    let mut spec = fixture.spec(1)?;
    spec.translator = Arc::new(TranslationService::new(TranslatorConfig {
        provider: "google".into(),
        endpoint: server.endpoint().into(),
        api_key: String::new(),
        model: String::new(),
        timeout_seconds: 1.0,
        lang_out: "zh".into(),
        options: json!({"qps":200}),
    })?);
    spec.initialization = Some(babeldoc_bridge::FactoryInitialization {
        matching_engine_settings: false,
    });
    let manager = TaskManager::new(1)?;
    let result = timeout(DEADLINE, manager.submit(spec)?.wait()).await?;
    ensure!(result.status == TaskStatus::Succeeded, "{result:?}");
    let requests = server.requests();
    ensure!(requests.len() == 1 && requests[0]["_method"] == "GET");
    ensure!(requests[0]["_path"].as_str().unwrap().contains("Hello"));
    ensure!(result.results[0]["translation_metrics"]["main"]["calls"] == 1);
    Ok(())
}

#[tokio::test]
async fn free_recommendations_decide_sharing_after_main_health_and_before_term_creation()
-> Result<()> {
    for term_qps in [None, Some(7), Some(20)] {
        let fixture = Fixture::new("translation_routing")?;
        let server = LoopbackProvider::free()?;
        let mut settings = fixture.application_settings(server.endpoint(), 1, term_qps)?;
        settings.set("siliconflowfree", json!(true))?;
        settings.set(
            "siliconflowfree_detail.core_endpoint",
            json!(server.endpoint()),
        )?;
        let manager = TaskManager::new(1)?;
        let task = manager.submit(task_from_settings(&settings, fixture.worker.clone(), None)?)?;
        let result = match timeout(DEADLINE, task.wait()).await {
            Ok(result) => result,
            Err(error) => {
                let paths: Vec<_> = server
                    .requests()
                    .iter()
                    .map(|request| request["_path"].as_str().unwrap_or("?").to_owned())
                    .collect();
                let worker_started = fixture.directory.path().join("worker-starts.log").exists();
                anyhow::bail!(
                    "term_qps={term_qps:?}: {error}; snapshot={:?}; paths={paths:?}; worker_started={worker_started}",
                    task.snapshot()
                );
            }
        };
        ensure!(
            result.status == TaskStatus::Succeeded,
            "term_qps={term_qps:?}: {result:?}"
        );
        let result = &result.results[0];
        let shared = term_qps == Some(7);
        ensure!(result["translation_metrics"]["shared_service"] == shared);
        ensure!(result["received_job"]["qps"] == 7);
        ensure!(result["received_job"]["pool_max_workers"] == 9);
        ensure!(result["received_job"]["term_pool_max_workers"] == 9);
        let requests = server.requests();
        ensure!(
            requests.len() == if shared { 7 } else { 12 },
            "term_qps={term_qps:?}, requests={requests:?}, result={result:?}"
        );
        ensure!(requests[0]["_path"].as_str().unwrap().ends_with("/check"));
        ensure!(requests[3]["_path"].as_str().unwrap().ends_with("/config"));
        ensure!(requests[4]["text"].as_str().unwrap().contains("Hello"));
        if !shared {
            ensure!(requests[5]["_path"].as_str().unwrap().ends_with("/check"));
            ensure!(requests[9]["text"].as_str().unwrap().contains("Hello"));
        }
    }
    Ok(())
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
