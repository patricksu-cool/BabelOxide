use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use babeldoc_bridge::{WorkerOptions, translate_pdf};
use clap::Parser;
use pdf2zh_core::{PdfJob, TranslationService, TranslatorConfig};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Translate a PDF through a Rust OpenAI-compatible service and BabelDOC worker.
#[derive(Parser)]
#[command(
    name = "pdf2zh",
    version,
    about,
    long_about = "Rust/BabelDOC PDF translation CLI. This initial CLI exposes the Rust OpenAI-compatible service with provider-specific retries. Set PDF2ZH_OPENAI_API_KEY in the environment; local synthetic servers may use a synthetic key. Persistent translation cache, config migration, and CLI selection of other services are still being implemented."
)]
struct Args {
    /// A single PDF input file.
    input: PathBuf,
    /// Directory for generated PDF files.
    #[arg(long, default_value = ".")]
    output: PathBuf,
    /// Pages to translate, for example 1-3,5.
    #[arg(long)]
    pages: Option<String>,
    #[arg(long, default_value = "en")]
    lang_in: String,
    #[arg(long, default_value = "zh")]
    lang_out: String,
    /// Maximum request starts per second, enforced in Rust.
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..))]
    qps: u32,
    /// Concurrent translation callbacks; defaults to --qps.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pool_max_workers: Option<u32>,
    #[arg(long, default_value = "watermarked", value_parser = ["watermarked", "no_watermark", "both"])]
    watermark_output_mode: String,
    #[arg(long)]
    no_auto_extract_glossary: bool,
    /// Enable BabelDOC debug output and bounded worker diagnostics.
    #[arg(long)]
    debug: bool,
    /// Accepted for protocol compatibility; this PoC has no translation cache.
    #[arg(long)]
    ignore_cache: bool,
    /// Select the OpenAI-compatible provider, the only provider in this PoC.
    #[arg(long)]
    openai: bool,
    #[arg(long, default_value = "https://api.openai.com/v1")]
    openai_base_url: String,
    #[arg(long, default_value = "gpt-4o-mini")]
    openai_model: String,
    /// Explicit HTTP timeout in seconds; default: read 600 seconds, connect 5 seconds.
    #[arg(long)]
    openai_timeout: Option<f64>,
    /// Enable JSON responses when BabelDOC explicitly requests JSON mode.
    #[arg(long)]
    openai_enable_json_mode: bool,
    /// Worker Python executable; required for source-tree development.
    #[arg(long)]
    python: Option<PathBuf>,
    /// Worker script; required for source-tree development.
    #[arg(long)]
    worker: Option<PathBuf>,
    /// BabelDOC asset/cache home directory.
    #[arg(long)]
    worker_home: Option<PathBuf>,
    /// Write newline-delimited progress/result JSON to stdout.
    #[arg(long)]
    json: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let failed = error.use_stderr();
            let _ = error.print();
            return if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let json_output = args.json;
    let cancel = CancellationToken::new();
    let interrupt = cancel.clone();
    let signal_task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt.cancel();
        }
    });
    let result = run(args, cancel.clone()).await;
    signal_task.abort();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) if cancel.is_cancelled() => {
            report_failure(json_output, "cancelled", "translation cancelled");
            ExitCode::from(130)
        }
        Err(error) => {
            report_failure(json_output, "error", &format!("{error:#}"));
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args, cancel: CancellationToken) -> Result<()> {
    if args.python.is_some() != args.worker.is_some() {
        bail!("source-tree development requires both --python and --worker");
    }
    let api_key = std::env::var("PDF2ZH_OPENAI_API_KEY").map_err(|_| {
        anyhow!("set PDF2ZH_OPENAI_API_KEY; use a synthetic key for local test servers")
    })?;
    if !args.input.is_file() {
        bail!("the input PDF does not exist or is not a file");
    }
    let input_file = args
        .input
        .canonicalize()
        .map_err(|_| anyhow!("could not resolve the input PDF path"))?;
    std::fs::create_dir_all(&args.output)
        .map_err(|_| anyhow!("could not create the output directory"))?;
    let output_dir = args
        .output
        .canonicalize()
        .map_err(|_| anyhow!("could not resolve the output directory"))?;
    let pool_max_workers = args.pool_max_workers.unwrap_or(args.qps);
    let translator = Arc::new(TranslationService::new(TranslatorConfig {
        provider: "openai".into(),
        endpoint: args.openai_base_url,
        api_key,
        model: args.openai_model,
        timeout_seconds: args.openai_timeout.unwrap_or(600.0),
        lang_out: args.lang_out.clone(),
        options: json!({
            "qps": args.qps,
            "timeout_explicit": args.openai_timeout.is_some(),
            "enable_json_mode": args.openai_enable_json_mode,
            "ignore_cache": args.ignore_cache,
        }),
    })?);
    let mut worker = WorkerOptions::default();
    if let Some(python) = args.python {
        worker.python = python;
    }
    if let Some(script) = args.worker {
        worker.script = script;
    }
    if let Some(home) = args.worker_home {
        worker.home = Some(home);
    }
    // The Python application explicitly enables this; BabelDOC's own default differs.
    let mut extra = BTreeMap::from([("remove_non_formula_lines".into(), json!(true))]);
    if args.debug {
        extra.insert("debug".into(), json!(true));
    }
    let job = PdfJob {
        input_file,
        output_dir,
        lang_in: args.lang_in,
        lang_out: args.lang_out,
        pages: args.pages,
        qps: args.qps,
        pool_max_workers,
        no_auto_extract_glossary: args.no_auto_extract_glossary,
        watermark_output_mode: args.watermark_output_mode,
        extra,
    };
    let (events_tx, mut events_rx) = mpsc::channel::<Value>(64);
    let json_output = args.json;
    let event_task = tokio::spawn(async move {
        let mut previous_stage = None;
        while let Some(event) = events_rx.recv().await {
            // The final result is emitted below exactly once.
            if matches!(
                event.get("type").and_then(Value::as_str),
                Some("finish" | "finished")
            ) {
                continue;
            }
            if json_output {
                write_json(&event)?;
            } else if matches!(
                event.get("type").and_then(Value::as_str),
                Some("progress" | "progress_start" | "progress_update" | "progress_end")
            ) && let Some(stage) = event
                .get("event")
                .unwrap_or(&event)
                .get("stage")
                .and_then(Value::as_str)
                && previous_stage.as_deref() != Some(stage)
            {
                eprintln!("{stage}");
                previous_stage = Some(stage.to_owned());
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    let result = translate_pdf(&worker, job, translator, cancel, Some(events_tx)).await;
    event_task
        .await
        .map_err(|_| anyhow!("progress reporting task failed"))??;
    let result = result?;
    if json_output {
        write_json(&json!({"type":"finish", "result":result}))?;
    } else {
        for key in ["mono_pdf_path", "dual_pdf_path"] {
            if let Some(path) = result.get(key).and_then(Value::as_str) {
                println!("{path}");
            }
        }
    }
    Ok(())
}

fn write_json(value: &Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)
        .map_err(|_| anyhow!("could not write progress JSON"))?;
    writeln!(stdout).map_err(|_| anyhow!("could not write progress JSON"))?;
    stdout
        .flush()
        .map_err(|_| anyhow!("could not flush progress JSON"))?;
    Ok(())
}

fn report_failure(json_output: bool, kind: &str, message: &str) {
    if json_output {
        let _ = write_json(&json!({"type":kind,"message":message}));
    } else {
        eprintln!("pdf2zh: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_timeout_remains_distinct_from_an_explicit_float() {
        let implicit = Args::try_parse_from(["pdf2zh", "input.pdf"]).unwrap();
        assert_eq!(implicit.openai_timeout, None);
        let explicit =
            Args::try_parse_from(["pdf2zh", "input.pdf", "--openai-timeout", "1.5"]).unwrap();
        assert_eq!(explicit.openai_timeout, Some(1.5));
    }
}
