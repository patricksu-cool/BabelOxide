//! Copy the immutable engine assets into a writable BabelDOC home, atomically.
//! Existing user files are verified and never replaced. No runtime DB is copied.

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

/// Resolve only macOS's root-owned system aliases, never an asset/home link.
/// All subsequent I/O uses the resolved prefix so the alias is not revisited.
#[cfg(target_os = "macos")]
fn system_path(path: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    let path = std::path::absolute(path).context("Resolve engine asset path")?;
    for name in ["var", "tmp"] {
        let alias = Path::new("/").join(name);
        let Ok(relative) = path.strip_prefix(&alias) else {
            continue;
        };
        let metadata = fs::symlink_metadata(&alias)?;
        if !metadata.file_type().is_symlink() {
            return Ok(path);
        }
        // These are OS trust anchors, not user-selected directories. Do not
        // canonicalize an arbitrary existing ancestor and thereby hide a link.
        for root in [Path::new("/"), Path::new("/private")] {
            let metadata = fs::symlink_metadata(root)?;
            ensure!(
                metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
                "Untrusted engine system root: {}",
                root.display()
            );
        }
        let expected = Path::new("/private").join(name);
        let link = fs::read_link(&alias)?;
        ensure!(
            metadata.uid() == 0 && (link == expected || link == Path::new("private").join(name)),
            "Untrusted engine system alias: {}",
            alias.display()
        );
        let resolved = alias.canonicalize()?;
        let metadata = fs::symlink_metadata(&expected)?;
        ensure!(
            resolved == expected && metadata.is_dir() && metadata.uid() == 0,
            "Unexpected engine system alias target: {}",
            alias.display()
        );
        return Ok(resolved.join(relative));
    }
    Ok(path)
}

fn plain_path(path: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let path = system_path(path)?;
    #[cfg(not(target_os = "macos"))]
    let path = path.to_path_buf();
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "Engine asset path is a link: {}",
                    ancestor.display()
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        metadata.file_attributes() & 0x400 == 0,
                        "Engine asset path is a reparse point: {}",
                        ancestor.display()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Inspect engine asset path"),
        }
    }
    Ok(path)
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
    let manifest = plain_path(manifest)?;
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
    let home = plain_path(home)?;
    ensure!(
        fs::metadata(&manifest)?.len() <= 1024 * 1024,
        "Engine asset manifest is too large"
    );
    let value: Value =
        serde_json::from_slice(&fs::read(&manifest)?).context("Read engine asset manifest")?;
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
        plain_path(parent)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).context("Create temporary engine asset")?;
        ensure!(
            digest(&bundled, cancel, Some(temporary.as_file_mut()))? == expected,
            "Bundled engine asset checksum differs"
        );
        temporary.as_file_mut().sync_all()?;
        ensure!(!cancel.is_cancelled(), "Translation cancelled");
        plain_path(&target)?;
        match temporary.persist_noclobber(&target) {
            Ok(_) => {}
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                plain_path(&target)?;
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
        fixture_in(tempfile::tempdir().unwrap())
    }

    fn fixture_in(root: tempfile::TempDir) -> (tempfile::TempDir, PathBuf, PathBuf) {
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
        for name in [
            "../escape",
            "..",
            "/escape",
            "C:\\escape",
            "a/b",
            "a\\b",
            "a:stream",
        ] {
            value["files"][0]["name"] = name.into();
            fs::write(&manifest, serde_json::to_vec(&value)?)?;
            assert!(
                seed_assets(&manifest, &home, &CancellationToken::new()).is_err(),
                "{name}"
            );
            assert!(!home.exists());
        }
        value["files"][0]["name"] = "font.ttf".into();
        fs::write(&manifest, serde_json::to_vec(&value)?)?;
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

    #[cfg(any(windows, unix))]
    fn directory_link(target: &Path, link: &Path) -> Result<()> {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link)?;
        #[cfg(windows)]
        {
            // Junctions exercise the Windows reparse-point guard without
            // requiring Developer Mode or administrator symlink privileges.
            let powershell = PathBuf::from(std::env::var_os("SystemRoot").context("SystemRoot")?)
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let output = std::process::Command::new(powershell)
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
                    "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:BABELOXIDE_TEST_LINK -Target $env:BABELOXIDE_TEST_TARGET | Out-Null"])
                .env("BABELOXIDE_TEST_LINK", link)
                .env("BABELOXIDE_TEST_TARGET", target)
                .output()?;
            ensure!(
                output.status.success(),
                "Create test junction: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn linked_bundle_or_home_root_is_not_a_system_alias() -> Result<()> {
        for bundle in [true, false] {
            let (root, manifest, home) = fixture();
            let outside = root.path().join("outside");
            fs::create_dir(&outside)?;
            let sentinel = outside.join("sentinel");
            fs::write(&sentinel, b"user data")?;
            if bundle {
                let resources = manifest.parent().unwrap();
                let real = root.path().join("real resources");
                fs::rename(resources, &real)?;
                directory_link(&real, resources)?;
            } else {
                directory_link(&outside, &home)?;
            }
            let error = seed_assets(&manifest, &home, &CancellationToken::new()).unwrap_err();
            assert!(
                error.to_string().contains("Engine asset path is a"),
                "{error:#}"
            );
            assert_eq!(fs::read(&sentinel)?, b"user data");
            assert_eq!(fs::read_dir(&outside)?.count(), 1);
            assert!(!home.join(".cache").exists());
        }
        Ok(())
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn linked_cache_and_asset_directories_cannot_redirect_initialization() -> Result<()> {
        for relative in [".cache", ".cache/babeldoc/fonts"] {
            let (root, manifest, home) = fixture();
            let outside = root.path().join("outside");
            fs::create_dir(&outside)?;
            fs::write(outside.join("sentinel"), b"user data")?;
            let link = home.join(relative);
            fs::create_dir_all(link.parent().unwrap())?;
            directory_link(&outside, &link)?;
            assert!(seed_assets(&manifest, &home, &CancellationToken::new()).is_err());
            assert_eq!(fs::read(outside.join("sentinel"))?, b"user data");
            assert_eq!(fs::read_dir(&outside)?.count(), 1);
        }
        let (root, manifest, home) = fixture();
        let fonts = manifest
            .parent()
            .unwrap()
            .join("worker-home/.cache/babeldoc/fonts");
        let real = root.path().join("real fonts");
        fs::rename(&fonts, &real)?;
        directory_link(&real, &fonts)?;
        assert!(seed_assets(&manifest, &home, &CancellationToken::new()).is_err());
        assert!(!home.exists());
        assert_eq!(fs::read(real.join("font.ttf"))?, b"fixed font fixture");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn linked_asset_files_and_dangling_manifest_are_rejected() -> Result<()> {
        for source in [true, false] {
            let (root, manifest, home) = fixture();
            let outside = root.path().join("outside.ttf");
            // Matching checksums do not make a symlink an allowed asset.
            fs::write(&outside, b"fixed font fixture")?;
            let link = if source {
                manifest
                    .parent()
                    .unwrap()
                    .join("worker-home/.cache/babeldoc/fonts/font.ttf")
            } else {
                home.join(".cache/babeldoc/fonts/font.ttf")
            };
            fs::create_dir_all(link.parent().unwrap())?;
            if source {
                fs::remove_file(&link)?;
            }
            std::os::unix::fs::symlink(&outside, &link)?;
            assert!(seed_assets(&manifest, &home, &CancellationToken::new()).is_err());
            assert_eq!(fs::read(&outside)?, b"fixed font fixture");
        }
        let (root, manifest, home) = fixture();
        fs::remove_file(&manifest)?;
        std::os::unix::fs::symlink(root.path().join("missing manifest"), &manifest)?;
        assert!(seed_assets(&manifest, &home, &CancellationToken::new()).is_err());
        assert!(!home.exists());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_system_temp_aliases_seed_using_the_same_resource_policy() -> Result<()> {
        for base in [
            std::env::temp_dir(),
            "/var/tmp".into(),
            "/tmp".into(),
            "/private/tmp".into(),
        ] {
            let temporary = tempfile::tempdir_in(&base)?;
            eprintln!(
                "Engine fixture: {}; canonical: {}",
                temporary.path().display(),
                temporary.path().canonicalize()?.display()
            );
            for ancestor in temporary.path().ancestors() {
                if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
                    eprintln!(
                        "System alias: {} -> {}",
                        ancestor.display(),
                        fs::read_link(ancestor)?.display()
                    );
                }
            }
            let (_root, manifest, home) = fixture_in(temporary);
            seed_assets(&manifest, &home, &CancellationToken::new())?;
            seed_assets(&manifest, &home, &CancellationToken::new())?;
            assert_eq!(plain_path(&manifest)?, manifest.canonicalize()?);
            assert_eq!(plain_path(&home)?, home.canonicalize()?);
            assert_eq!(
                fs::read(home.join(".cache/babeldoc/fonts/font.ttf"))?,
                b"fixed font fixture"
            );
            assert!(!home.join(".cache/babeldoc/cache.db").exists());
        }
        Ok(())
    }
}
