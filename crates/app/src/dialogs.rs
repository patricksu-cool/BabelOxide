//! Parented native dialogs; paths are returned as data, never shell commands.
use std::path::{Path, PathBuf};

#[cfg(windows)]
pub fn pdfs(parent: &eframe::Frame) -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .set_parent(parent)
        .add_filter("PDF documents", &["pdf"])
        .pick_files()
}
#[cfg(not(windows))]
pub fn pdfs(_parent: &eframe::Frame) -> Option<Vec<PathBuf>> {
    None
}

#[cfg(windows)]
pub fn glossary(parent: &eframe::Frame) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_parent(parent)
        .add_filter("Glossary CSV", &["csv"])
        .pick_file()
}
#[cfg(not(windows))]
pub fn glossary(_parent: &eframe::Frame) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
pub fn folder(parent: &eframe::Frame) -> Option<PathBuf> {
    rfd::FileDialog::new().set_parent(parent).pick_folder()
}
#[cfg(not(windows))]
pub fn folder(_parent: &eframe::Frame) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
pub fn save(parent: &eframe::Frame, source: &Path) -> Option<PathBuf> {
    let extension = source.extension()?.to_str()?;
    rfd::FileDialog::new()
        .set_parent(parent)
        .set_file_name(source.file_name()?.to_string_lossy())
        .add_filter("Result", &[extension])
        .save_file()
}
#[cfg(not(windows))]
pub fn save(_parent: &eframe::Frame, _source: &Path) -> Option<PathBuf> {
    None
}

pub fn report_startup_error(message: &str) {
    #[cfg(windows)]
    {
        let _ = rfd::MessageDialog::new()
            .set_title("PDFMathTranslate")
            .set_description(message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    #[cfg(not(windows))]
    eprintln!("{message}");
}
