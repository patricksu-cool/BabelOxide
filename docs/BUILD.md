# 建置與平台交付

版本由根目錄 Cargo workspace 讀取。Rust／開發 Python 使用 lockfile；Worker 分別使用 Windows 85 個、macOS 84 個固定 wheel（差異只有 Windows 專用 colorama），BabelDOC **0.6.4**、Pillow **13.0.0**。Mac 的 ONNX Runtime 需要 **macOS 14+、Apple Silicon arm64**。

## GitHub Actions

[build.yml](../.github/workflows/build.yml) 接受手動、來源 push 與 PR 觸發。`windows-latest` 建 Windows x64，`macos-latest` 先確認為原生 arm64 才建 Mac。僅需 `contents: read`；不使用 Secrets、不 Push、不簽章、不建立 Release。官方 checkout v6、setup-python v6、cache v5、upload-artifact v6 的 `action.yml` 已核對 `using: node24`。

兩平台執行 fmt、嚴格 Clippy、Rust／Worker／Builder 測試、Release build、原生 import 與 PDFium、CLI／IPC 本機 Mock、套件閉包與 manifest 檢查；Windows 另執行 CMD 原始碼封裝測試。GUI `--version` 僅是版本 smoke，Actions 沒有宣稱通過桌面互動。

輸出完整 ZIP：`BabelOxide-v<workspace-version>-windows-x64`／`BabelOxide-v<workspace-version>-macos-arm64`，Artifacts 保留 **7 天**。只有所有步驟成功才上傳；缺失 wheel、錯誤架構或不可攜的 dylib/rpath 都會使 job 失敗，不使用 continue-on-error。

## 本機

保留原有 Rust／uv，Python 依賴只放專案 `.venv`。一般檢查：

```text
uv sync --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
uv run python -m pytest python-worker/tests -q
uv run python script/build_hybrid_windows.py --self-test
uv run python -m unittest discover -s script -p test_build_portable.py
cargo build --release --workspace --locked
```

`script/ci.py --platform windows-x64 --check` 與 `--build` 是 Actions 的相同入口；Mac 改用 `macos-arm64`。本機已有名稱不同但版本相符的 Rust toolchain 時，可指定 `--rust-toolchain <existing-name>`；不會替換或安裝本機工具鏈。CI 的 Rust 固定版本、Python runtime、PDFium 與 bootstrap uv 的公開來源／SHA256／大小在 [build-inputs.lock.json](../resources/build-inputs.lock.json)。

完整包 builder（先編譯 Rust，輸出目錄須不存在）：

```text
uv run python script/build_portable.py --platform windows-x64 --output dist/BabelOxide-windows-test --allow-unresolved-native --zip
uv run python script/build_portable.py --platform macos-arm64 --output dist/BabelOxide-macos-test --zip
```

Windows 的旗標明示需要官方 VC++ x64 Runtime；CI 只容許已知 VC++ Runtime DLL，其他未解析 import 一律失敗。不從 System32 複製檔案。

## 打包邊界

- Python 來自固定官方 embedded archive（Windows）或 Astral standalone archive（Mac）。Mac 移除 pristine archive 中的 pip／安裝 wrapper 後才安裝固定 engine closure，保留 stdlib 與其必要資源；沒有 venv 複製或全域 Python 依賴。
- BabelDOC 透過隔離 Worker 下載公開資產，逐項按 182 個 size／SHA3 pin 核對。CI 可重建 ZIP；ZIP 時戳不同不影響資產身分，產物另記實際 archive SHA256。
- Windows 保留 PE imports、原生 probes、完整 licenses／provenance／RELEASE_FILES。Mac `.app` 保留內部相對 runtime symlink，查核 Mach-O arm64、dylib 與 rpath，移動完整包後再執行 native／CLI smoke；不修改全域 Library 搜尋路徑。
- 只散布固定執行期、模型／字型種子、空 Key 範本與程式；不帶入個人 config、cache、測試 PDF 或翻譯結果。Manifest 同時檢查檔案集合、雜湊與 symlink 目標。
- Mac 未簽章／未公證，Gatekeeper 可能阻擋；不移除 quarantine 或停用系統保護。沒有 Intel、CUDA、DirectML 或必然 CoreML 加速的承諾。

兩平台實際驗證狀態與尚存限制以 [STATUS](STATUS.md) 為準；本機 Windows 成功不代表 Mac 已原生通過。

## 開發驗證

Rust process 測試預設使用專案 `.venv`，可由 `BABELOXIDE_TEST_PYTHON` 明確指定。測試使用合成憑證、本機 socket 與受控子行程，不讀使用者設定。受限沙箱可能拒絕 loopback、socketpair 或原子替換；應在允許這些操作的隔離測試環境重跑，不將未執行列為通過。

`cargo test -p babeloxide-core --test provider_compatibility`、`--test legacy_providers` 與 cache suites 消費獨立 frozen oracle。保留 `tests/baseline/`、core fixtures 與 `resources/licenses/`，不因文件精簡而刪除。原 Python App 已在早期移植中移除；歷史來源與基準保留於 Git，不能把舊 App 還原到正式執行流程。

PDF 測試請使用自己有權處理的文件；指定論文本身不納入來源或成品：

```text
uv run python script/rust_migration_smoke.py <input.pdf> --label local-qa --exe dist/BabelOxide-v1.0.0-windows-x64/BabelOxide.exe --packaged --isolated-cwd --timeout-seconds 600
uv run python script/gui_smoke.py dist/BabelOxide-v1.0.0-windows-x64/BabelOxide-gui.exe --output .test-output/gui-qa
dist/BabelOxide-v1.0.0-windows-x64/runtime/python/python.exe -I -B script/compare_rust_pdf.py <baseline-dir> <candidate-dir> --output .test-output/comparison.json
```

GUI harness 只提供本機固定回應與合成 Key，仍須實際操作。`control.json` 的 mode 可用 identity／error／slow，關閉後 action=reopen，結束時 action=stop。PDF 比對涵蓋頁數、文字、字元座標、圖片、幾何、嵌入字型與 Poppler 96 DPI RGB；固定回應不代表遠端翻譯品質。

`pdf-preview` 的 native smoke 刻意預設 ignored，避免自動找系統 DLL／私人文件。CI 以 `BABELOXIDE_PREVIEW_NATIVE_APP_ROOT`、`BABELOXIDE_PREVIEW_NATIVE_PDF` 明確指定完整包及合成 PDF，另行執行 `--test native_smoke -- --ignored`。

## 原始碼審查 ZIP

Windows 雙擊根目錄 `打包原始碼.cmd`，或加 `--no-pause` 自動執行。只使用內建 PowerShell／.NET，不執行被打包的程式；輸出專案上一層的 `BabelOxide-source-review.zip`，逐檔 SHA256／解壓複查後原子取代舊 ZIP。若來源位於不可寫的 parent，請先在可寫位置建立來源複本。工具不修改工作樹。

允許清單包括正式 Rust／Worker／scripts／tests／docs、鎖檔、圖示、LICENSE／來源告知與 CI。個人 config、cache、PDF、模型、編譯結果、環境憑證、索引狀態與未知根目錄不進 ZIP。九份微型壓縮 fixture 與三份二進位原創圖示必須符合固定雜湊；SVG 仍按文字掃描。新增必要檔案或更新圖示時須審查允許清單與 hash。

疑似憑證只顯示檔案路徑，不印出匹配值；既有測試假值由受限標記／逐檔 hash 辨識，沒有整檔免掃描。啟發式檢查不能辨識所有混淆憑證，私人資料仍應放在排除目錄。未知類型、大小超限、連結／junction、竄改或原子替換失敗都停止且保留舊 ZIP。退出碼 0 成功、1 I/O／初始化、2 allowlist／安全檢查、3 疑似憑證、4 ZIP 完整性。

`powershell -NoProfile -NonInteractive -File tests/source-review/test_packaging.ps1` 在唯一合成暫存目錄驗證上述契約，不拿私人檔案做 fixtures。CMD 保持 UTF-8 無 BOM／CRLF；PowerShell 測試保留 UTF-8 BOM，供 Windows PowerShell 5.1 正確讀取中文。
