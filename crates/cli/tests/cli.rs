//! Actual CLI subprocesses with isolated settings and controlled worker/HTTP.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};
mod http_fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-support/loopback.rs"
    ));
}
use http_fixture::LoopbackProvider;

struct Fixture {
    directory: TempDir,
    app: PathBuf,
    python: PathBuf,
    script: PathBuf,
    inputs: Vec<PathBuf>,
    output: PathBuf,
}

impl Fixture {
    fn new(scenario: &str, count: usize) -> Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let directory = tempfile::tempdir()?;
        let app = directory.path().join("app");
        for name in ["home", "tmp"] {
            fs::create_dir(directory.path().join(name))?;
        }
        let script = directory.path().join("fake_worker.py");
        fs::write(
            &script,
            include_str!("../../babeldoc-bridge/tests/fixtures/fake_worker.py"),
        )?;
        fs::write(
            directory.path().join("scenario.json"),
            serde_json::to_vec(&json!({"scenario":scenario}))?,
        )?;
        let inputs = (0..count)
            .map(|index| {
                let input = directory.path().join(format!("input-{index}.pdf"));
                fs::write(&input, b"%PDF-1.4\n%%EOF\n")?;
                Ok(input)
            })
            .collect::<Result<Vec<_>>>()?;
        let output = directory.path().join("output");
        let python = std::env::var_os("BABELOXIDE_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                root.join(if cfg!(windows) {
                    ".venv/Scripts/python.exe"
                } else {
                    ".venv/bin/python"
                })
            });
        Ok(Self {
            directory,
            app,
            python,
            script,
            inputs,
            output,
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_BabelOxide"));
        command
            .env_clear()
            .current_dir(self.directory.path())
            .env("BABELOXIDE_APP_DIR", &self.app)
            .env("HOME", self.directory.path().join("home"))
            .env("USERPROFILE", self.directory.path().join("home"))
            .env("TEMP", self.directory.path().join("tmp"))
            .env("TMP", self.directory.path().join("tmp"))
            .env("NO_PROXY", "127.0.0.1,localhost");
        for name in ["SystemRoot", "WINDIR", "COMSPEC"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
    }

    fn translation(&self) -> Command {
        let mut command = self.command();
        command
            .args(&self.inputs)
            .arg("--output")
            .arg(&self.output)
            .arg("--python")
            .arg(&self.python)
            .arg("--worker")
            .arg(&self.script)
            .arg("--worker-home")
            .arg(self.directory.path().join("assets"))
            .args(["--ignore-cache", "--no-auto-extract-glossary", "--json"]);
        command
    }
}

fn events(output: &Output) -> Result<Vec<Value>> {
    String::from_utf8(output.stdout.clone())?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(anyhow::Error::new))
        .collect()
}

#[test]
fn resource_cli_operations_need_no_pdf_or_provider_credentials() -> Result<()> {
    for flag in [
        "--warmup",
        "--generate-offline-assets",
        "--restore-offline-assets",
    ] {
        let fixture = Fixture::new("asset_success", 0)?;
        let mut command = fixture.command();
        command.arg(flag);
        if flag == "--generate-offline-assets" {
            command.arg(&fixture.output);
        }
        if flag == "--restore-offline-assets" {
            let archive = fixture.directory.path().join("offline.zip");
            fs::write(&archive, b"controlled restore fixture")?;
            command.arg(archive);
        }
        let output = command
            .arg("--python")
            .arg(&fixture.python)
            .arg("--worker")
            .arg(&fixture.script)
            .arg("--worker-home")
            .arg(fixture.directory.path().join("assets"))
            .arg("--json")
            .output()?;
        ensure!(
            output.status.success(),
            "{flag}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let terminal = events(&output)?;
        ensure!(
            terminal
                .iter()
                .filter(|event| event["type"] == "finish")
                .count()
                == 1
        );
        ensure!(!fixture.app.join("config/config.v3.toml").exists());
    }
    Ok(())
}

#[test]
fn conflicting_asset_operations_fail_before_worker_starts() -> Result<()> {
    let fixture = Fixture::new("asset_success", 0)?;
    let output = fixture
        .command()
        .arg("--generate-offline-assets")
        .arg(&fixture.output)
        .arg("--restore-offline-assets")
        .arg(fixture.directory.path())
        .arg("--python")
        .arg(&fixture.python)
        .arg("--worker")
        .arg(&fixture.script)
        .arg("--json")
        .output()?;
    ensure!(!output.status.success());
    ensure!(!fixture.directory.path().join("worker-starts.log").exists());
    ensure!(!fixture.output.exists());
    ensure!(
        events(&output)?
            .iter()
            .all(|event| event["type"] != "finish")
    );
    Ok(())
}

#[test]
fn version_and_help_need_no_python_and_never_create_configuration() -> Result<()> {
    let fixture = Fixture::new("startup_crash", 0)?;
    for flag in ["--version", "--help"] {
        let output = fixture.command().arg(flag).output()?;
        ensure!(output.status.success());
        ensure!(!fixture.app.exists());
        ensure!(!fixture.directory.path().join("worker.pid").exists());
    }
    Ok(())
}

#[test]
fn invalid_settings_do_not_echo_secret_arguments_or_write_configuration() -> Result<()> {
    let fixture = Fixture::new("startup_crash", 1)?;
    let secret = "synthetic-sensitive-config-value";
    let output = fixture
        .command()
        .arg(&fixture.inputs[0])
        .args([
            "--openai",
            "--openai-api-key",
            secret,
            "--qps",
            secret,
            "--json",
        ])
        .output()?;
    ensure!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(!combined.contains(secret));
    ensure!(!fixture.app.exists());
    ensure!(!fixture.directory.path().join("worker.pid").exists());
    ensure!(events(&output)?[0]["type"] == "error");
    Ok(())
}

#[test]
fn multiple_pdfs_share_the_task_pipeline_and_return_one_batch_terminal() -> Result<()> {
    let fixture = Fixture::new("finished_then_exit", 2)?;
    let server = LoopbackProvider::new()?;
    let output = fixture
        .translation()
        .args([
            "--openai",
            "--openai-api-key",
            "synthetic-only",
            "--openai-base-url",
            server.endpoint(),
            "--openai-model",
            "fixture-model",
            "--qps",
            "200",
        ])
        .output()?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = events(&output)?;
    let terminal: Vec<_> = events
        .iter()
        .filter(|event| event["type"] == "finish")
        .collect();
    ensure!(
        terminal.len() == 1
            && terminal[0]["results"]
                .as_array()
                .context("batch results missing")?
                .len()
                == 2
    );
    ensure!(
        fs::read_to_string(fixture.directory.path().join("worker-starts.log"))?
            .lines()
            .count()
            == 2
    );
    ensure!(
        !fixture.app.join("config").exists(),
        "CLI load must not auto-save merged runtime settings"
    );
    ensure!(
        server.requests().len() == 4,
        "Each PDF initializes main and default term independently"
    );
    Ok(())
}

#[tokio::test]
async fn main_and_term_cli_options_reach_separate_rust_services_over_real_http() -> Result<()> {
    let fixture = Fixture::new("translation_routing", 1)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/v1", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let mut models = Vec::new();
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().await?;
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await?;
                header.push(byte[0]);
                ensure!(header.len() < 64 * 1024);
            }
            let length: usize = std::str::from_utf8(&header)?
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())
                        .flatten()
                })
                .context("content-length missing")?;
            ensure!(length < 64 * 1024);
            let mut body = vec![0; length];
            stream.read_exact(&mut body).await?;
            let body: Value = serde_json::from_slice(&body)?;
            models.push(body["model"].clone());
            let reply = serde_json::to_vec(
                &json!({"choices":[{"message":{"content":body["model"]}}],"usage":{"total_tokens":5,"prompt_tokens":3,"completion_tokens":2}}),
            )?;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n", reply.len()).as_bytes()).await?;
            stream.write_all(&reply).await?;
        }
        Ok::<_, anyhow::Error>(models)
    });
    let mut command = fixture.translation();
    command.args([
        "--openai",
        "--openai-api-key",
        "synthetic-main-key",
        "--openai-base-url",
        &endpoint,
        "--openai-model",
        "main-model",
        "--term-openai",
        "--term-openai-api-key",
        "synthetic-term-key",
        "--term-openai-base-url",
        &endpoint,
        "--term-openai-model",
        "term-model",
    ]);
    let output = timeout(
        Duration::from_secs(15),
        tokio::process::Command::from(command)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = events(&output)?;
    let result = &events
        .iter()
        .find(|event| event["type"] == "finish")
        .context("no terminal result")?["result"];
    ensure!(
        result["translations"]["w-0"] == "main-model"
            && result["translations"]["w-1"] == "term-model"
    );
    ensure!(
        result["translation_metrics"]["main"]["calls"] == 2
            && result["translation_metrics"]["term"]["calls"] == 2
    );
    let mut models = timeout(Duration::from_secs(5), server).await???;
    models.sort_by_key(|model| model.as_str().unwrap_or("").to_owned());
    ensure!(
        models
            == vec![
                json!("main-model"),
                json!("main-model"),
                json!("term-model"),
                json!("term-model")
            ]
    );
    Ok(())
}
