//! Windows COM/OLE requires a short process image path for the desktop window.

use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, ensure};
use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

const RELAUNCHED: &str = "BABELOXIDE_SHORT_GUI_LAUNCH";
const PATH_ERROR: &str = "Windows GUI needs a shorter executable path. Move the complete application folder to a shorter location; no system policy change is required.";

fn short_path(path: &Path) -> Result<PathBuf> {
    let full = path.canonicalize().context(PATH_ERROR)?;
    let mut input: Vec<u16> = full.as_os_str().encode_wide().collect();
    input.push(0);
    let mut output = vec![0u16; 32768];
    // The canonical, NUL-terminated input and output buffer remain valid here.
    let length =
        unsafe { GetShortPathNameW(input.as_ptr(), output.as_mut_ptr(), output.len() as u32) }
            as usize;
    ensure!(length > 0 && length < 260, PATH_ERROR);
    Ok(PathBuf::from(OsString::from_wide(&output[..length])))
}

pub(super) fn relaunch_if_needed() -> Result<bool> {
    let executable = std::env::current_exe().context("Locate GUI executable")?;
    if executable.as_os_str().encode_wide().count() < 260 {
        return Ok(false);
    }
    // Guard against a loader which resolves the alias back to the long spelling.
    ensure!(std::env::var_os(RELAUNCHED).is_none(), PATH_ERROR);
    let executable = short_path(&executable)?;
    let mut directory = std::env::current_dir().context("Locate GUI working directory")?;
    if directory.as_os_str().encode_wide().count() >= 260 {
        directory = short_path(&directory)?;
    }
    let status = Command::new(executable)
        .args(std::env::args_os().skip(1))
        .current_dir(directory)
        .env(RELAUNCHED, "1")
        .status()
        .context("Start GUI through its existing Windows short path")?;
    // Await the desktop child so launchers keep their normal close/exit contract.
    ensure!(status.success(), "The desktop window exited unsuccessfully");
    Ok(true)
}
