#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{collections::BTreeMap, process::ExitCode};

fn main() -> ExitCode {
    let invocation = match babeloxide_cli::parse(std::env::args_os()) {
        Ok(invocation) => invocation,
        Err(error) => {
            if error.use_stderr() {
                babeloxide_app::report_startup_error(
                    "Cannot read the launch options. Run BabelOxide --help to see supported options.",
                );
                return ExitCode::from(2);
            }
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
    };
    let environment: BTreeMap<String, String> = std::env::vars().collect();
    match babeloxide_app::launch(invocation, environment) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            babeloxide_app::report_startup_error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}
