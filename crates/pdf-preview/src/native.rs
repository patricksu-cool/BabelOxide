use crate::{
    MAX_RENDER_EDGE, PreviewError, PreviewErrorKind, PreviewFrame, PreviewRequest, PreviewTicket,
    render_size,
};
use crate::{
    platform::{TrustedSearchDirectory, bundled_library},
    queue::RenderBackend,
    types::clamp_page,
};
use pdfium_render::prelude::{
    PdfBitmapFormat, PdfColor, PdfRenderConfig, Pdfium, PdfiumError, PdfiumInternalError,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicBool = AtomicBool::new(false);

struct Reservation;
impl Reservation {
    fn acquire() -> Result<Self, PreviewError> {
        if INITIALIZED.load(Ordering::Acquire)
            || ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(PreviewError::new(PreviewErrorKind::AlreadyInitialized));
        }
        // Protect against a concurrent successful initializer between checks.
        if INITIALIZED.load(Ordering::Acquire) {
            ACTIVE.store(false, Ordering::Release);
            return Err(PreviewError::new(PreviewErrorKind::AlreadyInitialized));
        }
        Ok(Self)
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

/// Created and destroyed on the render thread. The wrapper's global bindings
/// remain installed until process exit; a second native renderer is rejected.
struct NativeBackend {
    pdfium: Option<Pdfium>,
    search: TrustedSearchDirectory,
    _reservation: Reservation,
}

pub(crate) fn create(app_root: PathBuf) -> Result<Box<dyn RenderBackend>, PreviewError> {
    let library = bundled_library(&app_root)?;
    let reservation = Reservation::acquire()?;
    let search = TrustedSearchDirectory::new(
        library
            .parent()
            .ok_or_else(|| PreviewError::new(PreviewErrorKind::DllInvalid))?,
    )?;
    let bindings = Pdfium::bind_to_library(&library).map_err(|error| {
        PreviewError::new(match error {
            PdfiumError::PdfiumLibraryBindingsAlreadyInitialized => {
                PreviewErrorKind::AlreadyInitialized
            }
            _ => PreviewErrorKind::NativeLoad,
        })
    })?;
    let pdfium = Pdfium::new(bindings);
    INITIALIZED.store(true, Ordering::Release);
    Ok(Box::new(NativeBackend {
        pdfium: Some(pdfium),
        search,
        _reservation: reservation,
    }))
}

impl RenderBackend for NativeBackend {
    fn render(
        &mut self,
        request: &PreviewRequest,
        ticket: PreviewTicket,
    ) -> Result<PreviewFrame, PreviewError> {
        let pdfium = self
            .pdfium
            .as_ref()
            .ok_or_else(|| PreviewError::new(PreviewErrorKind::Closed))?;
        // load_pdf_from_file uses Rust File/PdfiumReader; preserve native UTF-16
        // paths rather than converting filenames through a lossy ANSI string.
        let document = pdfium
            .load_pdf_from_file(&request.path, None)
            .map_err(document_error)?;
        let count = u32::try_from(document.pages().len())
            .map_err(|_| PreviewError::new(PreviewErrorKind::EmptyDocument))?;
        let number = clamp_page(request.page_one_based, count)?;
        let page = document
            .pages()
            .get((number - 1) as i32)
            .map_err(|_| PreviewError::new(PreviewErrorKind::RenderFailed))?;
        let width = f64::from(page.width().value);
        let height = f64::from(page.height().value);
        render_size(
            width,
            height,
            request.zoom,
            request.max_width,
            request.max_height,
        )?;
        // A single uniform scale preserves page aspect ratio, including native
        // page rotation. Do not auto-rotate landscapes or disable annotations
        // and smoothing through the thumbnail configuration.
        let config = PdfRenderConfig::new()
            .scale_page_by_factor(request.zoom.min(2.0))
            .set_maximum_width(request.max_width.min(MAX_RENDER_EDGE) as i32)
            .set_maximum_height(request.max_height.min(MAX_RENDER_EDGE) as i32)
            .set_format(PdfBitmapFormat::BGRA)
            .clear_before_rendering(true)
            .set_clear_color(PdfColor::WHITE)
            .render_annotations(true)
            .render_form_data(true);
        let bitmap = page
            .render_with_config(&config)
            .map_err(|_| PreviewError::new(PreviewErrorKind::RenderFailed))?;
        let width = u32::try_from(bitmap.width())
            .map_err(|_| PreviewError::new(PreviewErrorKind::InvalidPixels))?;
        let height = u32::try_from(bitmap.height())
            .map_err(|_| PreviewError::new(PreviewErrorKind::InvalidPixels))?;
        if width == 0 || height == 0 || width > MAX_RENDER_EDGE || height > MAX_RENDER_EDGE {
            return Err(PreviewError::new(PreviewErrorKind::InvalidPixels));
        }
        let rgba = bitmap.as_rgba_bytes();
        // Close the native bitmap, page, document and owned file reader before
        // any result becomes visible. No long-lived Windows PDF file lock.
        drop(bitmap);
        drop(page);
        drop(document);
        let frame = PreviewFrame {
            ticket,
            document_key: request.document_key.clone(),
            generation: request.generation,
            path: request.path.clone(),
            page_one_based: number,
            page_count: count,
            width,
            height,
            rgba,
        };
        frame.validate()?;
        Ok(frame)
    }
    fn close(&mut self) -> Result<(), PreviewError> {
        drop(self.pdfium.take());
        self.search.close()
    }
}
impl Drop for NativeBackend {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

pub(crate) fn document_error(error: PdfiumError) -> PreviewError {
    PreviewError::new(match error {
        PdfiumError::IoError(_)
        | PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FileError) => {
            PreviewErrorKind::FileMissing
        }
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::PasswordError | PdfiumInternalError::SecurityError,
        ) => PreviewErrorKind::EncryptedPdf,
        _ => PreviewErrorKind::InvalidPdf,
    })
}
