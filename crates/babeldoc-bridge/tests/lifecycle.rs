//! Real child processes exercise the IPC lifecycle without importing BabelDOC.
//! Translation fixtures use a loopback HTTP server or a controlled native CLI.

use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use babeldoc_bridge::{WorkerOptions, translate_pdf, translate_pdf_with_services};
use pdf2zh_core::{PdfJob, TranslationService, TranslatorConfig};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Barrier, mpsc},
    task::{JoinHandle, JoinSet},
    time::timeout,
};
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
async fn unread_bounded_progress_does_not_block_a_terminal_result() -> Result<()> {
    let fixture = Fixture::new("progress_flood")?;
    let (events, mut received) = mpsc::channel(2);
    let result = timeout(
        DEADLINE,
        translate_pdf(
            &fixture.worker,
            fixture.job.clone(),
            fixture.service()?,
            CancellationToken::new(),
            Some(events),
        ),
    )
    .await
    .context("progress backpressure blocked the worker")??;
    ensure!(result["mono_pdf_path"].is_string());
    ensure!(received.len() == 2, "progress must remain bounded");
    ensure!(received.recv().await.is_some());
    Ok(())
}

async fn read_http_body(stream: &mut TcpStream) -> Result<Value> {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut byte = [0; 1];
        stream.read_exact(&mut byte).await?;
        bytes.push(byte[0]);
        ensure!(bytes.len() <= 64 * 1024, "fixture request too large");
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let headers = std::str::from_utf8(&bytes)?;
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .context("fixture request has no content-length")?;
    ensure!(length <= 64 * 1024);
    bytes.resize(header_end + length, 0);
    stream.read_exact(&mut bytes[header_end..]).await?;
    Ok(serde_json::from_slice(&bytes[header_end..])?)
}

/// All requests can be gated together to prove >128 RPCs remain in flight.
async fn model_server(
    expected: usize,
    gate_all: bool,
) -> Result<(String, JoinHandle<Result<Vec<Value>>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/v1", listener.local_addr()?);
    let barrier = Arc::new(Barrier::new(if gate_all { expected } else { 1 }));
    let server = tokio::spawn(async move {
        let mut requests = JoinSet::new();
        for _ in 0..expected {
            let (mut stream, _) = listener.accept().await?;
            let barrier = barrier.clone();
            requests.spawn(async move {
                let request = read_http_body(&mut stream).await?;
                barrier.wait().await;
                let body = serde_json::to_vec(&json!({
                    "choices":[{"message":{"content":request["model"]}}],
                    "usage":{"total_tokens":5,"prompt_tokens":3,"completion_tokens":2,
                        "prompt_tokens_details":{"cached_tokens":1}},
                }))?;
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
                stream.write_all(&body).await?;
                Ok::<_, anyhow::Error>(request)
            });
        }
        let mut result = Vec::new();
        while let Some(request) = requests.join_next().await {
            result.push(request??);
        }
        Ok(result)
    });
    Ok((endpoint, server))
}

fn model_service(endpoint: &str, model: &str) -> Result<Arc<TranslationService>> {
    Ok(Arc::new(TranslationService::new(TranslatorConfig {
        provider: "openai".into(),
        endpoint: endpoint.into(),
        api_key: "synthetic-loopback-key".into(),
        model: model.into(),
        timeout_seconds: 8.0,
        lang_out: "zh-TW".into(),
        options: json!({"qps": 100_000}),
    })?))
}

#[tokio::test]
async fn main_and_term_requests_use_separate_services_and_usage() -> Result<()> {
    let mut fixture = Fixture::new("translation_routing")?;
    fixture.job.qps = 100_000;
    fixture.job.extra.insert("term_qps".into(), json!(17));
    fixture
        .job
        .extra
        .insert("term_pool_max_workers".into(), json!(7));
    let (endpoint, server) = model_server(2, false).await?;
    let result = timeout(
        DEADLINE,
        translate_pdf_with_services(
            &fixture.worker,
            fixture.job.clone(),
            model_service(&endpoint, "main-model")?,
            Some(model_service(&endpoint, "term-model")?),
            CancellationToken::new(),
            None,
        ),
    )
    .await??;
    ensure!(result["translations"]["w-0"] == "main-model");
    ensure!(result["translations"]["w-1"] == "term-model");
    ensure!(
        result["received_job"].get("term_qps").is_none(),
        "Rust-only limiter option leaked into engine config"
    );
    ensure!(result["received_job"]["term_pool_max_workers"] == 7);
    let metrics = &result["translation_metrics"];
    ensure!(metrics["shared_service"] == false);
    for role in ["main", "term"] {
        ensure!(metrics[role]["calls"] == 1);
        ensure!(metrics[role]["http_attempts"] == 1);
        ensure!(metrics[role]["total_tokens"] == 5);
    }
    ensure!(timeout(DEADLINE, server).await???.len() == 2);
    Ok(())
}

#[tokio::test]
async fn shared_service_usage_is_reported_once() -> Result<()> {
    let mut fixture = Fixture::new("translation_shared")?;
    fixture.job.qps = 100_000;
    let (endpoint, server) = model_server(2, false).await?;
    let service = model_service(&endpoint, "shared-model")?;
    let result = timeout(
        DEADLINE,
        translate_pdf_with_services(
            &fixture.worker,
            fixture.job.clone(),
            service.clone(),
            Some(service),
            CancellationToken::new(),
            None,
        ),
    )
    .await??;
    let metrics = &result["translation_metrics"];
    ensure!(metrics["shared_service"] == true);
    ensure!(metrics["term"].is_null());
    ensure!(metrics["main"]["calls"] == 2);
    ensure!(metrics["main"]["total_tokens"] == 10);
    ensure!(timeout(DEADLINE, server).await???.len() == 2);
    Ok(())
}

#[tokio::test]
async fn configured_large_pool_supports_more_than_128_outstanding_requests() -> Result<()> {
    let mut fixture = Fixture::new("request_capacity")?;
    fixture.job.qps = 100_000;
    fixture.job.pool_max_workers = 200;
    let (endpoint, server) = model_server(200, true).await?;
    let result = timeout(
        DEADLINE,
        translate_pdf(
            &fixture.worker,
            fixture.job.clone(),
            model_service(&endpoint, "capacity-model")?,
            CancellationToken::new(),
            None,
        ),
    )
    .await
    .context("large pool failed to admit all gated requests")??;
    ensure!(result["received_capacity"] == 400);
    ensure!(result["translation_metrics"]["main"]["peak_inflight"] == 200);
    ensure!(timeout(DEADLINE, server).await???.len() == 200);
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
    let (events, mut received) = mpsc::channel(64);
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

        pub fn assert_exited_now(&self) -> Result<()> {
            ensure!(
                !self.running()?,
                "process was still active when the bridge returned"
            );
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
    let (events, mut received) = mpsc::channel(64);
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
    let (events, mut received) = mpsc::channel(64);
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

#[cfg(windows)]
#[tokio::test]
async fn cancelling_a_cli_request_waits_for_native_process_tree_cleanup() -> Result<()> {
    let mut fixture = Fixture::new("cli_pending")?;
    // This timeout must not abort CLI providers while they kill and reap.
    fixture.worker.shutdown_timeout = Duration::ZERO;
    let source = fixture.directory.path().join("offline_cli.rs");
    let executable = fixture.directory.path().join("offline_cli.exe");
    fs::write(
        &source,
        include_str!("../../core/src/providers/legacy/fixtures/offline_cli.rs"),
    )?;
    let mut compiler = std::process::Command::new("rustc");
    compiler.args(["--edition=2024", "--crate-name", "bridge_cli_fixture"]);
    #[cfg(target_env = "gnu")]
    compiler.args([
        "-C",
        "linker=rust-lld",
        "-C",
        "link-self-contained=yes",
        "-C",
        "linker-flavor=ld.lld",
    ]);
    let compiled = compiler.arg(source).arg("-o").arg(&executable).output()?;
    ensure!(
        compiled.status.success(),
        "native fixture compile failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let command = format!(
        "'{}' tree",
        executable.to_string_lossy().replace('\'', "'\"'\"'")
    );
    let service = Arc::new(TranslationService::new(TranslatorConfig {
        provider: "CLI-Translator".into(),
        endpoint: String::new(),
        api_key: String::new(),
        model: String::new(),
        timeout_seconds: 60.0,
        lang_out: "zh-TW".into(),
        options: json!({"qps":1,"clitranslator_command":command,"clitranslator_timeout":60.0}),
    })?);
    ensure!(
        service.requires_reap(),
        "aliases must preserve process cleanup ownership"
    );
    let cancel = CancellationToken::new();
    let operation = translate_pdf(
        &fixture.worker,
        fixture.job.clone(),
        service,
        cancel.clone(),
        None,
    );
    tokio::pin!(operation);
    timeout(DEADLINE, async {
        loop {
            if fixture.directory.path().join("descendant.pid").is_file() { break; }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(10)) => {},
                result = &mut operation => bail!("bridge ended before native fixture was ready: {result:?}"),
            }
        }
        Ok::<_, anyhow::Error>(())
    }).await.context("native process did not become ready")??;
    let child = windows_process::ObservedProcess::from_pid_file(
        fixture.directory.path().join("child.pid"),
    )?;
    let descendant = windows_process::ObservedProcess::from_pid_file(
        fixture.directory.path().join("descendant.pid"),
    )?;
    cancel.cancel();
    let error = timeout(DEADLINE, &mut operation)
        .await
        .context("native cleanup hung")?
        .expect_err("cancel must fail the task");
    ensure!(format!("{error:#}").contains("cancelled"));
    child.assert_exited_now()?;
    descendant.assert_exited_now()?;
    Ok(())
}
