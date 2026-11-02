//! Command parsing shared with the native application. Help/version do no I/O.

use anyhow::Result;
use babeloxide_bridge::WorkerOptions;
use babeloxide_config::{ConfigStore, FieldType, Settings, field_specs};
use clap::{Arg, ArgAction, ArgGroup, Command, ValueHint};
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

pub struct Invocation {
    pub overrides: Value,
    pub json: bool,
    pub save_config: bool,
    pub cache_file: Option<PathBuf>,
    pub worker: WorkerOptions,
    /// Explicit trusted runtime/native directory for source development.
    pub runtime_dir: Option<PathBuf>,
    worker_home_explicit: bool,
}

pub fn command() -> Command {
    let mut command = Command::new(babeloxide_core::PRODUCT_NAME)
        .version(babeloxide_core::PRODUCT_VERSION)
        .about("Translate PDFs with Rust services and the BabelDOC engine")
        .arg(
            Arg::new("input-files")
                .num_args(0..)
                .action(ArgAction::Append)
                .value_hint(ValueHint::FilePath),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Write progress and results as newline-delimited JSON"),
        )
        .arg(
            Arg::new("runtime-dir")
                .long("runtime-dir")
                .value_hint(ValueHint::DirPath)
                .help("Complete bundle directory with Python, Worker, assets and native PDFium (development override)"),
        )
        .group(ArgGroup::new("worker-runtime").args(["python", "runtime-dir"]).multiple(true))
        .arg(
            Arg::new("python")
                .long("python")
                .requires("worker")
                .value_hint(ValueHint::FilePath)
                .help("Isolated worker Python executable (development override)"),
        )
        .arg(
            Arg::new("worker")
                .long("worker")
                .requires("worker-runtime")
                .value_hint(ValueHint::FilePath)
                .help("BabelDOC adapter script (development override)"),
        )
        .arg(
            Arg::new("worker-home")
                .long("worker-home")
                .value_hint(ValueHint::DirPath)
                .help("BabelDOC model/font cache directory"),
        )
        .arg(
            Arg::new("cache-file")
                .long("cache-file")
                .value_hint(ValueHint::FilePath)
                .help("Translation SQLite cache; default: application cache directory"),
        )
        .arg(
            Arg::new("save-config")
                .long("save-config")
                .action(ArgAction::SetTrue)
                .help("Save the merged settings to the program config directory"),
        );
    for spec in field_specs() {
        if matches!(spec.flag.as_str(), "input-files" | "version") {
            continue;
        }
        let mut arg = Arg::new(spec.flag.clone())
            .long(spec.flag.clone())
            .help(spec.help.clone())
            .hide_env_values(true)
            .hide_default_value(true);
        if spec.kind == FieldType::Bool {
            // Python's store_true/store_false depends on the declared default.
            arg = arg
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value(if spec.default == Value::Bool(true) {
                    "false"
                } else {
                    "true"
                });
        } else {
            arg = arg.num_args(1).allow_hyphen_values(true);
        }
        if spec.secret {
            arg = arg.hide_possible_values(true);
        }
        command = command.arg(arg);
    }
    command
}

pub fn parse<I, T>(arguments: I) -> std::result::Result<Invocation, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = command().try_get_matches_from(arguments)?;
    let mut overrides = Map::new();
    if let Some(inputs) = matches.get_many::<String>("input-files") {
        overrides.insert("input_files".into(), json!(inputs.collect::<Vec<_>>()));
    }
    for spec in field_specs() {
        if matches!(spec.flag.as_str(), "input-files" | "version") {
            continue;
        }
        if let Some(value) = matches.get_one::<String>(&spec.flag) {
            overrides.insert(spec.flag.replace('-', "_"), json!(value));
        }
    }
    let runtime_dir = matches
        .get_one::<String>("runtime-dir")
        .map(|value| {
            std::path::absolute(value).map_err(|_| {
                clap::Error::raw(
                    clap::error::ErrorKind::InvalidValue,
                    "Cannot resolve --runtime-dir",
                )
            })
        })
        .transpose()?;
    let mut worker = runtime_dir
        .as_deref()
        .map(WorkerOptions::for_bundle)
        .unwrap_or_default();
    if let Some(value) = matches.get_one::<String>("python") {
        worker.python = value.into();
    }
    if let Some(value) = matches.get_one::<String>("worker") {
        worker.script = value.into();
    }
    if let Some(value) = matches.get_one::<String>("worker-home") {
        worker.home = Some(value.into());
    }
    Ok(Invocation {
        overrides: Value::Object(overrides),
        json: matches.get_flag("json"),
        save_config: matches.get_flag("save-config"),
        cache_file: matches.get_one::<String>("cache-file").map(PathBuf::from),
        worker,
        runtime_dir,
        worker_home_explicit: matches.get_one::<String>("worker-home").is_some(),
    })
}

impl Invocation {
    /// The caller chooses which environment to supply; tests use synthetic maps.
    pub fn load(&self, environment: &BTreeMap<String, String>) -> Result<(ConfigStore, Settings)> {
        let store = ConfigStore::from_environment(environment)?.resolve_writable(environment)?;
        let settings = store.load_with_overrides(environment, &self.overrides)?;
        Ok((store, settings))
    }

    /// Mutable engine data follows configuration, including read-only fallback.
    pub fn worker_for_store(&self, store: &ConfigStore) -> WorkerOptions {
        let mut worker = self.worker.clone();
        if !self.worker_home_explicit {
            worker.home = Some(store.app_root().join("cache/babeldoc-home"));
        }
        worker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_schema_has_no_conflicts_and_preserves_implicit_timeout() {
        command().debug_assert();
        let parsed = parse(["BabelOxide", "input.pdf", "--openai"]).unwrap();
        assert!(parsed.overrides.get("openai_timeout").is_none());
        let explicit = parse(["BabelOxide", "input.pdf", "--openai-timeout", "1.5"]).unwrap();
        assert_eq!(explicit.overrides["openai_timeout"], "1.5");
    }

    #[test]
    fn bool_presence_matches_python_and_does_not_consume_a_positional_pdf() {
        let parsed = parse([
            "BabelOxide",
            "--google",
            "a.pdf",
            "b.pdf",
            "--ignore-cache=false",
        ])
        .unwrap();
        assert_eq!(parsed.overrides["google"], "true");
        assert_eq!(parsed.overrides["input_files"], json!(["a.pdf", "b.pdf"]));
        assert_eq!(parsed.overrides["ignore_cache"], "false");
        let enabled_by_default = field_specs()
            .iter()
            .find(|s| s.kind == FieldType::Bool && s.default == Value::Bool(true))
            .unwrap();
        let flag = format!("--{}", enabled_by_default.flag);
        let parsed = parse(["BabelOxide", flag.as_str()]).unwrap();
        assert_eq!(
            parsed.overrides[enabled_by_default.flag.replace('-', "_")],
            "false"
        );
    }

    #[test]
    fn help_and_version_do_not_need_input_files_or_configuration() {
        for flag in ["--help", "--version"] {
            let error = match parse(["BabelOxide", flag]) {
                Err(error) => error,
                Ok(_) => panic!("expected display output"),
            };
            assert!(!error.use_stderr());
        }
    }

    #[test]
    fn explicit_runtime_supplies_engine_assets_and_native_root_without_changing_storage() {
        let root = std::env::current_dir().unwrap().join("trusted-bundle");
        let parsed = parse([
            "BabelOxide-gui",
            "--runtime-dir",
            root.to_str().unwrap(),
            "--worker",
            "python-worker/worker.py",
        ])
        .unwrap();
        assert_eq!(parsed.runtime_dir.as_deref(), Some(root.as_path()));
        assert_eq!(
            parsed.worker.script,
            PathBuf::from("python-worker/worker.py")
        );
        assert!(parsed.worker.python.starts_with(&root));
        assert_eq!(
            parsed.worker.asset_manifest,
            Some(root.join("resources/engine-assets.json"))
        );
        let storage =
            ConfigStore::new(std::env::current_dir().unwrap().join("writable-data")).unwrap();
        assert_eq!(
            parsed.worker_for_store(&storage).home,
            Some(storage.app_root().join("cache/babeldoc-home"))
        );
        assert!(parse(["BabelOxide", "--worker", "worker.py"]).is_err());
        assert!(parse(["BabelOxide", "--python", "python"]).is_err());
    }
}
