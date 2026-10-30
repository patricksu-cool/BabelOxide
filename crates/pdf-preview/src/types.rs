use std::{fmt, path::PathBuf};

pub const MAX_RENDER_EDGE: u32 = 1600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreviewTicket(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewRequest {
    /// Include output identity/generation, not just the reusable filename.
    pub document_key: String,
    pub generation: u64,
    pub path: PathBuf,
    pub page_one_based: u32,
    pub zoom: f32,
    pub max_width: u32,
    pub max_height: u32,
}

impl PreviewRequest {
    pub fn new(path: PathBuf, page_one_based: u32) -> Self {
        Self {
            document_key: String::new(),
            generation: 0,
            path,
            page_one_based,
            zoom: 2.0,
            max_width: MAX_RENDER_EDGE,
            max_height: MAX_RENDER_EDGE,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), PreviewError> {
        if !self.path.is_absolute() {
            return Err(PreviewError::new(PreviewErrorKind::InvalidPath));
        }
        if !self.zoom.is_finite() || self.zoom <= 0.0 || self.max_width == 0 || self.max_height == 0
        {
            return Err(PreviewError::new(PreviewErrorKind::InvalidBounds));
        }
        Ok(())
    }
}

pub struct PreviewFrame {
    pub ticket: PreviewTicket,
    pub document_key: String,
    pub generation: u64,
    pub path: PathBuf,
    pub page_one_based: u32,
    pub page_count: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl fmt::Debug for PreviewFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewFrame")
            .field("ticket", &self.ticket)
            .field("generation", &self.generation)
            .field("page_one_based", &self.page_one_based)
            .field("page_count", &self.page_count)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgba_bytes", &self.rgba.len())
            .finish_non_exhaustive()
    }
}

impl PreviewFrame {
    pub(crate) fn validate(&self) -> Result<(), PreviewError> {
        let length = self
            .width
            .checked_mul(self.height)
            .and_then(|v| v.checked_mul(4))
            .and_then(|v| usize::try_from(v).ok());
        if self.width == 0
            || self.height == 0
            || self.width > MAX_RENDER_EDGE
            || self.height > MAX_RENDER_EDGE
            || self.page_count == 0
            || self.page_one_based == 0
            || self.page_one_based > self.page_count
            || length != Some(self.rgba.len())
            || self
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 255)
        {
            return Err(PreviewError::new(PreviewErrorKind::InvalidPixels));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PreviewFailure {
    /// None denotes initialization/thread failure, independent of any PDF.
    pub ticket: Option<PreviewTicket>,
    pub request: Option<PreviewRequest>,
    pub error: PreviewError,
}

#[derive(Debug)]
pub enum PreviewResult {
    Frame(PreviewFrame),
    Error(PreviewFailure),
}
impl PreviewResult {
    pub fn ticket(&self) -> Option<PreviewTicket> {
        match self {
            Self::Frame(frame) => Some(frame.ticket),
            Self::Error(failure) => failure.ticket,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewErrorKind {
    InvalidPath,
    InvalidBounds,
    Busy,
    Closed,
    TicketExhausted,
    DllMissing,
    DllInvalid,
    DllSearchPolicy,
    NativeLoad,
    AlreadyInitialized,
    FileMissing,
    InvalidPdf,
    EncryptedPdf,
    EmptyDocument,
    InvalidPageSize,
    RenderFailed,
    InvalidPixels,
    ThreadFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewError {
    pub kind: PreviewErrorKind,
}
impl PreviewError {
    pub const fn new(kind: PreviewErrorKind) -> Self {
        Self { kind }
    }
}
impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            PreviewErrorKind::InvalidPath=>"Preview requires an absolute path",
            PreviewErrorKind::InvalidBounds=>"Preview zoom and bounds must be positive and finite",
            PreviewErrorKind::Busy=>"Preview request slot is briefly busy; retry the latest request",
            PreviewErrorKind::Closed=>"Preview renderer has closed",
            PreviewErrorKind::TicketExhausted=>"Preview generation counter is exhausted",
            PreviewErrorKind::DllMissing=>"Bundled PDFium library is missing",
            PreviewErrorKind::DllInvalid=>"Bundled PDFium library is not a Windows x64 DLL",
            PreviewErrorKind::DllSearchPolicy=>"Cannot configure the trusted PDFium dependency search directory",
            PreviewErrorKind::NativeLoad=>"Cannot bind bundled PDFium; its architecture, exports, or dependencies are incompatible",
            PreviewErrorKind::AlreadyInitialized=>"PDFium was already initialized in this process; restart to replace or recreate the native renderer",
            PreviewErrorKind::FileMissing=>"PDF file is unavailable",
            PreviewErrorKind::InvalidPdf=>"PDF is invalid or cannot be opened",
            PreviewErrorKind::EncryptedPdf=>"PDF requires a password",
            PreviewErrorKind::EmptyDocument=>"PDF contains no pages",
            PreviewErrorKind::InvalidPageSize=>"PDF page dimensions are invalid",
            PreviewErrorKind::RenderFailed=>"PDFium could not render this page",
            PreviewErrorKind::InvalidPixels=>"Preview bitmap dimensions, format, or alpha are invalid",
            PreviewErrorKind::ThreadFailed=>"Preview render thread failed",
        })
    }
}
impl std::error::Error for PreviewError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderSize {
    pub width: u32,
    pub height: u32,
}

/// Aspect ratio is rounded once to the nearest pixel; it is never stretched to
/// fill a panel. Bounds and the original preview's 2x zoom/1600 edge are caps.
pub fn render_size(
    width: f64,
    height: f64,
    zoom: f32,
    max_width: u32,
    max_height: u32,
) -> Result<RenderSize, PreviewError> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(PreviewError::new(PreviewErrorKind::InvalidPageSize));
    }
    if !zoom.is_finite() || zoom <= 0.0 || max_width == 0 || max_height == 0 {
        return Err(PreviewError::new(PreviewErrorKind::InvalidBounds));
    }
    let max_width = max_width.min(MAX_RENDER_EDGE);
    let max_height = max_height.min(MAX_RENDER_EDGE);
    let scale = f64::from(zoom.min(2.0))
        .min(f64::from(max_width) / width)
        .min(f64::from(max_height) / height);
    Ok(RenderSize {
        width: (width * scale).round().clamp(1.0, f64::from(max_width)) as u32,
        height: (height * scale).round().clamp(1.0, f64::from(max_height)) as u32,
    })
}

pub(crate) fn clamp_page(requested: u32, count: u32) -> Result<u32, PreviewError> {
    if count == 0 {
        Err(PreviewError::new(PreviewErrorKind::EmptyDocument))
    } else {
        Ok(requested.clamp(1, count))
    }
}
