//! Parented native dialogs; paths are returned as data, never shell commands.
use std::path::{Path, PathBuf};

#[cfg(any(windows, target_os = "macos"))]
pub fn pdfs(parent: &eframe::Frame) -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .set_parent(parent)
        .add_filter("PDF documents", &["pdf"])
        .pick_files()
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn pdfs(_parent: &eframe::Frame) -> Option<Vec<PathBuf>> {
    None
}

#[cfg(any(windows, target_os = "macos"))]
pub fn glossary(parent: &eframe::Frame) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_parent(parent)
        .add_filter("Glossary CSV", &["csv"])
        .pick_file()
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn glossary(_parent: &eframe::Frame) -> Option<PathBuf> {
    None
}

#[cfg(any(windows, target_os = "macos"))]
pub fn save(parent: &eframe::Frame, source: &Path) -> Option<PathBuf> {
    let extension = source.extension()?.to_str()?;
    rfd::FileDialog::new()
        .set_parent(parent)
        .set_file_name(source.file_name()?.to_string_lossy())
        .add_filter("Result", &[extension])
        .save_file()
}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn save(_parent: &eframe::Frame, _source: &Path) -> Option<PathBuf> {
    None
}

pub fn report_startup_error(message: &str) {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let _ = rfd::MessageDialog::new()
            .set_title("BabelOxide")
            .set_description(message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    eprintln!("{message}");
}
