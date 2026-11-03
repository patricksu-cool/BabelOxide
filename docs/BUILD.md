# 建置與平台交付

版本由根目錄 Cargo workspace 讀取。Rust／開發 Python 使用 lockfile；Worker 分別使用 Windows 85 個、macOS 84 個固定 wheel（差異只有 Windows 專用 colorama），BabelDOC **0.6.4**、Pillow **13.0.0**。Mac 的 ONNX Runtime 需要 **macOS 14+、Apple Silicon arm64**。

## GitHub Actions

[build.yml](../.github/workflows/build.yml) **僅接受 `workflow_dispatch` 手動觸發**；push、PR、排程、Release 或其他 Workflow 都不觸發建置。`windows-latest` 建 Windows x64，`macos-latest` 先確認為原生 arm64 才建 Mac。僅需 `contents: read`；不使用 Secrets、不 Push、不簽章、不建立 Release。官方 checkout v6、setup-python v6、cache v5、upload-artifact v6 的 `action.yml` 已核對 `using: node24`。

[GitHub 要求手動 Workflow 存在於預設分支](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)。目前本機 main 沒有此 Workflow，也沒有 remote 可核對 GitHub 預設分支；若只 Push feature branch，Actions 可能尚不顯示 Run workflow。請使用者自行經審查將手動版 Workflow 放入實際預設分支，再到 Actions → BabelOxide build and test → Run workflow，選擇 `feature/babeloxide-v1`。不必為測試由工具擅自合併 main；本次沒有 Push 或觸發遠端工作。

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

CI 建包從 Git checkout 記錄實際 HEAD；未提交修改會標記 `-dirty`。來源審查 ZIP 不含 `.git`，從該 ZIP 重建時可先執行 Cargo build，再使用下列 `build_portable.py` 直接入口，明確提供已知的 `--source-commit` 識別；不為此重新初始化原專案。

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

`cargo build --release --locked` 只編譯 Rust，不安裝外部執行環境。新 checkout 的 `target/release/` 沒有 Python 與 PDFium，直接啟動 GUI 時可開視窗，但翻譯會在 Worker 路徑檢查失敗，預覽則缺少原生函式庫；這與完整 Artifact 的翻譯／渲染演算法無關。

先安裝 Rust／Cargo 與 uv，從原始碼目錄執行（不需要既有 Artifact）：

```powershell
# Windows x64；亦可直接雙擊啟動開發版.cmd
cargo build --release --locked
.\啟動開發版.cmd --prepare
.\啟動開發版.cmd
```

```bash
# macOS ARM64、macOS 14+；Finder 可雙擊 .command
cargo build --release --locked
./啟動開發版.command --prepare
./啟動開發版.command
```

未編譯時啟動工具會執行 locked Release build；修改 Rust 後使用 `--build` 強制重建。`--prepare` 完成準備及檢查即退出，`--check` 只檢查既有 Rust／Runtime，不重建或下載 Runtime；不帶選項會檢查後啟動 GUI。兩個入口共用 `script/dev.py`，由腳本位置定位根目錄，支援中文、空格及不同啟動工作目錄。`.command` 在 Git 記錄為 executable；解壓工具若遺失權限，只需 `chmod u+x ./啟動開發版.command`。

首次準備呼叫原有 `build_portable.py`，並保留正式 Builder 的來源 pins、wheel lock／hash、原生架構、依賴及資產驗證。下載放在 `build-inputs/<platform>/`，wheels 共用該平台的 `uv-cache/`；固定資源改變時建立新的 `dist/BabelOxide-dev-<platform>-<digest>/`，不覆寫舊資料夾。Python Runtime、模型與原生函式庫不進 Git。首次需要網路與較大型資源下載；成功後不會反覆下載相同內容。工具只在專案內管理環境，不安裝全域 Python、不改全域 PATH；Finder 入口只為其子行程加入一般 Cargo／uv 工具位置。

每次啟動／`--check` 會核對不可變 Runtime 清單、固定 Python 3.13.16／BabelDOC 0.6.4／wheel closure、模型與字型 seed，執行 Rust→Worker warmup，以及隔離 Python 原生 import／PDFium 初始化。日誌在 `.test-output/dev-launch/`；錯誤會顯示原因與日誌路徑。檢查不建立翻譯服務，不消耗 API 額度。若資源已損毀，保留錯誤日誌，移開該生成的 `BabelOxide-dev-…` 資料夾再執行 `--prepare`；可保留已核對的下載快取。

CLI 使用同一套 Runtime：

```powershell
.\啟動開發版.cmd --cli -- --version
# 翻譯參數放在 -- 後；macOS 對應 ./啟動開發版.command --cli -- ...
```

已有可信任的完整 Bundle 時，可用 `啟動開發版.cmd --runtime-dir "C:\完整包路徑" --check` 重用；macOS 指向 `BabelOxide.app/Contents/MacOS`。CLI／GUI 本身也接受 `--runtime-dir`，它同時選擇 Python、BabelDOC、資源 manifest 與 `native/pdfium`，明確的 `--python`／`--worker` 再覆寫對應欄位。僅指定 `--python` 與 `--worker` 不會提供 PDFium 或完整模型種子，因此不是完整 GUI 開發流程。開發啟動工具統一傳入一個 Runtime 根目錄及目前 checkout 的 `python-worker/worker.py`，禁止在轉交的 CLI 參數中混用其他 Runtime。

預設路徑相對 EXE，`--runtime-dir` 的相對路徑按呼叫者工作目錄解析為絕對路徑；PDFium 只從該根目錄的 `native/pdfium` 載入，不搜尋 CWD／系統 Python／系統 PDFium。兩平台原則一致，Python 在固定 Runtime 的隔離模式執行，套件由其 `site-packages` 載入。設定與可變翻譯／引擎快取仍屬於原始碼 EXE 的應用程式儲存區，Runtime 檢查使用獨立暫存設定，不搬移或覆寫私人設定。完整發行包仍使用原有相鄰資源佈局，無需這些開發腳本。

開發啟動產生的資料夾用於支援目前 checkout；需交付可獨立執行的成品時，使用上節完整包 Builder／Actions。HTTP 500 等翻譯服務錯誤另見 [翻譯診斷與實測](TRANSLATION_DIAGNOSIS.md)。

GUI 的輸出位置空白時，使用應用程式可寫儲存區下的 `outputs`，並在任務區顯示實際資料夾；Finder 的 `/` 啟動目錄不再決定輸出及引擎暫存位置。使用者明確指定的位置（包括 `.`）不變；CLI 的預設輸出仍是目前目錄。

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

## 測試資料與本機目錄

Rust 測試使用 `tempfile`，Worker 使用 pytest 的 `tmp_path`；CI 會以包內 PyMuPDF 生成合成 PDF。它們不依賴根目錄 `test/`、`.baseline-home/` 或過去 `.test-output/` 的 PDF。真實文件的手動回歸需明確提供輸入與比較目錄，不會自動下載外部論文。`tests/baseline/` 與 core fixtures 是保留的凍結基準，不能當作執行產物刪除。

手動驗證的產物集中放在已忽略版控的 `.test-output/`；腳本會自行建立輸出路徑，驗證後可以清除合成 PDF、Log、截圖及大型中間資料。`target/`、`.venv/`、Cargo／uv 套件快取允許保留，以加速後續測試；不需要每輪清理。實際使用的 config、API Key、翻譯 cache 與自訂模型必須保留。

本機保留一份 `dist/BabelOxide-v1.0.0-windows-x64/`、開發 `.venv/` 與 `build-inputs/windows-x64/`。建置輸入含目前 85 個固定套件的 uv 快取、pristine CPython、PDFium、完整資產 ZIP 及雜湊清單，已確認可離線安裝；舊 BabelDOC 0.6.3 實體快取已移除。資產 ZIP 分別供建置與執行包使用，兩者不可因檔名相同就任意刪除。macOS 的鎖檔、下載 pins、圖示與 builder 保留；Windows 的離線快取不能作為 Mac 原生驗證。

`pdf_regression_metrics.py` 可擷取單份文件的 Poppler 指標；`compare_rust_pdf.py` 比較兩組 mono／dual 的結構與像素。`audit_hybrid_dependencies.py` 是明確提供 Cargo metadata 與 Worker WHEELS 清單的 Windows OSV 查詢工具，需網路；它們是維護入口，並非應用程式執行期。

## 原始碼審查 ZIP

Windows 雙擊根目錄 `打包原始碼.cmd`；macOS 從 Git checkout 雙擊 `打包原始碼.command`。兩者支援 `--no-pause` 自動執行，互動終端預設保留完成／失敗結果並等待 Return。Mac 版以 Bash 3.2、系統 Perl（Encode／Digest::SHA）、zip／unzip 等原生工具實作，不需要 Python、Node 或 Homebrew；CMD 使用內建 PowerShell／.NET。工具不執行被打包的專案程式、不修改工作樹，均由腳本位置找根目錄，不依賴目前 Terminal 的目錄。

輸出專案上一層的 `BabelOxide-source-review.zip`：建立專案外快照與暫存 ZIP，檢查項目、解壓與逐檔 SHA256／敏感資訊後，才在同一檔案系統原子取代舊 ZIP。失敗保留既有 ZIP；完成顯示路徑、大小、檔案數及驗證結果。若 parent 不可寫，請先在可寫位置建立來源複本。Mac 腳本使用 LF 並在 Git 記錄為 executable；若其他 Windows 解壓工具遺失執行位元，Mac 可在該來源目錄執行 `chmod u+x ./打包原始碼.command`，不需改系統安全政策。

允許清單包括正式 Rust／Worker／scripts／tests／docs、鎖檔、圖示、LICENSE／來源告知與 CI。個人 config、cache、PDF、模型、編譯結果、環境憑證、索引狀態與未知根目錄不進 ZIP。九份微型壓縮 fixture 與三份二進位原創圖示必須符合固定雜湊；SVG 仍按文字掃描。新增必要檔案或更新圖示時須審查允許清單與 hash。

疑似憑證只顯示檔案路徑，不印出匹配值；既有測試假值由受限標記／逐檔 hash 辨識，沒有整檔免掃描。啟發式檢查不能辨識所有混淆憑證，私人資料仍應放在排除目錄。未知類型、大小超限、連結／junction、竄改或原子替換失敗都停止且保留舊 ZIP。退出碼 0 成功、2 allowlist／安全檢查、3 疑似憑證、4 ZIP 驗證／發布；CMD 以 1 表示 I/O／初始化，Mac 原生工具失敗可保留工具的非零退出碼。

`powershell -NoProfile -NonInteractive -File tests/source-review/test_packaging.ps1` 在唯一合成暫存目錄驗證上述契約，不拿私人檔案做 fixtures。CMD 保持 UTF-8 無 BOM／CRLF；PowerShell 測試保留 UTF-8 BOM，供 Windows PowerShell 5.1 正確讀取中文。

Mac 對應測試為 `/bin/bash tests/source-review/test_packaging_macos.sh`；手動 Actions 的 Mac job 會執行。它使用英文、中文／空格、括號及標點路徑、合成秘密與可控損壞 ZIP，檢查實際解壓、重跑、大小上限、二進位 pins、連結拒絕與失敗保舊包。另驗證 ZIP64、重複項目、路徑穿越、連結屬性、Unicode 欄位、截斷目錄及本地／中央標頭不一致。原生 macOS 必須執行兩項連結拒絕測試，建立連結失敗即失敗；Windows Git Bash 不具備原生連結能力時仍會明示未驗證。

macOS 打包器不再以 `unzip -Z -1` 的顯示文字驗證原始檔名。[Apple 的 `fnprint()`](https://github.com/apple-oss-distributions/zip/blob/main/unzip/unzip60/list.c) 仍會呼叫 [顯示用 `fnfilter()`](https://github.com/apple-oss-distributions/zip/blob/main/unzip/unzip60/extract.c)，可能取代非列印位元組。有效 ZIP 因而可能在項目比對時得到退出碼 4；原始失敗 Runner 未保留 `run.log`，無法還原該次 stderr，但 Windows 已重現「顯示轉換後項目不符 → 退出碼 4」的相同機制。現在按 [ZIP 格式](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT) 讀取有邊界檢查的中央目錄，逐項對照快照清單，支援原始 UTF-8、CRC 綁定的 Unicode 路徑額外欄位及 ZIP64；標頭不一致、重複、缺項、越界、分割／加密或連結項目均停止。CRC32 使用系統 Perl 隨附的 `Compress::Raw::Zlib`，不需安裝套件。ZIP 建立、CRC 檢查及解壓仍使用原有系統工具，逐檔 SHA256、敏感資訊掃描及原子發布保持。

打包命令失敗會顯示階段、腳本行及原始退出碼，保留工具 stderr，不輸出 shell trace、環境變數或憑證內容。封裝與測試會清除繼承的 ZIP／UnZip 選項，保持工具參數固定。測試為每次打包保留獨立日誌、工具版本及摘要於 `.test-output/ci/source-packaging-macos/run.*/`，可用 `SOURCE_REVIEW_TEST_LOG_DIR` 指定測試日誌父目錄；故障會在 Console 顯示末 100 行與失敗摘要。測試產生的合成憑證在持久日誌中遮蔽，但原始測試輸出仍用於檢查是否洩漏。既有 CI 的失敗 Console 摘要與 `always()` 日誌 Artifact 已涵蓋此位置，無須修改 Workflow。Windows Git Bash 的通過結果不等於原生 Bash 3.2／Finder 通過，尚缺的平台驗證見 [STATUS](STATUS.md)。
