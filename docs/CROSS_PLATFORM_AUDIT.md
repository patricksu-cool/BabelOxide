# Windows / macOS 平台檢查（2026-11-02）

本次針對 7 個 workspace crate 的 73 個 Rust 檔案進行平台條件、平台 API、路徑及測試 Fixture 掃描；其中 23 個檔案包含平台條件。範圍包含 `src/`、`tests/`、Rust/Python Fixture、共用測試輔助及 Windows resource build script。這是原始碼審查與下列實際驗證的紀錄，不代表 macOS 原生驗收已完成。

## 修正

- `babeldoc-bridge/tests/lifecycle.rs`：共用 import 與程序生命週期測試的 import 分開，後者使用相同的 `cfg(any(windows, unix))`。原本只有 Windows 執行的程序樹當機、取消、CLI 取消及 factory health 清理測試現在也在 macOS 執行，因此 `FactoryInitialization` 與 `translate_pdf_with_initialization` 在 macOS 也有實際用途。
- `app/src/settings_ui.rs`：macOS 設定頁的檔案／資料夾選擇改用現有的 `rfd` 原生對話框並保留父視窗；取消選取仍回傳 `None`。Windows 實作不變。
- `core/src/process.rs`、`core/src/process/unix.rs`：Windows Job Object 的建立、暫停／恢復、終止、等待及關閉 handle 邏輯不變。Unix 改在 exec 前建立獨立 process group，清理時終止所屬 group 並等待可執行成員退出，避免只殺直接子程序而留下持有 stdout 的後代。檢查排除已釋放執行資源的 zombie，避免在 Tokio 回收直接子程序之前互相等待。清理有 5 秒期限；失敗仍回報錯誤，Drop 則作最後的終止嘗試。
- `core/tests/legacy_providers.rs`：CLI / Claude 的 timeout、取消及後代程序退出斷言擴及 Unix。Windows 換行斷言與原有 handle 觀察器仍只用於 Windows。
- `core/src/providers/legacy/fixtures/offline_cli.rs`：PID 先寫入臨時檔，再 rename 發布，避免以檔案存在為 readiness gate 時讀到空 PID。直接呼叫 rustc 的 Windows GNU linker 設定仍只套用 Windows GNU。
- 補上 macOS Application Support fallback、Unix 執行權限及原子替換保留既有 reader snapshot 的測試。新增 3 個可在 Windows 執行的 process group snapshot 測試，涵蓋其他 group、zombie、無效狀態與危險 group ID。

Unix 使用既有 system `/bin/ps`，只查詢 PID／group／狀態／啟動時間，不讀取命令列或個人設定。正式依賴僅將既有鎖定的 `libc 0.2.191` 列為 core 的 Unix 直接依賴，沒有新增 crate 版本或更新其他依賴。BabelDOC 保留 0.6.4；翻譯、Retry / Backoff 與 IPC 訊息協定不變。

## 平台條件審查範圍

| 範圍 | 檢查結果 |
| --- | --- |
| app：`background.rs`、`dialogs.rs`、`lib.rs`、`main.rs`、`output_paths.rs`、`settings_ui.rs`、`transfers.rs` | Windows Shell / 長路徑啟動程式有平台邊界；macOS 使用 `/usr/bin/open` 直接傳遞路徑，既有 PDF、CSV、儲存對話框支援 macOS。本次補上設定頁遺漏的原生選擇器。Windows 檔案分享鎖定測試保留其 Windows 條件。 |
| bridge：`lib.rs`、`assets.rs`、`seed.rs`、`tests/lifecycle.rs` 與 Python Fixture | Worker 的 Python 路徑區分平台；非同步 Worker wait 與主錯誤保留。Windows reparse-point / sharing API 在條件內；關於跨平台程序清理的測試擴及 Unix。 |
| core：`process.rs`、`process/unix.rs`、`providers/legacy/cli.rs`、`tests/legacy_providers.rs` 與 native CLI Fixture | Windows API、PATHEXT、CRLF 只用於 Windows；Unix 執行位元、process group 與清理觀察器有實作與測試。既有 `Context` import 與 tail expression 修正保留。 |
| config：`storage.rs`、`tests/store.rs` | 共用 import 亦用於跨平台測試，沒有僅在 Windows 才使用的頂層 import。macOS writable fallback、Unix directory sync / atomic rename 分支有對應測試；Windows sharing failure 仍為 Windows 專屬測試。 |
| pdf-preview：`platform.rs`、`tests.rs`、`tests/native_smoke.rs` | Windows loader API 有條件；macOS 驗證 ARM64 Mach-O 並從固定 bundle 路徑載入 dylib。macOS 不使用 Windows DLL search-directory API 是平台差異，沒有以空 renderer 取代預覽。PE validator 在 macOS test build 中仍有使用。原有 native ABI 測試保留，未新增 ignore。 |
| CLI / tasks：`cli/src/main.rs`、`cli/tests/cli.rs`、`tasks/tests/batches.rs` | Python 路徑與測試 executable 路徑有平台選擇；沒有將 Windows 專用 API 放進 `cfg!` 兩側一起編譯。 |
| build / 其餘來源 | `windows_resource.rs` 依 Cargo TARGET 決定是否產生 Windows resource；`windows_launch.rs` 由 Windows-only module 引入。其餘來源掃描過平台 API、硬編碼路徑與條件。7 個 crate 均未宣告自訂 Cargo feature，沒有額外的 workspace feature 組合可展開；依賴原有 feature 設定未變。 |

未使用新增的 `allow`、排除測試或空的 macOS 實作來避開 Clippy。codebase-memory 連線在本次檢查時不可用，因此平台審查使用實際原始碼，不依賴索引推論。

## Windows 實際驗證

Rust 1.99.1，Windows GNU；使用專案既有 Python 環境與可重建的合成 PDF，不使用私人 PDF、個人 API Key 或付費服務。

- `cargo fmt --all -- --check`：通過。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通過。
- `cargo test --workspace --locked`：304 通過、0 失敗；原有 1 個 native ABI 測試預設 ignore，隨後明確執行並通過，未跳過驗證。
- `cargo build --release --workspace --locked`：CLI / GUI 通過；Release CLI `--version`、`--help` 通過。沒有重建或覆寫既有完整發行包。
- 原生 PDFium：使用保留的已驗證 bundle 與新產生的合成 PDF，明確執行 native smoke，1 通過。
- Python Worker 35、Windows builder 36、portable builder 7、CI diagnostics 9：合計 87 通過；Ruff、Workflow 靜態檢查通過。
- Windows 原始碼打包整合檢查：22 通過，使用隔離 Fixture，沒有對真實專案或使用者資料進行破壞性測試。

本機執行 Cargo 時另使用 `--offline`，保留 `--locked` 與原有警告規則。完整日誌在忽略版本控制的 `.test-output/ci/cross-platform-audit-20261102/`。

## macOS 交叉檢查與待驗證事項

在既有 Rust 1.99.1 工具鏈加入官方 `aarch64-apple-darwin` 標準函式庫目標，沒有升級工具鏈或安裝 Apple SDK。

- `babeloxide-pdf-preview` 真實來源及所有 target 的 macOS ARM64 Clippy / 型別檢查通過，保留 `-D warnings`。
- 直接引用真實 Unix process scope、共用 Unix 測試觀察器、`dialogs.rs`，以及逐字擷取的設定頁 `pick_path` 函式，以既有 eframe / rfd 版本執行 macOS ARM64 Clippy / 型別檢查，包含 test build，通過。這個暫存檢查 crate 位於 `.test-output/`，沒有加入正式產品或測試框架。
- 完整 workspace 的 macOS 交叉 Clippy 已嘗試，但 C 相依套件的編譯因缺少 Apple SDK 的 `string.h` 等標頭而停止（exit 101）。沒有改依賴 feature、略過 C build 或放寬警告來繞過環境限制。
- 尚未在 macOS 執行任何測試或二進位，亦未驗證原生 GUI 對話框、實際 Worker / CLI 程序群組取消與當機清理、Application Support 寫入、native PDFium 或 macOS Release Build。交叉檢查不能取代這些驗收。

**macOS Runner 必須再完整通過現有 Workflow 的測試、Release Build 與原生 smoke，才能宣告 macOS CI 通過。** 使用者手動 Push 後，至 Actions 選擇 `feature/babeloxide-v1` 執行既有 Build Workflow；原生檔案／資料夾對話框另需 macOS 桌面互動驗收。本次未 Push、合併 main 或觸發 Actions。

Workflow 與日誌機制保持不變：Windows / macOS、僅 `workflow_dispatch`、Artifact 7 天、失敗亦上傳完整隱藏目錄 Log，以及 Console 最後 100 行與原始失敗 exit code。

## 平台 API 參考

- [rfd 0.17.2 FileDialog：Mac、父視窗及檔案／資料夾選取](https://docs.rs/rfd/0.17.2/rfd/struct.FileDialog.html)
- [Apple kill(2)：process group signaling](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kill.2.html)
- [Apple ps(1)：pgid、state、lstart 與空標題欄位](https://github.com/apple-oss-distributions/adv_cmds/blob/main/ps/ps.1)
