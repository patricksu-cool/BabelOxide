# Native desktop application

`pdf2zh-gui` uses eframe/egui 0.37.0 with the Glow renderer. It opens a native
Windows window, native file dialogs, and the bundled PDFium preview renderer.
There is no browser, HTTP UI server, JavaScript, or Gradio startup path.

The binary uses `pdf2zh_cli::parse` and `Invocation::load` with an explicit
environment map. Embedders can call `launch_with_settings(LaunchOptions)` with
their own `ConfigStore`, `Settings`, `WorkerOptions`, cache filename, and bundle
directory. Constructing settings or opening the UI does not start Python.
Only an explicit Translate action submits the shared `pdf2zh_tasks` batch.
The BabelDOC engine remains an isolated Python worker for actual translation.

## Desktop workflow

- Add several PDFs with the picker or drag them into the window; remove individual
  inputs and preview them before translating.
- Choose any of the 24 shared translation engines, model and credentials, input
  and output languages, output folder, mono/bilingual output, page selection,
  glossary, terminology engine, request rate and concurrency, and layout options.
  Additional typed options use the shared configuration metadata and help text.
- Save settings explicitly. Changes are never automatically written, including
  when `disable_config_auto_save` is set. Unknown configuration fields survive
  the shared atomic save. Transient input filenames are excluded from defaults.
  Secret metadata produces password fields; `disable_gui_sensitive_input`
  disables editing them. Neither settings nor secret edit buffers are logged.
- Follow the latest shared task snapshot, cancel active or queued batches, choose
  an input/result document, switch original/translated/bilingual views, navigate
  pages, zoom, open the result, or save a copy of a PDF or extracted glossary.

PDF progress uses BabelDOC's weighted `overall_progress / 100`, including its
part offsets, then combines it with the number of completed files in a batch.
The terminology selector distinguishes explicit term flags from the shared
configuration's fallback to the main LLM, preserving the legacy default.

The application creates a persistent Tokio runtime on a background thread.
Disk writes and result copies use blocking-pool jobs. Task progress uses bounded
watch snapshots rather than an unbounded per-update queue. Closing the window
disables further actions, cancels batches, awaits `TaskManager::shutdown`, and
closes/joins PDFium on the background owner before allowing the viewport to close.
No translation, bridge, or SQLite cleanup future is aborted to meet a grace timer.

Preview is delegated to `pdf2zh-pdf-preview`; it only loads
`<bundle_root>/native/pdfium/pdfium.dll`, independently of a writable
`PDF2ZH_APP_DIR`. One renderer lives for the process. Requests use document
identity, generation and ticket checks; a busy slot retains only the latest UI
request for a later frame. A native preview failure leaves result open/save
available. Closing can wait for the current synchronous native render to finish.
See the workspace `PREVIEW_DESIGN.md` and `NATIVE_DEPENDENCIES.md` for the native
closure, hash verification, licenses, and OpenGL requirements.

## Validation boundaries

All application tests use temporary configuration/data directories and synthetic
credentials. Headless egui tests exercise the real UI frame, password painting,
and multi-file native drop handling without reading PDF bytes on the UI thread.
The background tests exercise explicit persistence, result copies, and shared
TaskManager submission/results/cancellation using the controlled fake worker
fixture. That fixture uses project-local Python solely for IPC/process tests;
it loads no BabelDOC or provider SDK and makes no HTTP/API calls.

Native window and file-dialog behavior, actual PDFium rendering and clean-machine
packaging need the root integration checks. Unit tests alone do not establish
those properties. The app currently targets Windows; the retained legacy web UI
preferences in configuration do not create a server in this native application.
