# macOS 系統路徑與資源初始化檢查（2026-11-02）

## 根因與證據邊界

使用者提供的 macOS ARM64 日誌顯示三個 `seed::tests` 在 0.02 秒內因 `Engine asset path is a link` 失敗；舊訊息沒有附上被拒絕的路徑。該次 Runner 的隨機 TempDir 名稱與實際連結位置無法由這份日誌還原，本機也不是 macOS，不能宣稱已在原 Runner 重現。

程式碼可確認三項測試均使用 `tempfile::tempdir()`，將原始暫存路徑交給正式的 `seed_assets()`；原 `plain_path()` 檢查所有祖先直到 `/`，不區分資源樹與 OS 根路徑。Rust 明確說明暫存目錄可能是符號連結。在 macOS 的 `/var` → `/private/var` 或 `/tmp` → `/private/tmp` 前綴下，原程式必然拒絕合法的資源／快取檔案。另檢出 macOS Builder 共用的 Python `plain_path()` 有相同的祖先檢查。

新版拒絕訊息包含實際祖先路徑。新增的 macOS 原生測試會記錄原始 TempDir、canonical 路徑與祖先連結；測試失敗時 Rust 會將這些捕捉輸出附於現有 CI Log。特定 Runner 的實際路徑仍需下一次手動 CI 確認。

## 修正及安全邊界

- Rust `seed.rs` 與 Python Builder 只在 macOS 處理固定的 `/var`、`/tmp` 根前綴，使用 Path component 比較，沒有做字串全域替換。
- 若前綴是連結，要求連結由 root 擁有、指向對應的 `private/var` 或 `private/tmp`（亦接受相同絕對形式）；`/`、`/private` 必須為 root 擁有且不可由 group/other 寫入的一般目錄；目標也須為 root 擁有的一般目錄，canonical 結果須完全等於預期位置。
- 只 canonicalize 上述 OS 信任根，再接回原相對部分，後續資源 I/O 使用解析後的前綴。沒有對整個資源路徑或任意最近祖先做 canonicalize 來隱藏連結。
- `/private/...` 本身不需要例外；其他祖先、manifest、來源資源、快取目錄及目標檔仍逐層拒絕 Symlink。Windows 的 Reparse Point／Junction 檢查保留。
- manifest 的連結檢查提前至存在性判定之前，避免懸空 manifest 連結被誤認成沒有打包資源的原始碼環境。建立資源目錄後、發布前、並行發布遇到既有目標時，再檢查目標祖先。
- 保留 allowlist、檔名／穿越驗證、SHA256、大小檢查、取消、同檔案系統暫存檔與 `persist_noclobber()`。不同內容的使用者檔案不覆寫；並行初始化仍須得到一份完整檔案。新增檢查不代表把既有路徑式 I/O 改成完整的 descriptor-relative 檔案系統沙箱。
- Python Builder 另發現 `abspath()` 會在檢查前消除 `link/..`，可能隱藏中間的連結。改為先以不消除 `..` 的 `Path.absolute()` 檢查所有祖先，通過後才做字面正規化；一般 `ordinary/../new` 仍可使用。需要相對路徑比較的 Builder 先統一根目錄拼法，避免 `/var` 與 `/private/var` 比較不一致。

## 路徑審查範圍

掃描所有 73 個 Rust 檔案、Python Worker 的 3 個來源／測試檔案及 Worker Fixture，檢查 `canonicalize`、`symlink_metadata`、`is_symlink`、`tempdir`、`create_dir_all`、`resolve`、`realpath` 與路徑／權限處理；另追查共用 Builder 的呼叫點。codebase-memory 連線不可用，結論依據實際來源，未依賴過時索引。

| 範圍 | 審查結果 |
| --- | --- |
| bridge `seed.rs` | 修正系統前綴；保留資源樹連結限制；擴充安全及平台測試。三個原失敗測試完整保留。 |
| bridge `lib.rs`、`assets.rs` | 輸入、輸出、明確指定的 Worker home 根目錄、Python／script 及 staging 邊界原有 canonicalize 保留；輸出與 staging 的兩邊使用 canonical 路徑再做 containment 比較，沒有相同的祖先連結誤判。使用者指定 home 的根解析與其內資源樹檢查是不同信任邊界。 |
| app `transfers.rs`、`output_paths.rs`、`background.rs` | TempDir 所有權保留；ZIP 使用同目錄原子發布；來源與既有目的比較 canonical 路徑；沒有將 OS 祖先連結誤判為資源樹連結。不需修改。 |
| Python Worker | PDF 與資源操作使用絕對路徑及 `resolve(strict=True)`；HOME／TEMP 由 bridge 隔離傳入，沒有硬編碼 `/var` 或 `/tmp` 的拒絕規則。不需修改 Worker／BabelDOC。 |
| Python Runtime／Builder | 修正共用 `plain_path()` 及根路徑拼法；現有 standalone Python 的 bundle 內相對 interpreter symlink 仍由 `checked_files()` 驗證 containment，未改成拒絕全部 runtime alias 或允許 bundle 外連結。 |
| config、preview、CLI、tasks、其餘 crate | canonical 根目錄、固定 bundle-relative PDFium、同目錄原子儲存／輸出及 Fixture 路徑沒有相同的 OS 祖先連結判斷。原平台功能及測試保留。 |

本次審查範圍內，沒有其他已確認的同類 OS 祖先連結誤判；這不是對任意檔案系統、權限配置或同權限惡意程序的全面安全保證。

## Windows 實際驗證

Rust 1.99.1 Windows GNU，既有 Python 3.13.16 環境。Cargo 另使用 `--offline`，保留 `--locked`、全部既有測試及 `-D warnings`。

- `cargo fmt --all -- --check`：通過。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通過。既有 incremental cache 另回報 hard-link fallback 訊息，沒有停用規則或 source lint 錯誤。
- `cargo test --workspace --locked`：306 通過、0 失敗；原有預設 ignore 的 native PDFium smoke 隨後明確執行，1 通過。
- `cargo build --release --workspace --locked`：CLI／GUI 通過；CLI version/help 通過。
- Python Worker 35、Windows／共用 Builder 40、portable Builder 7、CI diagnostics 9、BabelDOC engine regressions 4：共 95 通過。Ruff 通過。
- 原始碼打包整合檢查 22 通過；Workflow 靜態檢查通過。
- 使用新 Release CLI、目前 Worker、既有 BabelDOC 0.6.4 runtime、182 項 allowlisted 資源的隔離副本與新的合成 PDF，經 localhost 固定回覆翻譯成功。單語及雙語 PDF 均可重新開啟，包含預期中文字；雙語另保留原文。3 次本機 API Fixture 呼叫，無真實 API Key／付費服務／私人 PDF。fresh home 的 182 項種子資源 SHA256 全部符合 manifest。
- 原失敗三項、穿越檔名、取消、不同使用者內容、壞 bundle、並行初始化，以及 Windows Junction 指向來源／home／cache／category 的防護測試均通過。

完整日誌及合成產物保留在 `.test-output/ci/macos-path-audit-20261102/`，CLI 輸出在 `.test-output/rust-migration/macos-path-audit-20261102/`。沒有重建或覆寫保留的完整 Windows 發行包。

## macOS 已做與待驗證

本機以已安裝的 `aarch64-apple-darwin` 標準函式庫，直接引用真實 `seed.rs`，對全部 target（包含 macOS／Unix 測試）執行嚴格交叉 Clippy，通過。暫存檢查 crate 使用既有鎖定的依賴，位於 `.test-output/`，沒有納入正式 workspace 或擴充測試框架。這僅證明相關型別／lint，不代表 macOS 原生測試或完整 workspace 驗證。

本機缺少 Apple SDK，仍不能完成 macOS workspace 的 C 相依編譯、連結或執行。下一次使用者手動執行既有 macOS Runner，必須完整通過：fmt、嚴格 Clippy、Rust workspace tests、Python Worker／Builder／IPC、CLI 單語／雙語、native PDFium 及 Release Build。新增測試會實測預設暫存根、`/var/tmp`、`/tmp`、`/private/tmp`，並驗證任意資源內連結依然被拒絕。macOS 原生執行、該次 Runner 的實際暫存路徑與 Release Build 均未在本次宣稱通過。

BabelDOC 0.6.4、依賴鎖檔、Retry／Backoff、翻譯及 IPC 協定不變。Workflow／失敗日誌收集保持不變：Windows／macOS、僅手動觸發、Artifact 7 天、失敗亦上傳完整 Log 與 Console 摘要。本次不 Push、不合併 main、不觸發 Actions。

## 參考

- [Rust temp_dir：暫存路徑可能為符號連結、Darwin 暫存根來源](https://doc.rust-lang.org/std/env/fn.temp_dir.html)
- [Rust canonicalize：解析中間連結、Windows extended-length 路徑](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)
