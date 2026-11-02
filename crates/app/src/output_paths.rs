//! GUI submission preparation: select application-owned default output storage
//! and isolate output-name collisions without changing shared CLI defaults.

use anyhow::{Context, Result, ensure};
use babeloxide_config::Settings;
use babeloxide_core::PdfJob;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

/// Finder and other desktop launchers can set the working directory to `/` or
/// another unwritable location. A blank GUI output uses the already-resolved
/// application storage instead. Inspect the original setting: an explicit `.`
/// or a path equal to the working directory is still the user's choice.
/// The shared Settings::pdf_job default remains unchanged for CLI callers.
pub(crate) fn apply_gui_default_output(
    jobs: &mut [PdfJob],
    settings: &Settings,
    app_root: &Path,
) -> Result<()> {
    if settings
        .translation()?
        .output
        .is_some_and(|output| !output.is_empty())
    {
        return Ok(());
    }
    let output = app_root.join("outputs");
    for job in jobs {
        job.output_dir = output.clone();
    }
    Ok(())
}

/// Keep earlier results when a user submits or retries the same PDF again.
/// Preparation and directory cleanup have the same transaction as collisions.
pub(crate) fn preserve_existing_outputs(jobs: &mut [PdfJob]) -> Result<Vec<TempDir>> {
    let mut guards = Vec::new();
    let mut replacements = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        let stem = job
            .input_file
            .file_stem()
            .context("PDF 輸入檔名無效")?
            .to_string_lossy()
            .to_lowercase();
        std::fs::create_dir_all(&job.output_dir).context("無法建立 PDF 輸出資料夾")?;
        let mut exists = false;
        for entry in std::fs::read_dir(&job.output_dir).context("無法讀取 PDF 輸出資料夾")?
        {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with(&format!("{stem}.")) && name.ends_with(".pdf") {
                exists = true;
                break;
            }
        }
        if exists {
            let guard = tempfile::Builder::new()
                .prefix("babeloxide-batch-")
                .tempdir_in(&job.output_dir)?;
            replacements.push((index, guard.path().to_path_buf()));
            guards.push(guard);
        }
    }
    for (index, output) in replacements {
        jobs[index].output_dir = output;
    }
    Ok(guards)
}

/// Run on a blocking background worker before submitting the task. Keep every
/// returned guard only after submission succeeds; dropping an uncommitted guard
/// removes its private directories. Kept result directories survive app exit.
pub(crate) fn isolate_colliding_outputs(jobs: &mut [PdfJob]) -> Result<Vec<TempDir>> {
    if jobs.len() < 2 {
        return Ok(Vec::new());
    }

    let mut keys = Vec::with_capacity(jobs.len());
    let mut counts = HashMap::new();
    for job in jobs.iter() {
        // The bridge canonicalizes input_file before BabelDOC derives its stem;
        // resolve here as well so symlink aliases cannot hide a collision.
        let input = job
            .input_file
            .canonicalize()
            .context("無法確認 PDF 輸入來源，請確認檔案仍然存在")?;
        ensure!(input.is_file(), "PDF 輸入來源不是一般檔案");
        let stem = input
            .file_stem()
            .context("PDF 輸入檔名無效")?
            .to_string_lossy()
            .to_uppercase();

        // GUI defaults are resolved before collision preparation; explicit
        // relative paths remain supported just as they are in the bridge.
        // Canonicalization also merges spelling and directory aliases.
        std::fs::create_dir_all(&job.output_dir).context("無法建立 PDF 輸出資料夾")?;
        let output = job
            .output_dir
            .canonicalize()
            .context("無法確認 PDF 輸出資料夾")?;
        let key = (output, stem);
        *counts.entry(key.clone()).or_insert(0_usize) += 1;
        keys.push(key);
    }

    let mut guards: Vec<TempDir> = Vec::new();
    let mut output_roots = HashMap::<PathBuf, usize>::new();
    let mut replacements = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        if counts[key] < 2 {
            continue;
        }
        let guard_index = if let Some(index) = output_roots.get(&key.0) {
            *index
        } else {
            let guard = tempfile::Builder::new()
                .prefix("babeloxide-batch-")
                .tempdir_in(&key.0)
                .context("無法建立批次 PDF 的獨立輸出資料夾")?;
            let index = guards.len();
            guards.push(guard);
            output_roots.insert(key.0.clone(), index);
            index
        };
        // The private root is freshly created. Use only an index as the child
        // name, and require a new directory rather than reusing existing data.
        let output = guards[guard_index].path().join(format!("job-{index:04}"));
        std::fs::create_dir(&output).context("無法建立 PDF 的獨立輸出資料夾")?;
        replacements.push((index, output));
    }

    // Commit only after all preparation succeeds. An I/O error leaves the jobs
    // untouched and automatically drops every private temporary root.
    for (index, output) in replacements {
        jobs[index].output_dir = output;
    }
    Ok(guards)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_gui_output_uses_app_storage_even_when_the_launch_directory_is_root() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let app_root = temporary.path().join("app");
        for configured_output in [serde_json::Value::Null, serde_json::json!("")] {
            let mut settings = Settings::default();
            settings.set("translation.output", configured_output)?;
            let mut jobs = vec![
                job(input(temporary.path(), "one", "report.pdf"), Path::new("/")),
                job(input(temporary.path(), "two", "other.pdf"), Path::new("/")),
            ];
            // Model the shared builder's Finder-launch result without changing
            // the process-wide current directory under parallel Rust tests.
            apply_gui_default_output(&mut jobs, &settings, &app_root)?;
            let expected = app_root.join("outputs");
            assert!(jobs.iter().all(|job| job.output_dir == expected));
            assert!(preserve_existing_outputs(&mut jobs)?.is_empty());
            // The engine uses tempdir_in(output_dir), so verify its failing
            // operation now succeeds in the selected application storage.
            let worker_temporary = tempfile::tempdir_in(&jobs[0].output_dir)?;
            assert_eq!(worker_temporary.path().parent(), Some(expected.as_path()));
            assert!(
                settings
                    .translation()?
                    .output
                    .is_none_or(|path| path.is_empty())
            );
        }
        Ok(())
    }

    #[test]
    fn explicit_gui_output_is_preserved_including_dot_and_the_launch_directory() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let app_root = temporary.path().join("app");
        for output in [
            PathBuf::from("."),
            PathBuf::from("/"),
            std::env::current_dir()?,
            temporary.path().join("chosen-output"),
        ] {
            let mut settings = Settings::default();
            settings.set("translation.output", serde_json::json!(output))?;
            let mut jobs = [settings.pdf_job(input(temporary.path(), "one", "report.pdf"))?];
            apply_gui_default_output(&mut jobs, &settings, &app_root)?;
            assert_eq!(jobs[0].output_dir, output);
            assert!(!app_root.exists());
        }
        Ok(())
    }

    #[test]
    fn unusable_explicit_gui_output_fails_without_falling_back() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let output = temporary.path().join("not-a-directory");
        std::fs::write(&output, b"keep this file")?;
        let app_root = temporary.path().join("app");
        let mut settings = Settings::default();
        settings.set("translation.output", serde_json::json!(output))?;
        let mut jobs = [settings.pdf_job(input(temporary.path(), "one", "report.pdf"))?];
        apply_gui_default_output(&mut jobs, &settings, &app_root)?;
        assert!(preserve_existing_outputs(&mut jobs).is_err());
        assert_eq!(jobs[0].output_dir, output);
        assert_eq!(std::fs::read(&output)?, b"keep this file");
        assert!(!app_root.exists());
        Ok(())
    }

    #[test]
    fn repeating_one_pdf_preserves_previous_outputs() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let output = temporary.path().join("output");
        std::fs::create_dir(&output)?;
        std::fs::write(output.join("report.zh.mono.pdf"), b"previous result")?;
        let mut jobs = vec![job(input(temporary.path(), "one", "report.pdf"), &output)];
        let guards = preserve_existing_outputs(&mut jobs)?;
        assert_eq!(guards.len(), 1);
        assert_ne!(jobs[0].output_dir, output);
        assert_eq!(
            std::fs::read(output.join("report.zh.mono.pdf"))?,
            b"previous result"
        );
        drop(guards);
        assert!(!jobs[0].output_dir.exists());
        Ok(())
    }

    fn input(root: &Path, directory: &str, name: &str) -> PathBuf {
        let parent = root.join(directory);
        std::fs::create_dir_all(&parent).unwrap();
        let path = parent.join(name);
        std::fs::write(&path, b"test PDF input").unwrap();
        path
    }

    fn job(input: PathBuf, output: &Path) -> PdfJob {
        let mut job = Settings::default().pdf_job(input).unwrap();
        job.output_dir = output.to_path_buf();
        job
    }

    #[test]
    fn same_stem_outputs_keep_distinct_contents_without_replacing_existing_results() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("output");
        std::fs::create_dir(&output).unwrap();
        let filename = "report.zh.mono.pdf";
        std::fs::write(output.join(filename), b"existing result").unwrap();
        let mut jobs = vec![
            job(input(temporary.path(), "one", "report.pdf"), &output),
            job(input(temporary.path(), "two", "report.pdf"), &output),
        ];
        let guards = isolate_colliding_outputs(&mut jobs).unwrap();
        assert_eq!(guards.len(), 1);
        assert_ne!(jobs[0].output_dir, jobs[1].output_dir);
        assert_eq!(jobs[0].output_dir.parent(), Some(guards[0].path()));
        assert_eq!(jobs[1].output_dir.parent(), Some(guards[0].path()));
        std::fs::write(jobs[0].output_dir.join(filename), b"first result").unwrap();
        std::fs::write(jobs[1].output_dir.join(filename), b"second result").unwrap();
        let kept: Vec<_> = guards.into_iter().map(TempDir::keep).collect();
        assert!(kept[0].is_dir());
        assert_eq!(
            std::fs::read(jobs[0].output_dir.join(filename)).unwrap(),
            b"first result"
        );
        assert_eq!(
            std::fs::read(jobs[1].output_dir.join(filename)).unwrap(),
            b"second result"
        );
        assert_eq!(
            std::fs::read(output.join(filename)).unwrap(),
            b"existing result"
        );
    }

    #[test]
    fn windows_case_names_and_canonical_output_aliases_collide() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("output");
        std::fs::create_dir(&output).unwrap();
        let alias = if cfg!(windows) {
            PathBuf::from(output.to_string_lossy().to_uppercase())
        } else {
            output.join(".")
        };
        let mut jobs = vec![
            job(input(temporary.path(), "one", "Report.pdf"), &output),
            job(input(temporary.path(), "two", "report.PDF"), &alias),
        ];
        let guards = isolate_colliding_outputs(&mut jobs).unwrap();
        assert_eq!(guards.len(), 1);
        assert_ne!(jobs[0].output_dir, jobs[1].output_dir);
    }

    #[test]
    fn same_stem_in_different_output_roots_is_unchanged() {
        let temporary = tempfile::tempdir().unwrap();
        let outputs = [
            temporary.path().join("left"),
            temporary.path().join("right"),
        ];
        let mut jobs = vec![
            job(input(temporary.path(), "one", "report.pdf"), &outputs[0]),
            job(input(temporary.path(), "two", "report.pdf"), &outputs[1]),
        ];
        assert!(isolate_colliding_outputs(&mut jobs).unwrap().is_empty());
        assert_eq!(jobs[0].output_dir, outputs[0]);
        assert_eq!(jobs[1].output_dir, outputs[1]);
    }

    #[test]
    fn distinct_stems_and_unrelated_jobs_keep_original_output_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("output");
        let mut jobs = vec![
            job(input(temporary.path(), "one", "report.pdf"), &output),
            job(input(temporary.path(), "two", "other.pdf"), &output),
        ];
        assert!(isolate_colliding_outputs(&mut jobs).unwrap().is_empty());
        assert!(jobs.iter().all(|job| job.output_dir == output));

        jobs.push(job(input(temporary.path(), "three", "report.pdf"), &output));
        let guards = isolate_colliding_outputs(&mut jobs).unwrap();
        assert_eq!(guards.len(), 1);
        assert_eq!(jobs[1].output_dir, output);
        assert_ne!(jobs[0].output_dir, jobs[2].output_dir);
    }

    #[test]
    fn single_default_job_preserves_the_settings_current_directory_output() {
        let temporary = tempfile::tempdir().unwrap();
        let input = input(temporary.path(), "one", "report.pdf");
        let mut jobs = [Settings::default().pdf_job(input).unwrap()];
        let original = jobs[0].output_dir.clone();
        assert_eq!(original, std::env::current_dir().unwrap());
        assert!(isolate_colliding_outputs(&mut jobs).unwrap().is_empty());
        assert_eq!(jobs[0].output_dir, original);
    }

    #[test]
    fn colliding_jobs_use_one_private_guard_per_actual_output_root() {
        let temporary = tempfile::tempdir().unwrap();
        let outputs = [
            temporary.path().join("left"),
            temporary.path().join("right"),
        ];
        let mut jobs = vec![
            job(input(temporary.path(), "one", "report.pdf"), &outputs[0]),
            job(input(temporary.path(), "two", "report.pdf"), &outputs[0]),
            job(input(temporary.path(), "three", "report.pdf"), &outputs[1]),
            job(input(temporary.path(), "four", "report.pdf"), &outputs[1]),
        ];
        let guards = isolate_colliding_outputs(&mut jobs).unwrap();
        assert_eq!(guards.len(), 2);
        for (index, job) in jobs.iter().enumerate() {
            assert!(
                job.output_dir
                    .starts_with(outputs[index / 2].canonicalize().unwrap())
            );
        }
    }

    #[test]
    fn dropping_unkept_guards_cleans_private_outputs_and_preserves_existing_data() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("output");
        std::fs::create_dir(&output).unwrap();
        let existing = output.join("existing.pdf");
        std::fs::write(&existing, b"existing").unwrap();
        let mut jobs = vec![
            job(input(temporary.path(), "one", "report.pdf"), &output),
            job(input(temporary.path(), "two", "report.pdf"), &output),
        ];
        let guards = isolate_colliding_outputs(&mut jobs).unwrap();
        let private = guards[0].path().to_path_buf();
        std::fs::write(jobs[0].output_dir.join("partial.pdf"), b"partial").unwrap();
        drop(guards);
        assert!(!private.exists());
        assert_eq!(std::fs::read(existing).unwrap(), b"existing");
        assert_eq!(std::fs::read_dir(output).unwrap().count(), 1);
    }
}
