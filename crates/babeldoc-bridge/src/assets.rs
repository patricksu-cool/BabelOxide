use super::{AssetOperation, SessionPlan, WorkerOptions, run_session};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

/// Calls only BabelDOC's resource APIs. Cancellation waits for worker tree
/// cleanup. Restore is per-file and may require a retry after interruption.
pub async fn maintain_assets(
    worker: &WorkerOptions,
    operation: AssetOperation,
    cancel: CancellationToken,
    events: Option<mpsc::Sender<Value>>,
) -> Result<Value> {
    ensure!(!cancel.is_cancelled(), "Resource operation cancelled");
    let home = worker
        .home
        .clone()
        .unwrap_or_else(|| WorkerOptions::default().home.expect("Default worker home"));
    std::fs::create_dir_all(&home).context("Create resource directory")?;
    let home = home.canonicalize().context("Resolve resource directory")?;
    // Resolve an absent home once so the process and its temporary files use
    // the same isolated resource location.
    let worker = WorkerOptions {
        home: Some(home.clone()),
        ..worker.clone()
    };
    let mut staging = None;
    let mut destination = None;
    let (kind, path) = match operation {
        AssetOperation::Warmup => ("warmup", None),
        AssetOperation::GenerateOfflineAssets { output_dir } => {
            std::fs::create_dir_all(&output_dir).context("Create archive directory")?;
            let output_dir = output_dir
                .canonicalize()
                .context("Resolve archive directory")?;
            let temporary =
                tempfile::tempdir_in(&output_dir).context("Create archive staging directory")?;
            let path = temporary.path().to_path_buf();
            destination = Some(output_dir);
            staging = Some(temporary);
            ("generate_offline_assets", Some(path))
        }
        AssetOperation::RestoreOfflineAssets { input } => {
            let path = input
                .canonicalize()
                .context("Offline asset package is unavailable")?;
            ensure!(
                path.is_file() || path.is_dir(),
                "Invalid offline asset package"
            );
            ("restore_offline_assets", Some(path))
        }
    };
    let mut result = run_session(
        &worker,
        SessionPlan {
            start: json!({"operation":kind,"path":path,"max_outstanding":1}),
            services: None,
            temporary_root: home,
            diagnostics: false,
            capacity: 1,
            operation: kind,
        },
        cancel.clone(),
        events,
    )
    .await?;
    ensure!(
        result["kind"] == kind,
        "Worker returned the wrong resource receipt"
    );
    let tag = result["tag"]
        .as_str()
        .context("Worker omitted the asset tag")?;
    ensure!(
        tag.len() == 64 && tag.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Worker returned an invalid asset tag"
    );
    ensure!(!cancel.is_cancelled(), "Resource operation cancelled");
    if let (Some(staging), Some(destination)) = (staging, destination) {
        let name = format!("offline_assets_{tag}.zip");
        ensure!(
            result["archive_name"] == name,
            "Worker returned an invalid archive name"
        );
        let staged = staging
            .path()
            .join(&name)
            .canonicalize()
            .context("Worker did not create the archive")?;
        let staging_root = staging.path().canonicalize()?;
        ensure!(
            staged.starts_with(staging_root) && staged.is_file(),
            "Archive escaped the staging directory"
        );
        let published = publish_archive(&staged, &destination, &name, &cancel).await?;
        result["archive_path"] = json!(published);
    }
    Ok(result)
}

async fn publish_archive(
    source: &Path,
    directory: &Path,
    name: &str,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    let temporary =
        tempfile::NamedTempFile::new_in(directory).context("Create publication temporary file")?;
    let mut output = tokio::fs::File::from_std(
        temporary
            .reopen()
            .context("Open publication temporary file")?,
    );
    let mut input = tokio::fs::File::open(source)
        .await
        .context("Read generated archive")?;
    let size = input.metadata().await?.len();
    ensure!(
        size > 0 && size <= 2 * 1024 * 1024 * 1024,
        "Generated archive size is invalid"
    );
    let mut buffer = vec![0; 1024 * 1024];
    let mut copied = 0;
    loop {
        ensure!(!cancel.is_cancelled(), "Resource operation cancelled");
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        copied += read as u64;
        ensure!(
            copied <= size,
            "Generated archive changed during publication"
        );
        // Do not discard an in-flight filesystem writer on cancellation.
        output.write_all(&buffer[..read]).await?;
    }
    ensure!(
        copied == size,
        "Generated archive changed during publication"
    );
    output.flush().await?;
    output.sync_all().await?;
    drop(output);
    ensure!(!cancel.is_cancelled(), "Resource operation cancelled");
    let target = directory.join(name);
    // This replacement is the commit point. No delete-then-copy fallback.
    temporary
        .persist(&target)
        .map_err(|_| anyhow::anyhow!("Publish offline archive atomically failed"))?;
    Ok(target)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs, os::windows::fs::OpenOptionsExt};

    #[tokio::test]
    async fn locked_publication_target_keeps_old_archive_and_removes_temporary_file() -> Result<()>
    {
        let directory = tempfile::tempdir()?;
        let generated = tempfile::NamedTempFile::new()?;
        fs::write(generated.path(), b"new complete archive")?;
        let target = directory.path().join("archive.zip");
        fs::write(&target, b"previous complete archive")?;
        // FILE_SHARE_READ permits inspection but denies atomic replacement.
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&target)?;
        let error = publish_archive(
            generated.path(),
            directory.path(),
            "archive.zip",
            &CancellationToken::new(),
        )
        .await;
        ensure!(error.is_err());
        ensure!(fs::read(&target)? == b"previous complete archive");
        ensure!(fs::read_dir(directory.path())?.count() == 1);
        drop(locked);
        publish_archive(
            generated.path(),
            directory.path(),
            "archive.zip",
            &CancellationToken::new(),
        )
        .await?;
        ensure!(fs::read(&target)? == b"new complete archive");
        Ok(())
    }
}
