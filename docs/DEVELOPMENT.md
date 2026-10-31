# 開發與 Windows 建置

## 環境

使用既有 Rust、Git、uv 與 Poppler，不修改全域 PATH 或系統 Python。Workspace 共用最低 Rust 1.88，GUI crate 要求 1.95；本機驗證工具鏈為 Rust 1.99.1、Windows x64 GNU，`.cargo/config.toml` 使用隨 Rust 提供的 rust-lld。實際相依版本以 `Cargo.lock` 為準。

Python 只用於 BabelDOC Worker 與開發工具。根目錄是非套件式 uv 專案，開發環境只有 pytest／Ruff；執行期的 85 個 wheel 由 `python-worker/` 的獨立 profile 鎖定，包含 BabelDOC 0.6.3、CPython 3.13.16、Pillow 13.0.0。不要把開發 venv 複製到執行版本。

```powershell
uv sync --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
uv run python -m pytest python-worker/tests -q
uv run python script/build_hybrid_windows.py --self-test
cargo build --release --workspace --locked
```

Rust 的程序測試預設使用專案 `.venv`，也可明確設定 `PDF2ZH_TEST_PYTHON`。測試只使用受控 child process 與 loopback API。Windows 沙箱若阻擋 socketpair、loopback 或原子替換，需在正常權限下重跑隔離測試；不能將沙箱失敗當作應用程式缺陷，也不能將未執行列為通過。

## 組裝執行版本

`script/build_hybrid_windows.py` 不編譯 Rust。它驗證 supplied EXE、固定 Python runtime、wheel hashes、PDFium、182 項引擎資產及依賴閉包，再於新的 staging 目錄組裝與驗證，最後才原子發布。輸出目錄必須不存在；不覆寫既有成品或使用者設定。

本機可重用的固定輸入放在忽略版控的 `build-inputs/windows-x64/`，包含原始 embedded runtime 與 manifest、PDFium DLL／notices、offline assets ZIP 和離線 wheel cache。這些是可重建輸入，沒有私人設定；來源、版本與雜湊由 builder 常數及各 lock 記錄。第一次換機建置需準備相同的已驗證輸入，不能假設 Git clone 自帶大型二進位資源。

```powershell
uv run python script/build_hybrid_windows.py --plan --output dist/final
uv run python script/build_hybrid_windows.py --output dist/final --source-commit (git rev-parse HEAD) --cargo-registry-src "$env:USERPROFILE\.cargo\registry\src" --allow-unresolved-native
```

目前 ONNX Runtime 的兩個模組會引用未內附的 `msvcp140_1.dll`。`--allow-unresolved-native` 僅允許在已安裝官方 VC++ x64 Runtime 的個人 Windows 上組包，仍驗證其餘雜湊、依賴與 assets；不會把報告標成乾淨機／公開再散布已通過。不要從 System32 複製 DLL。可用 `--runtime-source`、`--runtime-manifest`、`--pdfium-source`、`--assets-zip`、`--wheel-cache` 指向其他已查核輸入；所有預設均為專案相對路徑。

## PDF 與桌面回歸

請使用自己有權處理的 PDF。基準指定四頁 PDF 的 SHA256 記錄於 `tests/baseline/python-round4-pdf.json`；PDF 本身不納入 Git 或執行包。保留的 Python 行為資料與 Rust retry fixture 可在移除舊 App 後繼續使用。若需要重新執行歷史 Python App，請在另外的 Git checkout 使用 `codex/baseline-final-integration-20261031`，不要把它還原到正式工作樹。

```powershell
uv run python script/rust_migration_smoke.py '<input.pdf>' --label final-cli --exe dist/final/pdf2zh.exe --packaged --isolated-cwd --timeout-seconds 600
uv run python script/gui_smoke.py dist/final/pdf2zh-gui.exe --output .test-output/gui-qa
```

GUI harness 只提供本機固定回應服務並啟動視窗；仍須實際操作選檔、設定、翻譯、預覽、另存、失敗、取消與關閉。`control.json` 的 mode 可設 identity／error／slow；關閉 GUI 後可 action=reopen，完成後 action=stop。只用合成 Key。

使用包內 Python 執行 PDF 比對，避免為工具重新安裝完整引擎到開發 venv：

```powershell
dist/final/runtime/python/python.exe -I -B script/compare_rust_pdf.py '<baseline-dir>' '<candidate-dir>' --output .test-output/comparison.json
```

比對包含頁數、文字、逐字元座標、圖片及 Poppler 96 DPI RGB 像素。另人工檢視公式、圖表、字型與版面。固定回應測試驗證排版與串接，不代表真實中文翻譯品質。

必要時明確執行 `crates/pdf-preview/tests/native_smoke.rs` 的 ignored native 測試，依該檔列出的環境變數提供已驗證的 PDFium 與授權 PDF。一般 Cargo test 刻意不自動尋找系統 DLL 或個人文件。
