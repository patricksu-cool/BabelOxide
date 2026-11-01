use super::*;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

#[test]
fn portrait_landscape_zoom_and_panel_bounds_preserve_aspect_ratio_and_pixel_cap() {
    for (width, height, zoom, bw, bh, expected) in [
        (612.0, 792.0, 2.0, 1600, 1600, (1224, 1584)),
        (792.0, 612.0, 2.0, 1600, 1600, (1584, 1224)),
        (2000.0, 1000.0, 2.0, 1600, 1600, (1600, 800)),
        (400.0, 1000.0, 2.0, 300, 200, (80, 200)),
        (100.0, 50.0, 20.0, 9999, 9999, (200, 100)),
        (612.0, 792.0, 0.5, 1600, 1600, (306, 396)),
    ] {
        let size = render_size(width, height, zoom, bw, bh).unwrap();
        assert_eq!((size.width, size.height), expected);
        assert!(size.width <= MAX_RENDER_EDGE && size.height <= MAX_RENDER_EDGE);
        assert!((f64::from(size.width) / width - f64::from(size.height) / height).abs() < 0.01);
    }
}

#[test]
fn invalid_sizes_and_requests_fail_before_allocation_and_page_bounds_are_clamped() {
    for (width, height) in [
        (0.0, 10.0),
        (-1.0, 1.0),
        (f64::NAN, 1.0),
        (f64::INFINITY, 1.0),
    ] {
        assert_eq!(
            render_size(width, height, 2.0, 1600, 1600)
                .unwrap_err()
                .kind,
            PreviewErrorKind::InvalidPageSize
        );
    }
    for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            render_size(10.0, 10.0, zoom, 1600, 1600).unwrap_err().kind,
            PreviewErrorKind::InvalidBounds
        );
    }
    assert!(render_size(10.0, 10.0, 1.0, 0, 100).is_err());
    assert_eq!(crate::types::clamp_page(0, 4).unwrap(), 1);
    assert_eq!(crate::types::clamp_page(u32::MAX, 4).unwrap(), 4);
    assert_eq!(
        crate::types::clamp_page(1, 0).unwrap_err().kind,
        PreviewErrorKind::EmptyDocument
    );
    assert_eq!(
        PreviewRequest::new(PathBuf::from("relative.pdf"), 1)
            .validate()
            .unwrap_err()
            .kind,
        PreviewErrorKind::InvalidPath
    );
}

#[test]
fn rgba_validation_checks_dimensions_byte_length_and_opaque_alpha() {
    let directory = TempDir::new().unwrap();
    let mut frame = PreviewFrame {
        ticket: PreviewTicket(1),
        document_key: "test".into(),
        generation: 1,
        path: directory.path().join("synthetic.pdf"),
        page_one_based: 1,
        page_count: 1,
        width: 1,
        height: 1,
        rgba: vec![255, 255, 255, 255],
    };
    frame.validate().unwrap();
    frame.rgba.pop();
    assert!(frame.validate().is_err());
    frame.rgba = vec![1, 2, 3, 0];
    assert!(frame.validate().is_err());
    frame.rgba = vec![1, 2, 3, 255];
    frame.width = 1601;
    assert!(frame.validate().is_err());
}

fn synthetic_pe(machine: u16, dll: bool) -> Vec<u8> {
    let mut bytes = vec![0u8; 128];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&machine.to_le_bytes());
    bytes[86..88].copy_from_slice(&(if dll { 0x2000u16 } else { 0 }).to_le_bytes());
    bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
    bytes
}

#[test]
fn bundled_loader_uses_only_fixed_absolute_path_and_rejects_missing_invalid_or_wrong_architecture()
{
    let root = TempDir::new().unwrap();
    assert_eq!(
        crate::platform::bundled_library(root.path())
            .unwrap_err()
            .kind,
        PreviewErrorKind::DllMissing
    );
    let native = root.path().join("native/pdfium");
    fs::create_dir_all(&native).unwrap();
    let dll = native.join(crate::platform::LIBRARY_FILENAME);
    let native_image = |machine: u16, dll: bool| {
        if cfg!(target_os = "macos") {
            let mut data = vec![0u8; 32];
            data[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
            data[4..8].copy_from_slice(
                &(if machine == 0x8664 {
                    0x0100000cu32
                } else {
                    0x01000007u32
                })
                .to_le_bytes(),
            );
            data[12..16].copy_from_slice(&(if dll { 6u32 } else { 2u32 }).to_le_bytes());
            data
        } else {
            synthetic_pe(machine, dll)
        }
    };
    for data in [
        b"not a DLL".to_vec(),
        native_image(0x14c, true),
        native_image(0x8664, false),
    ] {
        fs::write(&dll, data).unwrap();
        assert_eq!(
            crate::platform::bundled_library(root.path())
                .unwrap_err()
                .kind,
            PreviewErrorKind::DllInvalid
        );
    }
    fs::write(&dll, native_image(0x8664, true)).unwrap();
    assert_eq!(
        crate::platform::bundled_library(root.path()).unwrap(),
        dll.canonicalize().unwrap()
    );
    assert_eq!(
        crate::platform::bundled_library(&PathBuf::from("."))
            .unwrap_err()
            .kind,
        PreviewErrorKind::InvalidPath
    );
}

#[test]
fn document_load_error_mapping_distinguishes_missing_invalid_and_password_errors() {
    use pdfium_render::prelude::{PdfiumError, PdfiumInternalError};
    assert_eq!(
        crate::native::document_error(PdfiumError::IoError(std::io::Error::from(
            std::io::ErrorKind::NotFound
        )))
        .kind,
        PreviewErrorKind::FileMissing
    );
    assert_eq!(
        crate::native::document_error(PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::FormatError
        ))
        .kind,
        PreviewErrorKind::InvalidPdf
    );
    assert_eq!(
        crate::native::document_error(PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::PasswordError
        ))
        .kind,
        PreviewErrorKind::EncryptedPdf
    );
}
