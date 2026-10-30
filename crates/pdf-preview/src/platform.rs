use crate::{PreviewError, PreviewErrorKind};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Resolve only the fixed bundle-relative filename. A PDF directory, CWD, PATH,
/// or system PDFium installation is never a fallback search source.
pub(crate) fn bundled_library(app_root: &Path) -> Result<PathBuf, PreviewError> {
    if !app_root.is_absolute() {
        return Err(PreviewError::new(PreviewErrorKind::InvalidPath));
    }
    let root = app_root
        .canonicalize()
        .map_err(|_| PreviewError::new(PreviewErrorKind::DllMissing))?;
    let directory = root
        .join("native")
        .join("pdfium")
        .canonicalize()
        .map_err(|_| PreviewError::new(PreviewErrorKind::DllMissing))?;
    let library = directory
        .join("pdfium.dll")
        .canonicalize()
        .map_err(|_| PreviewError::new(PreviewErrorKind::DllMissing))?;
    if !directory.starts_with(&root)
        || library.parent() != Some(directory.as_path())
        || !library.is_file()
    {
        return Err(PreviewError::new(PreviewErrorKind::DllInvalid));
    }
    validate_pe_x64(&library)?;
    Ok(library)
}

fn validate_pe_x64(path: &Path) -> Result<(), PreviewError> {
    let invalid = || PreviewError::new(PreviewErrorKind::DllInvalid);
    let mut file = File::open(path).map_err(|_| invalid())?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).map_err(|_| invalid())?;
    if dos[..2] != *b"MZ" {
        return Err(invalid());
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().map_err(|_| invalid())?);
    if u64::from(offset) + 26 > file.metadata().map_err(|_| invalid())?.len() {
        return Err(invalid());
    }
    file.seek(SeekFrom::Start(u64::from(offset)))
        .map_err(|_| invalid())?;
    let mut pe = [0u8; 26];
    file.read_exact(&mut pe).map_err(|_| invalid())?;
    if pe[..4] != *b"PE\0\0"
        || u16::from_le_bytes([pe[4], pe[5]]) != 0x8664
        || u16::from_le_bytes([pe[22], pe[23]]) & 0x2000 == 0
        || u16::from_le_bytes([pe[24], pe[25]]) != 0x20b
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(windows)]
mod windows {
    use crate::{PreviewError, PreviewErrorKind};
    use std::{os::windows::ffi::OsStrExt, path::Path, sync::OnceLock};
    use windows_sys::Win32::System::LibraryLoader::{
        AddDllDirectory, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_LIBRARY_SEARCH_USER_DIRS,
        RemoveDllDirectory, SetDefaultDllDirectories,
    };

    static POLICY: OnceLock<Result<(), PreviewError>> = OnceLock::new();

    /// Process-local hardening is installed before the renderer thread starts.
    /// Windows offers no getter/RAII restoration of the previous policy; it
    /// deliberately remains restrictive for the rest of this process.
    pub(crate) fn configure_search_policy() -> Result<(), PreviewError> {
        *POLICY.get_or_init(|| {
            // SAFETY: constant Windows loader flags; this does not load a DLL.
            if unsafe {
                SetDefaultDllDirectories(
                    LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS,
                )
            } == 0
            {
                Err(PreviewError::new(PreviewErrorKind::DllSearchPolicy))
            } else {
                Ok(())
            }
        })
    }

    pub(crate) struct TrustedSearchDirectory {
        cookie: *mut core::ffi::c_void,
    }
    impl TrustedSearchDirectory {
        pub(crate) fn new(path: &Path) -> Result<Self, PreviewError> {
            configure_search_policy()?;
            if !path.is_absolute() {
                return Err(PreviewError::new(PreviewErrorKind::InvalidPath));
            }
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            if wide.contains(&0) {
                return Err(PreviewError::new(PreviewErrorKind::InvalidPath));
            }
            wide.push(0);
            // SAFETY: `wide` is a live, nul-terminated absolute UTF-16 path.
            let cookie = unsafe { AddDllDirectory(wide.as_ptr()) };
            if cookie.is_null() {
                Err(PreviewError::new(PreviewErrorKind::DllSearchPolicy))
            } else {
                Ok(Self { cookie })
            }
        }
        pub(crate) fn close(&mut self) -> Result<(), PreviewError> {
            if !self.cookie.is_null() {
                // SAFETY: this is the live cookie returned by AddDllDirectory,
                // owned solely by this render-thread guard.
                if unsafe { RemoveDllDirectory(self.cookie) } == 0 {
                    return Err(PreviewError::new(PreviewErrorKind::DllSearchPolicy));
                }
                self.cookie = std::ptr::null_mut();
            }
            Ok(())
        }
    }
    impl Drop for TrustedSearchDirectory {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }
}

#[cfg(windows)]
pub(crate) use windows::{TrustedSearchDirectory, configure_search_policy};

#[cfg(not(windows))]
pub(crate) fn configure_search_policy() -> Result<(), PreviewError> {
    Ok(())
}
#[cfg(not(windows))]
pub(crate) struct TrustedSearchDirectory;
#[cfg(not(windows))]
impl TrustedSearchDirectory {
    pub(crate) fn new(_: &Path) -> Result<Self, PreviewError> {
        Err(PreviewError::new(PreviewErrorKind::NativeLoad))
    }
    pub(crate) fn close(&mut self) -> Result<(), PreviewError> {
        Ok(())
    }
}
