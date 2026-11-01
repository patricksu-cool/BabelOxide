# Native PDF preview

`babeloxide-pdf-preview` renders one PDF page on a dedicated Rust thread. The GUI
receives owned RGBA bytes and metadata; it never receives PDFium objects, bitmap
pointers, or file readers. Preview does not use Python, BabelDOC, or a browser.

The fixed wrapper is `pdfium-render = 0.9.4`, without default/image features,
with `thread_safe` and `pdfium_7881`. The parent build owns acquisition, hashes,
licenses, and ABI validation of the bundled PDFium 8125 library (Windows x64 DLL
or macOS arm64 dylib). Header checks here do not replace package verification.

## GUI contract

Create one `PreviewRenderer` from an absolute application root, preferably before
starting other application loader work. `new_with_wakeup(root, callback)` can use
an owned egui context's `request_repaint` callback. Construction starts a thread;
that thread binds PDFium. Initialization errors are delivered as
`PreviewResult::Error` with `ticket=None` and `request=None`; the rest of the GUI
can remain available. Loader-policy or thread-start failure can instead fail the
constructor directly.

`PreviewRequest::new(absolute_pdf_path, page_one_based)` defaults to zoom2 and
1600×1600 bounds. Set `document_key` and `generation` to the UI's output identity;
the same path may have been replaced by a different output. `request()` uses
`try_lock`, returns a monotonically increasing `PreviewTicket`, and never waits
for rendering or a busy slot. On `Busy`, retain only the newest UI request and
retry on a later frame; no ticket was consumed. File access happens on the render
thread, not in `request()`.

There is at most one native render, one queued request, and one queued result.
Accepted requests replace queued work. The renderer drops stale tickets before
publication and again in `try_take_result()`. The GUI must also compare the
returned ticket, document key, and generation against its own current request.
`try_take_result()` is nonblocking and returns either `Frame(PreviewFrame)` or
`Error(PreviewFailure)`.

Frames contain ticket, document key, generation, original `PathBuf`, clamped
one-based page, page count, width, height, and `rgba: Vec<u8>`. Check these fields
before updating a texture. Native rendering uses a uniform scale, at most2×,
with both edges capped at1600 and the caller's panel bounds. It keeps natural
landscape/intrinsic page rotation, clears an opaque white BGRA bitmap, renders
annotations/form data, and converts through `as_rgba_bytes()`. It does not use a
thumbnail configuration or auto-rotate landscape pages. Dimensions, checked
byte length, and opaque alpha are validated before a frame reaches the GUI.

The 1600²×4=10,240,000-byte RGBA cap describes one frame. Native bitmap, the RGBA
copy, a previous result, GUI ColorImage, and GPU textures may coexist. The GUI
must bound its own texture slots and drop old handles; this crate does not claim
that total preview memory is10MB.

## Loader and lifetime

Only `app_root/native/pdfium/pdfium.dll` on Windows, or
`app_root/native/pdfium/libpdfium.dylib` on macOS, is accepted. Paths are absolute
and canonicalized within the trusted application root. Windows requires an
AMD64 PE32+ DLL header; macOS requires a thin arm64 Mach-O dylib header. The Mac
builder separately checks its dependencies and rpaths. There is no `Pdfium::default()`, system-library, PATH, or
CWD fallback. PDF paths are kept as `PathBuf`; wrapper file loading uses Rust's
File reader, including Unicode Windows paths.

Windows installs the process-local
`SetDefaultDllDirectories(SYSTEM32 | USER_DIRS)` policy before thread creation.
`AddDllDirectory` adds the canonical trusted PDFium directory with an owned
cookie; it never adds the selected PDF directory. Setup failures are checked.
The guard removes its cookie after native work ends, with a checked close and a
Drop fallback. Windows exposes no prior-policy getter/restoration contract;
the restrictive default policy deliberately remains until process exit. No
machine PATH or registry is changed.

PDFium bindings in wrapper0.9.4 use a process-global OnceCell. Successful native
initialization is therefore allowed once per process; attempting to create a
second native renderer, including after close, reports `AlreadyInitialized`.
Failure before successful initialization can be retried. Replacing/rebinding the
DLL requires a new process. The global wrapper binding/DLL remains mapped until
process exit; close does not claim to unload it.

Each render opens its document afresh and drops bitmap, page, document, and
owned file reader before publishing the frame. There is no native document
cache holding old Windows files open. `PreviewRenderer` itself is Send: only
channels/slots, atomics, the wakeup callback, and a JoinHandle cross threads.

`close()` and Drop stop admissions, clear pending work/results, wake the thread,
and join it. A currently executing synchronous native call must return first.
Move the renderer to a GUI shutdown helper if waiting on exit must not block the
UI. Do not close/recreate the renderer on page changes. Rust panic conversion is
not native crash isolation, and no timeout can safely force-kill a native
in-process thread. A hard render deadline needs a separate preview process.

## Verification

The ordinary unit tests use controlled fake backends and TempDir data:

- Portrait/landscape sizes, panel/zoom caps, invalid inputs and page clamping.
- RGBA byte length, dimensions, alpha, and invalid-frame rejection.
- Fixed bundle path and missing/invalid/wrong-architecture PE/Mach-O handling.
- Latest queued replacement, an old in-flight generation, unread-result
  replacement, and requests during a pending initializer.
- Nonblocking Busy without consuming a ticket, initialization-error delivery,
  wakeup ordering, Send, and construction/render/drop on one worker thread.
- Close waits for the controlled in-flight operation, discards queued work, and
  does not leave background rendering after it returns.

The real ABI smoke test is **ignored by default**, not silently passed when its
inputs are absent. The parent runner supplies explicit, verified inputs:

```powershell
$env:BABELOXIDE_PREVIEW_NATIVE_APP_ROOT = 'ABSOLUTE_VERIFIED_APP_ROOT'
$env:BABELOXIDE_PREVIEW_NATIVE_PDF = 'ABSOLUTE_AUTHORIZED_PDF_PATH'
$env:BABELOXIDE_PREVIEW_ARTIFACT_DIR = 'ABSOLUTE_OPTIONAL_ARTIFACT_DIRECTORY'
cargo test -p babeloxide-pdf-preview --lib --locked
cargo test -p babeloxide-pdf-preview --test native_smoke --locked -- --ignored --nocapture
```

One smoke process initializes one renderer, creates four synthetic PDF pages to
verify natural portrait/landscape, intrinsic rotation, CropBox, actual RGB
channels and white opaque background, then renders all pages of the supplied
real PDF. It also checks missing/invalid PDFs, lower/upper page clamping,
Unicode/space paths, and a Windows no-sharing File handle after publication.
That handle is stronger evidence than rename alone, which can succeed while a
shared-delete reader remains open. Optional PPM artifacts support visual review.
It finally checks the explicit reinitialization restriction.

Native calls cannot be interrupted by the test's polling timeout either; the
parent runner must apply a process timeout to detect/terminate a hung ABI test.
No native/DLL/clean-machine success is claimed merely because unit tests or the
crate compile. The parent records the actual native-library results separately;
see [STATUS](../../docs/STATUS.md) for platform execution and unverified Mac work.
