//! Copy the immutable engine assets into a writable BabelDOC home, atomically.
//! Existing user files are verified and never replaced. No runtime DB is copied.

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path},
};
use tokio_util::sync::CancellationToken;

fn plain_path(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "Engine asset path is a link"
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        metadata.file_attributes() & 0x400 == 0,
                        "Engine asset path is a reparse point"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Inspect engine asset path"),
        }
    }
    Ok(())
}

fn digest(
    path: &Path,
    cancel: &CancellationToken,
    mut writer: Option<&mut File>,
) -> Result<(u64, String)> {
    let mut source = File::open(path).context("Read engine asset")?;
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        ensure!(!cancel.is_cancelled(), "Translation cancelled");
        let count = source.read(&mut buffer).context("Read engine asset")?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        total += count as u64;
        if let Some(destination) = writer.as_deref_mut() {
            destination
                .write_all(&buffer[..count])
                .context("Initialize engine asset")?;
        }
    }
    Ok((total, format!("{:x}", hash.finalize())))
}

pub(super) fn seed_assets(manifest: &Path, home: &Path, cancel: &CancellationToken) -> Result<()> {
    if !manifest.try_exists()? {
        // Source checkouts may have no bundled resources. Packaged resources
        // without their manifest indicate an incomplete/corrupted bundle.
        ensure!(
            !manifest
                .parent()
                .is_some_and(|parent| parent.join("worker-home").exists()),
            "Bundled engine asset manifest is missing"
        );
        return Ok(());
    }
    plain_path(manifest)?;
    plain_path(home)?;
    ensure!(
        fs::metadata(manifest)?.len() <= 1024 * 1024,
        "Engine asset manifest is too large"
    );
    let value: Value =
        serde_json::from_slice(&fs::read(manifest)?).context("Read engine asset manifest")?;
    ensure!(
        value["schema_version"] == 1,
        "Unsupported engine asset manifest"
    );
    let files = value["files"]
        .as_array()
        .context("Engine asset list is missing")?;
    ensure!(
        !files.is_empty() && files.len() <= 2048,
        "Invalid engine asset count"
    );
    let source = manifest
        .parent()
        .context("Engine resource directory")?
        .join("worker-home/.cache/babeldoc");
    let destination = home.join(".cache/babeldoc");
    let mut seen = BTreeSet::new();
    for asset in files {
        ensure!(!cancel.is_cancelled(), "Translation cancelled");
        let category = asset["category"]
            .as_str()
            .context("Missing engine asset category")?;
        ensure!(
            matches!(category, "fonts" | "models" | "cmap" | "tiktoken"),
            "Invalid engine asset category"
        );
        let name = asset["name"]
            .as_str()
            .context("Missing engine asset name")?;
        ensure!(
            !name.is_empty()
                && !name
                    .chars()
                    .any(|ch| ch.is_control() || "/\\:<>\"|?*".contains(ch))
                && !name.ends_with(['.', ' '])
                && matches!(
                    Path::new(name).components().collect::<Vec<_>>().as_slice(),
                    [Component::Normal(_)]
                ),
            "Invalid engine asset name"
        );
        ensure!(seen.insert((category, name)), "Duplicate engine asset");
        let bytes = asset["bytes"]
            .as_u64()
            .context("Missing engine asset size")?;
        let hash = asset["sha256"]
            .as_str()
            .context("Missing engine asset hash")?;
        ensure!(
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()),
            "Invalid engine asset hash"
        );
        let expected = (bytes, hash.to_owned());
        let target = destination.join(category).join(name);
        plain_path(&target)?;
        if target.try_exists()? {
            ensure!(
                digest(&target, cancel, None)? == expected,
                "Existing BabelDOC asset differs from the bundle; choose a fresh --worker-home to preserve your files"
            );
            continue;
        }
        let bundled = source.join(category).join(name);
        plain_path(&bundled)?;
        let parent = target.parent().context("Engine asset directory")?;
        fs::create_dir_all(parent).context("Create writable engine asset directory")?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).context("Create temporary engine asset")?;
        ensure!(
            digest(&bundled, cancel, Some(temporary.as_file_mut()))? == expected,
            "Bundled engine asset checksum differs"
        );
        temporary.as_file_mut().sync_all()?;
        ensure!(!cancel.is_cancelled(), "Translation cancelled");
        match temporary.persist_noclobber(&target) {
            Ok(_) => {}
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                ensure!(
                    digest(&target, cancel, None)? == expected,
                    "Concurrent engine asset differs; existing file preserved"
                );
            }
            Err(_) => bail!("Publish writable engine asset failed"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("資源 with spaces");
        let source = resources.join("worker-home/.cache/babeldoc/fonts");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("font.ttf"), b"fixed font fixture").unwrap();
        fs::write(
            source.parent().unwrap().join("cache.db"),
            b"private cache not copied",
        )
        .unwrap();
        let manifest = resources.join("engine-assets.json");
        let (bytes, hash) =
            digest(&source.join("font.ttf"), &CancellationToken::new(), None).unwrap();
        fs::write(&manifest, serde_json::to_vec(&json!({"schema_version":1,"files":[{"category":"fonts","name":"font.ttf","bytes":bytes,"sha256":hash}]})).unwrap()).unwrap();
        let home = root.path().join("可寫 cache");
        (root, manifest, home)
    }

    #[test]
    fn seeds_allowlisted_assets_once_without_copying_runtime_cache() -> Result<()> {
        let (_root, manifest, home) = fixture();
        seed_assets(&manifest, &home, &CancellationToken::new())?;
        seed_assets(&manifest, &home, &CancellationToken::new())?;
        assert_eq!(
            fs::read(home.join(".cache/babeldoc/fonts/font.ttf"))?,
            b"fixed font fixture"
        );
        assert!(!home.join(".cache/babeldoc/cache.db").exists());
        Ok(())
    }

    #[test]
    fn changed_user_asset_is_preserved_and_bad_bundle_never_publishes() -> Result<()> {
        let (_root, manifest, home) = fixture();
        seed_assets(&manifest, &home, &CancellationToken::new())?;
        let target = home.join(".cache/babeldoc/fonts/font.ttf");
        fs::write(&target, b"user font")?;
        assert!(seed_assets(&manifest, &home, &CancellationToken::new()).is_err());
        assert_eq!(fs::read(&target)?, b"user font");
        fs::remove_file(&target)?;
        fs::write(
            manifest
                .parent()
                .unwrap()
                .join("worker-home/.cache/babeldoc/fonts/font.ttf"),
            b"tamper",
        )?;
        assert!(seed_assets(&manifest, &home, &CancellationToken::new()).is_err());
        assert_eq!(fs::read_dir(target.parent().unwrap())?.count(), 0);
        Ok(())
    }

    #[test]
    fn unsafe_manifest_and_cancellation_do_not_touch_user_data() -> Result<()> {
        let (_root, manifest, home) = fixture();
        let mut value: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
        value["files"][0]["name"] = "../escape".into();
        fs::write(&manifest, serde_json::to_vec(&value)?)?;
        assert!(seed_assets(&manifest, &home, &CancellationToken::new()).is_err());
        assert!(!home.exists());
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(seed_assets(&manifest, &home, &cancel).is_err());
        assert!(!home.exists());
        Ok(())
    }

    #[test]
    fn racing_initializers_publish_one_complete_file() -> Result<()> {
        let (_root, manifest, home) = fixture();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| seed_assets(&manifest, &home, &CancellationToken::new()));
            let second = scope.spawn(|| seed_assets(&manifest, &home, &CancellationToken::new()));
            first.join().unwrap()?;
            second.join().unwrap()?;
            Ok::<(), anyhow::Error>(())
        })?;
        assert_eq!(fs::read_dir(home.join(".cache/babeldoc/fonts"))?.count(), 1);
        Ok(())
    }
}
