//! Explicit, ignored-by-default ABI test. The parent runner supplies a verified
//! PDFium bundle and an authorized real PDF; there is no system-library fallback.
use pdf2zh_pdf_preview::{
    PreviewErrorKind, PreviewFrame, PreviewRenderer, PreviewRequest, PreviewResult,
};
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const LIMIT: Duration = Duration::from_secs(30);

fn render(renderer: &PreviewRenderer, path: &Path, page: u32, generation: u64) -> PreviewResult {
    let mut request = PreviewRequest::new(path.into(), page);
    request.document_key = "native-smoke".into();
    request.generation = generation;
    let deadline = Instant::now() + LIMIT;
    let ticket = loop {
        match renderer.request(request.clone()) {
            Ok(ticket) => break ticket,
            Err(error) if error.kind == PreviewErrorKind::Busy => {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            Err(error) => panic!("native request failed: {error}"),
        }
    };
    loop {
        if let Some(result) = renderer.try_take_result() {
            if let Some(actual) = result.ticket() {
                assert_eq!(actual, ticket);
            }
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "native render timed out (external runner must terminate a hung native process)"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn expect_frame(result: PreviewResult) -> PreviewFrame {
    let PreviewResult::Frame(frame) = result else {
        panic!("native renderer failed: {result:?}");
    };
    assert!((1..=1600).contains(&frame.width));
    assert!((1..=1600).contains(&frame.height));
    assert_eq!(
        frame.rgba.len(),
        frame.width as usize * frame.height as usize * 4
    );
    assert!(
        frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    frame
}

fn write_synthetic(path: &Path) {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 4 /Kids [3 0 R 5 0 R 7 0 R 9 0 R] >>".to_vec(),
    ];
    for (index, box_size, extra, content) in [
        (0, "300 600", "", "1 0 0 rg 0 0 300 600 re f\n"),
        (1, "600 300", "", "0 1 0 rg 0 0 600 300 re f\n"),
        (2, "300 600", "/Rotate 90", "0 0 1 rg 0 0 300 600 re f\n"),
        (
            3,
            "300 600",
            "/CropBox [0 0 100 200]",
            "1 0 1 rg 20 20 60 160 re f\n",
        ),
    ] {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {box_size}] {extra} /Resources << >> /Contents {} 0 R >>",4+index*2).into_bytes());
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            )
            .into_bytes(),
        );
    }
    let mut pdf = b"%PDF-1.7\n%synthetic-native-preview\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    fs::write(path, pdf).unwrap();
}

fn save_ppm(directory: Option<&Path>, name: &str, frame: &PreviewFrame) {
    let Some(directory) = directory else {
        return;
    };
    fs::create_dir_all(directory).unwrap();
    let mut stream =
        BufWriter::new(fs::File::create(directory.join(format!("{name}.ppm"))).unwrap());
    write!(stream, "P6\n{} {}\n255\n", frame.width, frame.height).unwrap();
    let mut row = Vec::with_capacity(frame.width as usize * 3);
    for pixels in frame.rgba.chunks_exact(frame.width as usize * 4) {
        row.clear();
        for pixel in pixels.as_chunks::<4>().0 {
            row.extend_from_slice(&pixel[..3]);
        }
        stream.write_all(&row).unwrap();
    }
    stream.flush().unwrap();
}

#[test]
#[ignore = "requires explicit verified PDF2ZH_PREVIEW_NATIVE_APP_ROOT and authorized PDF2ZH_PREVIEW_NATIVE_PDF"]
fn verified_bundle_renders_real_pdf_geometry_rgba_errors_and_releases_native_file_handles() {
    let app = PathBuf::from(
        std::env::var_os("PDF2ZH_PREVIEW_NATIVE_APP_ROOT")
            .expect("supply the verified bundle root"),
    );
    let source = PathBuf::from(
        std::env::var_os("PDF2ZH_PREVIEW_NATIVE_PDF").expect("supply the authorized real PDF"),
    );
    let artifacts = std::env::var_os("PDF2ZH_PREVIEW_ARTIFACT_DIR").map(PathBuf::from);
    assert!(app.is_absolute());
    assert!(source.is_file());
    if let Some(path) = &artifacts {
        assert!(path.is_absolute());
    }
    let temp = tempfile::tempdir().unwrap();
    let synthetic = temp.path().join("原生 測試.pdf");
    write_synthetic(&synthetic);
    let mut renderer = PreviewRenderer::new(app.clone()).unwrap();
    for (page, width, height, center) in [
        (1, 600, 1200, [255, 0, 0, 255]),
        (2, 1200, 600, [0, 255, 0, 255]),
        (3, 1200, 600, [0, 0, 255, 255]),
        (4, 200, 400, [255, 0, 255, 255]),
    ] {
        let frame = expect_frame(render(&renderer, &synthetic, page, u64::from(page)));
        assert_eq!((frame.width, frame.height), (width, height));
        assert_eq!(frame.page_count, 4);
        let offset = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
        assert_eq!(&frame.rgba[offset..offset + 4], &center);
        if page == 4 {
            assert_eq!(&frame.rgba[..4], &[255, 255, 255, 255]);
        }
        save_ppm(artifacts.as_deref(), &format!("synthetic-{page}"), &frame);
    }
    let missing = temp.path().join("missing.pdf");
    let invalid = temp.path().join("invalid.pdf");
    fs::write(&invalid, b"synthetic not a PDF").unwrap();
    for (path, kind) in [
        (&missing, PreviewErrorKind::FileMissing),
        (&invalid, PreviewErrorKind::InvalidPdf),
    ] {
        let PreviewResult::Error(failure) = render(&renderer, path, 1, 10) else {
            panic!("invalid document succeeded");
        };
        assert_eq!(failure.error.kind, kind);
    }
    let copy = temp.path().join("真實 PDF 中文 副本.pdf");
    fs::copy(source, &copy).unwrap();
    let first = expect_frame(render(&renderer, &copy, 0, 20));
    assert_eq!(first.page_one_based, 1);
    let count = first.page_count;
    save_ppm(artifacts.as_deref(), "real-1", &first);
    drop(first);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Rename alone is insufficient: a shared-delete reader may stay open.
        // A no-sharing handle proves no native-owned File reader remains.
        let exclusive = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&copy)
            .unwrap();
        drop(exclusive);
    }
    let moved = temp.path().join("已釋放 PDF.pdf");
    fs::rename(&copy, &moved).unwrap();
    for page in 2..=count {
        let frame = expect_frame(render(&renderer, &moved, page, 20 + u64::from(page)));
        assert_eq!(frame.page_one_based, page);
        save_ppm(artifacts.as_deref(), &format!("real-{page}"), &frame);
    }
    let last = expect_frame(render(&renderer, &moved, u32::MAX, 100));
    assert_eq!(last.page_one_based, count);
    drop(last);
    renderer.close().unwrap();
    fs::remove_file(moved).unwrap();
    let mut recreated = PreviewRenderer::new(app).unwrap();
    let deadline = Instant::now() + LIMIT;
    loop {
        if let Some(PreviewResult::Error(failure)) = recreated.try_take_result() {
            assert_eq!(failure.error.kind, PreviewErrorKind::AlreadyInitialized);
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        recreated.close().unwrap_err().kind,
        PreviewErrorKind::AlreadyInitialized
    );
}
