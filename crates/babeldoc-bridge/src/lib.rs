//! Private, bounded, bidirectional stdio transport for the BabelDOC engine.

mod assets;
pub use assets::maintain_assets;

use anyhow::{Context, Result, bail, ensure};
use pdf2zh_core::{PdfJob, TranslationRequest, TranslationService, process};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
    task::JoinSet,
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_OUTSTANDING: usize = 128;

/// Engine resource operations do not construct a PDF job or a translator.
#[derive(Clone, Debug)]
pub enum AssetOperation {
    Warmup,
    GenerateOfflineAssets { output_dir: PathBuf },
    RestoreOfflineAssets { input: PathBuf },
}

struct SessionServices {
    main: Arc<TranslationService>,
    term: Option<Arc<TranslationService>>,
}

struct SessionPlan {
    start: Value,
    services: Option<SessionServices>,
    temporary_root: PathBuf,
    diagnostics: bool,
    capacity: usize,
    operation: &'static str,
}

#[derive(Clone, Debug)]
pub struct WorkerOptions {
    pub python: PathBuf,
    pub script: PathBuf,
    pub home: Option<PathBuf>,
    pub startup_timeout: Duration,
    pub idle_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for WorkerOptions {
    fn default() -> Self {
        let root = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_default();
        Self {
            python: root.join(if cfg!(windows) {
                "runtime/python/python.exe"
            } else {
                "runtime/python/bin/python3"
            }),
            script: root.join("babeldoc/worker.py"),
            home: Some(root.join("resources/worker-home")),
            startup_timeout: Duration::from_secs(180),
            idle_timeout: Duration::from_secs(1800),
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}

async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    bytes: &mut Vec<u8>,
) -> Result<Option<Value>> {
    loop {
        let buffer = reader.fill_buf().await.context("Read worker pipe")?;
        if buffer.is_empty() {
            ensure!(bytes.is_empty(), "Worker closed an incomplete frame");
            return Ok(None);
        }
        let end = buffer.iter().position(|b| *b == b'\n').map(|p| p + 1);
        let count = end.unwrap_or(buffer.len());
        ensure!(
            bytes.len() + count <= MAX_FRAME_BYTES,
            "Worker frame exceeds limit"
        );
        bytes.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            let raw = std::mem::take(bytes);
            let value: Value =
                serde_json::from_slice(&raw).map_err(|_| anyhow::anyhow!("Invalid worker JSON"))?;
            ensure!(
                value.is_object() && value["v"] == 1,
                "Unsupported worker protocol"
            );
            return Ok(Some(value));
        }
    }
}

fn frame(kind: &str, session: &str, fields: Value) -> Value {
    let mut value = fields;
    value["v"] = json!(1);
    value["type"] = json!(kind);
    value["session"] = json!(session);
    value["task_id"] = json!("job-1");
    value
}

async fn enqueue(tx: &mpsc::Sender<Value>, value: Value) -> Result<()> {
    timeout(Duration::from_secs(5), tx.send(value))
        .await
        .context("Worker write queue timed out")?
        .context("Worker writer closed")
}

pub async fn translate_pdf(
    worker: &WorkerOptions,
    job: PdfJob,
    translator: Arc<TranslationService>,
    cancel: CancellationToken,
    events: Option<mpsc::Sender<Value>>,
) -> Result<Value> {
    translate_pdf_with_services(worker, job, translator, None, cancel, events).await
}

/// Main and term services may be shared or independently configured in Rust.
pub async fn translate_pdf_with_services(
    worker: &WorkerOptions,
    mut job: PdfJob,
    translator: Arc<TranslationService>,
    term_translator: Option<Arc<TranslationService>>,
    cancel: CancellationToken,
    events: Option<mpsc::Sender<Value>>,
) -> Result<Value> {
    ensure!(
        job.qps > 0 && job.pool_max_workers > 0,
        "QPS and worker pool must be positive"
    );
    ensure!(!cancel.is_cancelled(), "Translation cancelled");
    job.input_file = job
        .input_file
        .canonicalize()
        .context("Input PDF is unavailable")?;
    ensure!(job.input_file.is_file(), "Input PDF must be a file");
    std::fs::create_dir_all(&job.output_dir).context("Create output directory")?;
    job.output_dir = job
        .output_dir
        .canonicalize()
        .context("Resolve output directory")?;
    let output_root = job.output_dir.clone();
    if let Some(glossaries) = job.extra.get_mut("glossary_files") {
        let paths = glossaries
            .as_array_mut()
            .context("Glossary files must be a list")?;
        for path in paths {
            let resolved = PathBuf::from(path.as_str().context("Glossary path must be text")?)
                .canonicalize()
                .context("Glossary file is unavailable")?;
            ensure!(resolved.is_file(), "Glossary must be a file");
            *path = json!(resolved);
        }
    }
    if let Some(size) = job.extra.get("max_pages_per_part") {
        ensure!(
            size.as_u64().is_some_and(|size| size > 0),
            "Page split size must be positive"
        );
    }
    let require_mono = !job
        .extra
        .get("no_mono")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let require_dual = !job
        .extra
        .get("no_dual")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ensure!(
        require_mono || require_dual,
        "At least one PDF output must be enabled"
    );
    // Preflight can probe a service or validate an external executable. Await it
    // cooperatively so cancellation cannot discard its process cleanup future.
    translator.prepare(&cancel).await?;
    let (recommended_qps, recommended_pool) = translator.recommendations();
    job.qps = recommended_qps.unwrap_or(job.qps);
    job.pool_max_workers = recommended_pool.unwrap_or(job.pool_max_workers);
    // Free-service fallback recommendations only change the engine pool/QPS;
    // its existing limiter changes only after a valid server configuration.
    if recommended_qps.is_none() {
        translator.set_max_qps(job.qps).await?;
    }
    let term_qps = job.extra.remove("term_qps");
    let term_qps = match term_qps.filter(|value| !value.is_null()) {
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .context("Term QPS must be a positive 32-bit integer")?,
        None => job.qps,
    };
    let separate_term = term_translator
        .as_ref()
        .filter(|term| !Arc::ptr_eq(&translator, term));
    if let Some(term) = separate_term {
        term.prepare(&cancel).await?;
        let (qps, workers) = term.recommendations();
        if qps.is_none() {
            term.set_max_qps(term_qps).await?;
        }
        if let Some(workers) = workers {
            job.extra
                .insert("term_pool_max_workers".into(), json!(workers));
        }
    } else {
        ensure!(
            term_qps == job.qps,
            "Independent term QPS needs a separate translation service"
        );
        if let Some(workers) = recommended_pool {
            job.extra
                .insert("term_pool_max_workers".into(), json!(workers));
        }
    }
    let term_pool = match job.extra.get("term_pool_max_workers") {
        None | Some(Value::Null) => u64::from(job.pool_max_workers),
        Some(value) => value
            .as_u64()
            .context("Term worker pool must be a nonnegative integer")?,
    };
    let term_pool = if term_pool == 0 {
        u64::from(job.pool_max_workers)
    } else {
        term_pool
    };
    job.extra
        .insert("term_pool_max_workers".into(), json!(term_pool));
    let capacity = u64::from(job.pool_max_workers)
        .saturating_add(term_pool)
        .clamp(MAX_OUTSTANDING as u64, 1_000_000) as usize;
    let term_metadata = term_translator
        .as_ref()
        .map(|term| json!({"name":term.name(),"llm":term.is_llm()}));
    let diagnostics = job
        .extra
        .get("debug")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut result = run_session(worker, SessionPlan {
        start: json!({"operation":"pdf", "job":job, "translator":{"name":translator.name(),"llm":translator.is_llm()}, "term_translator":term_metadata, "max_outstanding":capacity}),
        services: Some(SessionServices { main:translator.clone(), term:term_translator.clone() }),
        temporary_root: output_root.clone(), diagnostics, capacity, operation:"pdf",
    }, cancel, events).await?;
    validate_pdf_result(&result, &output_root, require_mono, require_dual)?;
    // Service-lifetime counters; shared main/term instances appear only once.
    result["translation_metrics"] = json!({
        "main": translator.metrics(),
        "term": separate_term.map(|term| term.metrics()),
        "shared_service": separate_term.is_none(),
    });
    Ok(result)
}

fn validate_pdf_result(
    result: &Value,
    output_root: &Path,
    require_mono: bool,
    require_dual: bool,
) -> Result<()> {
    let mut output_count = 0;
    for key in [
        "mono_pdf_path",
        "dual_pdf_path",
        "no_watermark_mono_pdf_path",
        "no_watermark_dual_pdf_path",
    ] {
        if let Some(path) = result[key].as_str() {
            let path = PathBuf::from(path)
                .canonicalize()
                .context("Worker output does not exist")?;
            ensure!(
                path.is_file() && path.starts_with(output_root),
                "Worker output escaped the requested directory"
            );
            output_count += 1;
        }
    }
    ensure!(output_count > 0, "Worker returned no PDF output");
    ensure!(
        !require_mono
            || result["mono_pdf_path"].is_string()
            || result["no_watermark_mono_pdf_path"].is_string(),
        "Worker omitted the requested monolingual PDF"
    );
    ensure!(
        !require_dual
            || result["dual_pdf_path"].is_string()
            || result["no_watermark_dual_pdf_path"].is_string(),
        "Worker omitted the requested bilingual PDF"
    );
    if let Some(path) = result["auto_extracted_glossary_path"].as_str() {
        let path = PathBuf::from(path)
            .canonicalize()
            .context("Worker glossary does not exist")?;
        ensure!(
            path.is_file() && path.starts_with(output_root),
            "Worker glossary escaped the requested directory"
        );
    }
    Ok(())
}

async fn run_session(
    worker: &WorkerOptions,
    plan: SessionPlan,
    cancel: CancellationToken,
    events: Option<mpsc::Sender<Value>>,
) -> Result<Value> {
    let python = worker
        .python
        .canonicalize()
        .context("Worker Python is unavailable; provide --python for development")?;
    let script = worker
        .script
        .canonicalize()
        .context("Worker adapter is unavailable")?;
    let capacity = plan.capacity;
    let cooperative_cleanup = plan.services.as_ref().is_some_and(|services| {
        services.main.requires_cooperative_cleanup()
            || services
                .term
                .as_ref()
                .is_some_and(|term| term.requires_cooperative_cleanup())
    });
    ensure!(!cancel.is_cancelled(), "Translation cancelled");
    let temporary =
        tempfile::tempdir_in(&plan.temporary_root).context("Create engine temporary directory")?;
    let home = worker
        .home
        .clone()
        .unwrap_or_else(|| plan.temporary_root.join("worker-home"));
    std::fs::create_dir_all(&home).context("Create worker resource directory")?;
    let home = home.canonicalize()?;
    let mut command = Command::new(python);
    command
        .args(["-I", "-u"])
        .arg(&script)
        .current_dir(script.parent().context("Worker script parent")?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("TEMP", temporary.path())
        .env("TMP", temporary.path())
        .env("PYTHONIOENCODING", "utf-8")
        .kill_on_drop(true);
    for name in ["SystemRoot", "WINDIR", "COMSPEC"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("PATH", PathBuf::from(root).join("System32"));
    }
    process::prepare(&mut command);
    let mut child = command.spawn().context("Start BabelDOC worker")?;
    let scope = match process::contain_and_resume(&mut child) {
        Ok(scope) => scope,
        Err(error) => {
            let _ = child.kill().await;
            return Err(error);
        }
    };
    let mut stdin = child.stdin.take().context("Capture worker stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().context("Capture worker stdout")?);
    // Kept outside read futures so a select! cancellation cannot discard a partial frame.
    let mut frame_buffer = Vec::new();
    let mut stderr = child.stderr.take().context("Capture worker stderr")?;
    // Diagnostics are opt-in and bounded. Credentials never enter this process.
    let debug_log = plan
        .diagnostics
        .then(|| plan.temporary_root.join("worker-diagnostics.log"));
    let mut diagnostics = tokio::spawn(async move {
        if let Some(path) = debug_log {
            let mut file = tokio::fs::File::create(path).await?;
            tokio::io::copy(&mut (&mut stderr).take(16 * 1024 * 1024), &mut file).await?;
            file.flush().await?;
        }
        tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await
    });
    let (tx, mut rx) = mpsc::channel::<Value>(capacity + 4);
    let mut writer = tokio::spawn(async move {
        while let Some(value) = rx.recv().await {
            let mut raw = serde_json::to_vec(&value)?;
            raw.push(b'\n');
            ensure!(raw.len() <= MAX_FRAME_BYTES, "Host frame exceeds limit");
            stdin.write_all(&raw).await.context("Write worker pipe")?;
            stdin.flush().await?;
        }
        Ok::<(), anyhow::Error>(())
    });
    let requests_cancel = cancel.child_token();
    let mut requests: JoinSet<(String, Result<String>)> = JoinSet::new();
    let mut session = String::new();
    let mut writer_finished = false;
    let outcome: Result<Value> = async {
        let hello = tokio::select! {
            _ = cancel.cancelled() => bail!("Translation cancelled"),
            _ = &mut writer => { writer_finished = true; bail!("Worker writer failed during startup"); },
            _ = child.wait() => bail!("Worker exited during startup"),
            hello = timeout(worker.startup_timeout, read_frame(&mut stdout, &mut frame_buffer)) => hello.context("Worker startup timed out")??.context("Worker exited before ready")?,
        };
        ensure!(hello["type"] == "ready" && hello["babeldoc_version"] == "0.6.3", "Worker initialization failed or engine version differs");
        session = hello["session"].as_str().context("Missing worker session")?.to_owned();
        ensure!(session.len() == 32 && session.bytes().all(|b| b.is_ascii_hexdigit()), "Invalid worker session");
        if plan.operation != "pdf" {
            ensure!(hello["operations"].as_array().is_some_and(|operations| operations.iter().any(|operation| operation == plan.operation)), "Worker does not support the requested resource operation");
        }
        enqueue(&tx, frame("start", &session, plan.start)).await?;
        let mut active = HashSet::new();
        let mut seen = HashSet::new();
        let mut activity = Instant::now();
        // Resource APIs have no byte-progress callback. Their timeout is an
        // absolute operation deadline; incidental progress cannot renew it.
        let operation_deadline = (plan.operation != "pdf").then(|| activity + worker.idle_timeout);
        let mut exit_deadline = None;
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => bail!("Translation cancelled"),
                _ = tokio::time::sleep_until(exit_deadline.unwrap_or(operation_deadline.unwrap_or(activity + worker.idle_timeout))) => bail!("Worker idle timeout or exit without terminal result"),
                _ = &mut writer => { writer_finished = true; bail!("Worker writer failed"); },
                status = child.wait(), if exit_deadline.is_none() => {
                    ensure!(status.context("Observe worker exit")?.success(), "Worker crashed");
                    exit_deadline = Some(Instant::now() + Duration::from_secs(1));
                }
                done = requests.join_next(), if !requests.is_empty() => {
                    let (id, translated) = done.context("Missing translation task")?.context("Translation task panicked")?;
                    active.remove(&id);
                    match translated {
                        Ok(text) => enqueue(&tx, frame("translated", &session, json!({"id":id,"text":text}))).await?,
                        Err(error) => {
                            let _ = enqueue(&tx, frame("translated", &session, json!({"id":id,"error":{"code":"translation_failed","message":"Rust translation service failed"}}))).await;
                            return Err(error.context("Rust translation service failed; output is incomplete"));
                        }
                    }
                }
                message = read_frame(&mut stdout, &mut frame_buffer) => {
                    let message = message?.context("Worker exited without a terminal result")?;
                    ensure!(message["session"] == session && message["task_id"] == "job-1", "Worker session or task mismatch");
                    activity = Instant::now();
                    match message["type"].as_str().unwrap_or("") {
                        "translate" => {
                            let services = plan.services.as_ref().context("Resource worker requested a translation")?;
                            let request: TranslationRequest = serde_json::from_value(message).map_err(|_| anyhow::anyhow!("Malformed translation request"))?;
                            ensure!(seen.len() < 1_000_000, "Task exceeded the request ID budget");
                            ensure!(request.id.len() <= 64 && !request.id.is_empty() && seen.insert(request.id.clone()), "Invalid or repeated request ID");
                            ensure!(active.len() < capacity, "Worker exceeded request capacity");
                            active.insert(request.id.clone());
                            let service = match request.translator.as_str() {
                                "main" => services.main.clone(),
                                "term" => services.term.clone().unwrap_or_else(|| services.main.clone()),
                                _ => bail!("Worker requested an unknown translation service"),
                            };
                            let request_cancel = requests_cancel.clone();
                            requests.spawn(async move {
                                let result = service.translate(&request, &request_cancel).await;
                                (request.id, result)
                            });
                        }
                        // Progress can be dropped under backpressure; terminal results are returned separately.
                        "progress" => if let Some(events) = &events { let _ = events.try_send(message["event"].clone()); },
                        "finished" => {
                            ensure!(active.is_empty(), "Worker finished with pending translations");
                            let result = message["result"].clone();
                            ensure!(result.is_object(), "Worker result must be an object");
                            return Ok(result);
                        }
                        "error" => bail!("BabelDOC worker failed ({})", message["code"].as_str().filter(|s| s.len() < 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).unwrap_or("engine_failed")),
                        "cancel_ack" => {},
                        _ => bail!("Unsupported worker message"),
                    }
                }
            }
        }
    }.await;
    requests_cancel.cancel();
    // Keep output flowing while cooperative cancellation reaps provider processes.
    let drain = tokio::spawn(async move {
        while read_frame(&mut stdout, &mut frame_buffer).await?.is_some() {}
        Ok::<(), anyhow::Error>(())
    });
    if !session.is_empty() && outcome.is_err() {
        let _ = enqueue(&tx, frame("cancel", &session, json!({}))).await;
    }
    let reap = async { while requests.join_next().await.is_some() {} };
    if cooperative_cleanup {
        // CLI providers reap their process tree, and cache backends wait for
        // interrupted blocking writes. Aborting would discard those owners.
        reap.await;
    } else if timeout(worker.shutdown_timeout, reap).await.is_err() {
        requests.abort_all();
        while requests.join_next().await.is_some() {}
    }
    if !session.is_empty() {
        let _ = enqueue(&tx, frame("shutdown", &session, json!({}))).await;
    }
    drop(tx);
    let exited = match timeout(worker.shutdown_timeout, child.wait()).await {
        Ok(result) => result.context("Wait for worker"),
        Err(_) => {
            let _ = child.kill().await;
            Err(anyhow::anyhow!(
                "Worker shutdown timed out; process tree terminated"
            ))
        }
    };
    let tree_cleanup = scope.terminate_and_wait().await;
    drop(scope);
    let _writer_ok = if writer_finished {
        false
    } else {
        match timeout(Duration::from_secs(2), &mut writer).await {
            Ok(Ok(Ok(()))) => true,
            Ok(_) => false,
            Err(_) => {
                writer.abort();
                let _ = writer.await;
                false
            }
        }
    };
    if timeout(Duration::from_secs(2), &mut diagnostics)
        .await
        .is_err()
    {
        diagnostics.abort();
        let _ = diagnostics.await;
    }
    drain.abort();
    let _ = drain.await;
    match outcome {
        Err(primary) => {
            if tree_cleanup.is_err() {
                return Err(primary.context("Worker process-tree cleanup also failed"));
            }
            Err(primary)
        }
        Ok(result) => {
            tree_cleanup.context("Clean up worker process tree")?;
            ensure!(exited?.success(), "Worker exited with a failure status");
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn framing_rejects_partial_and_oversized_messages() {
        for raw in [
            b"{\"v\":1}".to_vec(),
            vec![b'x'; MAX_FRAME_BYTES + 1],
            b"{\"v\":2}\n".to_vec(),
            b"[]\n".to_vec(),
        ] {
            assert!(
                read_frame(&mut BufReader::new(raw.as_slice()), &mut Vec::new())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn framing_preserves_multiple_and_unicode_frames() {
        let raw = "{\"v\":1,\"text\":\"測試\"}\n{\"v\":1}\n";
        let mut reader = BufReader::new(raw.as_bytes());
        let mut bytes = Vec::new();
        assert_eq!(
            read_frame(&mut reader, &mut bytes).await.unwrap().unwrap()["text"],
            "測試"
        );
        assert!(read_frame(&mut reader, &mut bytes).await.unwrap().is_some());
        assert!(read_frame(&mut reader, &mut bytes).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn interrupted_read_retains_the_partial_frame() {
        let (mut write, read) = tokio::io::duplex(64);
        let mut reader = BufReader::new(read);
        let mut bytes = Vec::new();
        write.write_all(b"{\"v\":1,").await.unwrap();
        assert!(
            timeout(
                Duration::from_millis(10),
                read_frame(&mut reader, &mut bytes)
            )
            .await
            .is_err()
        );
        write.write_all(b"\"type\":\"ready\"}\n").await.unwrap();
        assert_eq!(
            read_frame(&mut reader, &mut bytes).await.unwrap().unwrap()["type"],
            "ready"
        );
    }
}
