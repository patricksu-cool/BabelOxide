use anyhow::{Context, Result, bail};
use pdf2zh_cli::Invocation;
use pdf2zh_core::{CacheBackend, CacheStore};
use pdf2zh_tasks::{TaskManager, TaskStatus, task_from_settings};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
};

fn main() -> ExitCode {
    let invocation = match pdf2zh_cli::parse(std::env::args_os()) {
        Ok(invocation) => invocation,
        Err(error) => {
            let failed = error.use_stderr();
            let _ = error.print();
            return if failed {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let json_output = invocation.json;
    let result = prepare_and_run(invocation);
    match result {
        Ok(TaskStatus::Succeeded) => ExitCode::SUCCESS,
        Ok(TaskStatus::Cancelled) => ExitCode::from(130),
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            report(json_output, "error", &format!("{error:#}"));
            ExitCode::FAILURE
        }
    }
}

fn prepare_and_run(invocation: Invocation) -> Result<TaskStatus> {
    let environment: BTreeMap<String, String> = std::env::vars().collect();
    let (store, settings) = invocation.load(&environment)?;
    let basic = settings.basic()?;
    if basic.gui || (basic.input_files.is_empty() && std::env::args_os().len() == 1) {
        let executable = std::env::current_exe()?;
        let gui = executable
            .parent()
            .context("Application directory is unavailable")?
            .join(if cfg!(windows) {
                "pdf2zh-gui.exe"
            } else {
                "pdf2zh-gui"
            });
        let status = std::process::Command::new(gui)
            .args(std::env::args_os().skip(1))
            .status()
            .context("Native GUI executable is unavailable in this development build")?;
        return Ok(if status.success() {
            TaskStatus::Succeeded
        } else {
            TaskStatus::Failed
        });
    }
    if basic.warmup
        || basic.generate_offline_assets.is_some()
        || basic.restore_offline_assets.is_some()
    {
        bail!("Engine asset maintenance has not yet been connected in this migration build");
    }
    if invocation.save_config {
        store.save_user_defaults(&settings)?;
        if basic.input_files.is_empty() {
            return Ok(TaskStatus::Succeeded);
        }
    }
    let cache: Option<Arc<dyn CacheBackend>> = if settings.translation()?.ignore_cache {
        None
    } else {
        let path = invocation
            .cache_file
            .clone()
            .unwrap_or_else(|| store.app_root().join("cache/translations.v2.db"));
        let opened = (|| -> Result<CacheStore> {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)?;
            }
            Ok(CacheStore::open(path)?)
        })();
        match opened {
            Ok(cache) => Some(Arc::new(cache)),
            Err(_) => {
                eprintln!("Translation cache is unavailable; continuing without it");
                None
            }
        }
    };
    let spec = task_from_settings(&settings, invocation.worker.clone(), cache)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(spec, invocation.json))
}

async fn run(spec: pdf2zh_tasks::TaskSpec, json_output: bool) -> Result<TaskStatus> {
    let manager = TaskManager::new(1)?;
    let task = manager.submit(spec)?;
    let interrupt_task = task.clone();
    let interrupt = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt_task.cancel();
        }
    });
    let mut changes = task.subscribe();
    let mut last_stage = None;
    let mut last_progress = None;
    let observed: Result<_> = async {
        loop {
            let snapshot = changes.borrow_and_update().clone();
            if let Some(event) = &snapshot.progress {
                let stage = event
                    .get("stage")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if json_output && last_progress.as_ref() != Some(event) {
                    write_json(event)?;
                    last_progress = Some(event.clone());
                }
                if !json_output && stage != last_stage {
                    if let Some(stage) = &stage {
                        eprintln!("{stage}");
                    }
                    last_stage = stage;
                }
            }
            if snapshot.status.is_terminal() {
                break Ok(snapshot);
            }
            changes
                .changed()
                .await
                .context("Translation task updates closed")?;
        }
    }
    .await;
    interrupt.abort();
    let _ = interrupt.await;
    // Output errors must still cancel and await provider/worker/cache cleanup.
    manager.shutdown().await;
    let snapshot = observed?;
    match snapshot.status {
        TaskStatus::Succeeded => {
            if json_output {
                if snapshot.results.len() == 1 {
                    write_json(&json!({"type":"finish","result":snapshot.results[0]}))?;
                } else {
                    write_json(&json!({"type":"finish","results":snapshot.results}))?;
                }
            } else {
                for result in &snapshot.results {
                    for key in [
                        "mono_pdf_path",
                        "dual_pdf_path",
                        "auto_extracted_glossary_path",
                    ] {
                        if let Some(path) = result[key].as_str() {
                            println!("{}", PathBuf::from(path).display());
                        }
                    }
                }
            }
        }
        TaskStatus::Cancelled => report(json_output, "cancelled", "Translation cancelled"),
        TaskStatus::Failed => report(
            json_output,
            "error",
            snapshot
                .failure
                .as_ref()
                .map(|failure| failure.message.as_str())
                .unwrap_or("Translation failed"),
        ),
        _ => bail!("Translation task returned without a terminal state"),
    }
    Ok(snapshot.status)
}

fn write_json(value: &Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value).context("Write progress JSON")?;
    writeln!(stdout).context("Write progress JSON newline")?;
    stdout.flush().context("Flush progress JSON")
}

fn report(json_output: bool, kind: &str, message: &str) {
    if json_output {
        let _ = write_json(&json!({"type":kind,"message":message}));
    } else {
        eprintln!("pdf2zh: {message}");
    }
}
