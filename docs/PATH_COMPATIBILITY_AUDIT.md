# Windows／macOS 系統路徑與資源初始化檢查（2026-11-02）

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

## Windows 實際驗證（macOS 根目錄修正）

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

## 後續修正：Windows 8.3／長路徑身分一致性

Windows CI 的 `test_plain_paths_and_asset_verification_use_consistent_system_roots` 比較 `plain_path(home)` 與 `home.resolve(strict=True)`。舊 `plain_path()` 只在 Windows 做 `abspath()`，保留 8.3 別名；`resolve()` 則取得檔案系統的長名稱。不能只憑 `RUNNER~1` 與 `runneradmin` 的字串推定兩者相同。

本機建立含空格的獨立測試目錄，透過 Windows `GetShortPathNameW` 取得真正的 8.3 路徑，先以 `samefile()` 與 `st_dev`／`st_ino` 確認身分，再將此短路徑指定為測試程序的 TEMP。未修改版本重現同名測試的路徑不相等斷言；另外重現正式 `validate_output()` 將 repository 內的合法新輸出誤認為 repository 外。原始重現日誌保留為 `baseline-short-path.log`，不是對原 Runner 隨機目錄的推測。

### 修正方式及防護

- Windows Builder 先檢查原始路徑的每一層，仍拒絕 Symlink、Junction 及 Reparse Point，並保留先檢查 `link/..`、後消除 `..` 的順序。
- 找到最近的既有祖先，保存其 `lstat()`，以 `resolve(strict=True)` 展開既有的 8.3 名稱，再接回尚未建立的輸出尾段。只容許 `FileNotFoundError` 作為尚不存在的路徑；權限等錯誤不被吞掉。
- 對標準 `\\?\C:\...`／`\\?\UNC\server\share\...` 命名空間統一為一般 drive／UNC 拼法前後，必須以 `samestat()` 確認仍為原本的同一實體。這是固定的命名空間處理，沒有將 `~1` 字串猜成使用者名稱。不能證明相同即失敗。
- 原始及正規化後的祖先會再次檢查，避免 canonicalize 隱藏連結。Verbatim 路徑若有結尾空格／句點的 component，直接拒絕，避免尚未存在的尾段在 Win32 正規化時改變意義。一般 Win32 路徑依原有系統語意處理。
- 輸出及受保護的 repository 根也採用一致的正規化方式，含 repository 以 verbatim 名稱表示的情況。既有輸出不覆寫、輸入／輸出不得互相包含、資源 SHA／allowlist 與鎖檔原子更新規則保留。
- macOS 的固定 root-owned `/var`／`/tmp` 信任根規則沒有修改，也沒有對任意使用者連結開放例外。這仍是路徑式 I/O 防護，不宣稱能完全防範同權限惡意程序持續更換整棵目錄。

### 其他來源審查

再次掃描 Rust crates、Python Worker、共用／Windows／portable Builder 中的 `canonicalize()`、`resolve()`、`symlink_metadata()`、`lstat()`、`relative_to()`、路徑相等及 containment 判斷。

| 範圍 | 本次結論 |
| --- | --- |
| Windows／共用 Builder | 修正 `plain_path()` 的既有祖先名稱與身分驗證，以及 repository 比較。補測 Runtime manifest、Cache／Engine Assets、鎖檔及輸出保護。 |
| Rust `seed.rs` | 正式 seeder 沒有比較未正規化的兩條絕對路徑；逐層 link／reparse 防護可接受真正的 8.3 名稱。只增加 Windows 實際 seeding 回歸：短、長、canonical verbatim 根均得到同一份資源，既有不同使用者內容仍不可覆寫。正式 Rust 程式不修改。 |
| bridge `lib.rs`／`assets.rs` | Worker 輸入／輸出、資源 home 與 staging 的 containment 兩邊已使用檔案系統 canonical 根。沒有發現同類混用短／長名稱的問題。 |
| Python Worker、portable Builder、`build_support.py` | Worker 已 `resolve(strict=True)`；bundle 內相對 symlink 的 containment 兩邊均解析；manifest 的根與列舉結果沿用同一表示。沒有已證實需修改的同類問題。 |
| app output／transfers／background、preview／config／CLI | 檔案輸出比較使用 canonical 路徑或由同一 canonical 根衍生。未發現本次同類問題，保留原始功能。 |

### 本次驗證

- Windows fmt、嚴格 Clippy、Rust workspace Tests：307 通過、0 失敗；原本標記 ignore 的 native PDFium smoke 另外明確執行並通過 1 項，共 308。Release CLI／GUI Build、CLI version／help 通過。保留 `--locked`、`-D warnings`，Cargo 使用既有離線快取。
- Python Worker 35、Windows／共用 Builder 44、portable Builder 7、CI diagnostics 9、BabelDOC engine regressions 4，共 99 項通過；Ruff 通過。最終 Builder 44 項另以真實 8.3 TEMP 全部執行並通過，沒有略過失敗測試。
- 新增 Windows Junction／Unix Symlink 共用防護測試，本機實際執行 Junction 分支通過，Unix 原生分支尚待 macOS Runner。同一目錄及不同目錄、正規化期間身分改變、輸入輸出重疊、惡意連結與穿越、verbatim 非預期名稱、UNC 命名空間與錯誤身分等 Windows 回歸通過。UNC 部分用合成 stat 模擬，未連接實際 SMB 分享。
- Windows CMD 原始碼打包 22 項通過；既有 Workflow 靜態檢查通過，Workflow 完全沒有修改。
- 新 Release CLI 使用真正短路徑的合成 PDF 與全新 Worker home，透過 Rust／Python IPC 及 localhost 固定回覆，成功產生單語、雙語 PDF。兩份重新開啟後均有預期中文字，雙語保留原文；182 項資源 SHA256 全部符合 manifest，沒有複製 cache DB。使用既有 BabelDOC 0.6.4，未使用私人 PDF、真實 Key 或付費 API，未修改保留的 Windows 完整包。
- macOS ARM64：直接引用目前正式 `seed.rs`（包含 Unix／macOS 測試）的獨立交叉 Clippy 全 target 通過。這不等於完整 macOS workspace 編譯或原生執行；macOS `/var`／`/tmp` 的實際測試、完整 Tests／Release Build 仍須由使用者手動啟動既有 Runner 驗證。本機沒有 Apple SDK 或 macOS 執行環境。

本次完整日誌與合成資料位於 `.test-output/ci/windows-path-audit-20261102/`，CLI 輸出位於 `.test-output/rust-migration/windows-path-audit-20261102/`。macOS 原生結果與實際 UNC／SMB 環境均未宣稱通過。BabelDOC、GUI／CLI、Retry／Backoff、依賴及 Workflow 不變；不 Push、不合併 main、不觸發 Actions。

## 參考

- [Rust temp_dir：暫存路徑可能為符號連結、Darwin 暫存根來源](https://doc.rust-lang.org/std/env/fn.temp_dir.html)
- [Rust canonicalize：解析中間連結、Windows extended-length 路徑](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)
- [Python 3.13 realpath／samefile／samestat：8.3 展開及檔案身分比較](https://docs.python.org/3.13/library/os.path.html)
- [Microsoft：Windows 路徑與命名空間](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file)
