//! Native desktop application. Loading this crate performs no configuration or
//! worker I/O; the explicit launch entry point owns those operations.

mod app;
mod background;
mod dialogs;
mod languages;
mod model;
mod output_paths;
mod presets;
mod settings_ui;
mod strings;
mod transfers;
#[cfg(windows)]
mod windows_launch;

use anyhow::{Context, Result, ensure};
use babeloxide_bridge::WorkerOptions;
use babeloxide_cli::Invocation;
use babeloxide_config::{ConfigStore, Settings};
use std::{collections::BTreeMap, path::PathBuf};

/// Explicit dependencies for embedders and isolated tests. Construction does
/// not inspect environment variables or read the user's configuration.
/// Deliberately has no Debug implementation: Settings can contain credentials.
pub struct LaunchOptions {
    pub store: ConfigStore,
    pub settings: Settings,
    pub worker: WorkerOptions,
    pub cache_file: Option<PathBuf>,
    /// Directory containing the executable and its trusted native/ subtree.
    /// This is independent of a writable BABELOXIDE_APP_DIR configuration override.
    pub bundle_root: PathBuf,
}

/// Load the shared CLI configuration with the caller's explicit environment,
/// then launch the desktop UI. No translation or Python process starts here.
pub fn launch(invocation: Invocation, environment: BTreeMap<String, String>) -> Result<()> {
    #[cfg(windows)]
    if windows_launch::relaunch_if_needed()? {
        return Ok(());
    }
    let (store, settings) = invocation.load(&environment)?;
    if invocation.save_config {
        store.save_user_defaults(&settings)?;
    }
    let executable = std::env::current_exe().context("Cannot locate the application executable")?;
    let bundle_root = invocation.runtime_dir.clone().unwrap_or(
        executable
            .parent()
            .context("Cannot locate the application folder")?
            .to_path_buf(),
    );
    let worker = invocation.worker_for_store(&store);
    launch_with_settings(LaunchOptions {
        store,
        settings,
        worker,
        cache_file: invocation.cache_file,
        bundle_root,
    })
}

pub fn launch_with_settings(options: LaunchOptions) -> Result<()> {
    ensure!(
        options.bundle_root.is_absolute(),
        "The application bundle folder must be absolute"
    );
    options.settings.validate()?;
    let (background, thread) = background::Background::start(
        options.store.clone(),
        options.worker.clone(),
        options.cache_file.clone(),
    )?;
    let control = background.control.clone();
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!(
                "{} v{}",
                babeloxide_core::PRODUCT_NAME,
                babeloxide_core::PRODUCT_VERSION
            ))
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../../resources/babeloxide.png"
                ))
                .expect("Bundled BabelOxide icon"),
            )
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([640.0, 520.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let result = eframe::run_native(
        babeloxide_core::PRODUCT_NAME,
        native_options,
        Box::new(move |context| Ok(Box::new(app::NativeApp::new(context, options, background)))),
    );
    // The event loop has ended. Joining here cannot block egui rendering, and
    // also covers failures before the App creator was invoked.
    control.close(None);
    thread.finish();
    result.map_err(|error| anyhow::anyhow!("Cannot run the desktop window: {error}"))
}

/// Native startup failures remain visible for the Windows subsystem binary.
pub fn report_startup_error(message: &str) {
    dialogs::report_startup_error(message);
}
