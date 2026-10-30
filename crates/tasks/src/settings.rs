//! The GUI and CLI construct the same services and PDF batch from settings.

use crate::TaskSpec;
use anyhow::{Result, ensure};
use babeldoc_bridge::WorkerOptions;
use pdf2zh_config::Settings;
use pdf2zh_core::{CacheBackend, TranslationService, TranslatorConfig};
use std::sync::Arc;

fn same_service(left: &TranslatorConfig, right: &TranslatorConfig) -> bool {
    left.provider == right.provider
        && left.endpoint == right.endpoint
        && left.api_key == right.api_key
        && left.model == right.model
        && left.timeout_seconds == right.timeout_seconds
        && left.lang_out == right.lang_out
        && left.options == right.options
}

pub fn task_from_settings(
    settings: &Settings,
    worker: WorkerOptions,
    cache: Option<Arc<dyn CacheBackend>>,
) -> Result<TaskSpec> {
    settings.validate()?;
    let basic = settings.basic()?;
    ensure!(
        !basic.input_files.is_empty(),
        "Choose at least one input PDF"
    );
    let main = settings.main_translator_config()?;
    let term = settings.term_translator_config()?;
    let separate_term = term.filter(|term| !same_service(&main, term));
    let make_service = |config| -> Result<_> {
        let service = TranslationService::new(config)?;
        Ok(Arc::new(if let Some(cache) = &cache {
            service.with_cache(cache.clone())
        } else {
            service
        }))
    };
    let translator = make_service(main)?;
    let term_translator = separate_term.map(make_service).transpose()?;
    let jobs = basic
        .input_files
        .into_iter()
        .map(|input| settings.pdf_job(input.into()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(TaskSpec {
        worker,
        jobs,
        translator,
        term_translator,
    })
}
