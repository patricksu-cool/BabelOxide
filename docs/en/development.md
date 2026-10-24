# Development and maintenance

## Install and run

Use Python 3.10-3.13 and a project-local environment:

```powershell
uv sync --locked
uv run pdf2zh --gui
uv run pdf2zh --help
```

The GUI listens on `127.0.0.1`. To use a trusted LAN, explicitly set
`--server-name 0.0.0.0` and configure `--auth-file`. Public sharing requires
`--share`; a launch failure never enables it automatically. Environment and
TOML configuration use the same settings model as the CLI.

Configuration lives in `config/` beside the application, independent of the
working directory. `PDF2ZH_APP_DIR` provides an explicit application directory
for isolated development. Existing settings migrate without overwriting a
destination file. Do not commit this directory, credentials or translated PDFs.

## Verify changes

```powershell
uv run pytest -q
uv run pytest -q tests/test_retry_characterization.py
uv run ruff check .
uv run ruff format --check .
uv run mkdocs build
uv run mkdocs build --strict
uv audit --locked
uv build --wheel
```

Tests use mocks and synthetic credentials. Real GUI/PDF checks should use a
loopback identity service rather than a paid API. Use isolated `USERPROFILE`,
`TEMP`, `TMP` and `PDF2ZH_APP_DIR` when running integration checks. Retry tests
characterize service-specific behavior; do not replace those strategies with a
single generic policy.

The extra `pytest -W error` check currently exposes unclosed Gradio event loops
and sockets during component construction. Record this separately from the
standard regression result; do not suppress all warnings.

## Source map

| Location | Responsibility |
| --- | --- |
| `pdf2zh_next/main.py` | CLI startup, logging and exit status |
| `pdf2zh_next/gui.py` | GUI entry points, components and event wiring |
| `pdf2zh_next/ui/` | Input preparation, task control, result/download projection, settings, sessions and native preview |
| `pdf2zh_next/high_level.py` | Public translation API and process lifecycle |
| `pdf2zh_next/translation/` | CLI batch coordination, BabelDOC configuration and completion data adapters |
| `pdf2zh_next/config/` | Typed models, CLI parser, environment mapping and atomic TOML storage |
| `pdf2zh_next/translator/` | Provider clients, caches and service-specific retry/rate limits |
| `pdf2zh_next/utils/asynchronize/` | Cross-process streaming and cancellation |
| `script/` | Supported launcher and PDF regression tools |
| `.github/workflows/` | CI, documentation and release packaging |
| `tests/`, `test/` | Regression tests and retained original fixtures/tests |

## Windows EXE

The release workflow `.github/workflows/exe-build.yml` downloads the official runtime
and launcher, then calls `script/build_windows.ps1`. It uses PyStand
1.1.4, embedded Python 3.13.16, and
`uv sync --locked --no-dev --no-editable`. Copy the installed package and its
resources, not an editable checkout. The bundle requires `runtime/`,
`site-packages/`, `pdf2zh.exe` and `_pystand_static.int`.

For a local build, supply an extracted embedded runtime and the PyStand CLI
executable. The script creates a locked, non-editable environment and refuses to
overwrite an existing output directory:

```powershell
./script/build_windows.ps1 -RuntimeDirectory ./dep_build/runtime -LauncherPath ./dep_build/PyStand/PyStand-x64-CLI/PyStand.exe
```

Use `-SkipOfflineAssets` only when intentionally omitting the offline asset ZIP;
the bundle will then need existing cached assets or network access on first use.
`BUILD_INFO.json` records the lock hash and this choice.

The main release, rebuild and fork-build workflows also use this script. They
select `-DirectML`, matching their existing acceleration profile through the
locked `directml` extra. The plain EXE workflow keeps the CPU profile.
Release staging excludes package tests, bytecode caches, editable provenance and
virtual-environment hooks, and includes license notices and a SHA256 manifest.

Test the EXE from another working directory, verify `--version` and failure exit
codes, then exercise upload, settings, translation, page navigation and downloads
in a real browser. Confirm `config/` is created beside the EXE. A successful
wheel build alone does not verify the Windows bundle.

## Containers

The Dockerfile installs the lockfile with `--no-dev --no-editable`; it does not
override BabelDOC/PyMuPDF afterward. Personal settings, caches, reports and local
test environments are excluded from the build context.

The container also defaults to loopback. Publishing a container port requires an
explicit listen address; for local-only host access:

```powershell
docker run --rm -p 127.0.0.1:7860:7860 IMAGE uv run --no-sync pdf2zh --gui --server-name 0.0.0.0
```

## Compatibility limits and evidence

Gradio 6.30 uses native Gradio image/page controls for PDF preview. The retired
custom PDF component could leave the whole page loading because its Svelte
runtime assets were unavailable. Pillow 12.3 remains constrained by Gradio's
`<13` requirement and still has security advisories; process trusted documents
and do not expose the GUI publicly without authentication.

BabelDOC 0.6.3 ignores its retired RapidOCR table model. The corresponding saved
option is accepted with a clear log message, without constructing an unused
model. Formula overlap, font warnings and true translation quality require PDF
review; identity responses only verify processing and layout regressions.

The repository's `reports/README.md` indexes the preserved maintenance reports,
including exact test results, dependency advisories and remaining limitations.
