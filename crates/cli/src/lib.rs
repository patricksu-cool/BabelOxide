//! Command parsing shared with the native application. Help/version do no I/O.

use anyhow::Result;
use babeldoc_bridge::WorkerOptions;
use clap::{Arg, ArgAction, Command, ValueHint};
use pdf2zh_config::{ConfigStore, FieldType, Settings, field_specs};
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

pub struct Invocation {
    pub overrides: Value,
    pub json: bool,
    pub save_config: bool,
    pub cache_file: Option<PathBuf>,
    pub worker: WorkerOptions,
}

pub fn command() -> Command {
    let mut command = Command::new("pdf2zh")
        .version(env!("CARGO_PKG_VERSION"))
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
            Arg::new("python")
                .long("python")
                .requires("worker")
                .value_hint(ValueHint::FilePath)
                .help("Isolated worker Python executable (development override)"),
        )
        .arg(
            Arg::new("worker")
                .long("worker")
                .requires("python")
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
    let mut worker = WorkerOptions::default();
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
    })
}

impl Invocation {
    /// The caller chooses which environment to supply; tests use synthetic maps.
    pub fn load(&self, environment: &BTreeMap<String, String>) -> Result<(ConfigStore, Settings)> {
        let store = ConfigStore::from_environment(environment)?;
        let settings = store.load_with_overrides(environment, &self.overrides)?;
        Ok((store, settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_schema_has_no_conflicts_and_preserves_implicit_timeout() {
        command().debug_assert();
        let parsed = parse(["pdf2zh", "input.pdf", "--openai"]).unwrap();
        assert!(parsed.overrides.get("openai_timeout").is_none());
        let explicit = parse(["pdf2zh", "input.pdf", "--openai-timeout", "1.5"]).unwrap();
        assert_eq!(explicit.overrides["openai_timeout"], "1.5");
    }

    #[test]
    fn bool_presence_matches_python_and_does_not_consume_a_positional_pdf() {
        let parsed = parse([
            "pdf2zh",
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
        let parsed = parse(["pdf2zh", flag.as_str()]).unwrap();
        assert_eq!(
            parsed.overrides[enabled_by_default.flag.replace('-', "_")],
            "false"
        );
    }

    #[test]
    fn help_and_version_do_not_need_input_files_or_configuration() {
        for flag in ["--help", "--version"] {
            let error = match parse(["pdf2zh", flag]) {
                Err(error) => error,
                Ok(_) => panic!("expected display output"),
            };
            assert!(!error.use_stderr());
        }
    }
}
