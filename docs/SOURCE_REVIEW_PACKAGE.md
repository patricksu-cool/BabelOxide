# 原始碼審查 ZIP

在專案根目錄雙擊 `打包原始碼.cmd`。不需要安裝工具，也不需要啟動 Rust、Python、Git 或任何專案程式。視窗顯示成功／失敗、完整輸出路徑、大小、檔案數、主要排除項目與敏感資訊檢查結果，最後暫停。

固定輸出為專案上一層的 `PDFMathTranslate-next-source-review.zip`。每次重新盤點目前磁碟上的檔案，包含尚未 Commit 的最新原始碼；ZIP 路徑直接從 CMD 所在目錄推導，與啟動時的工作目錄無關。中文、空格及路徑中的單引號、驚嘆號、`&`、括號均受支援。

## 收錄範圍

目前架構已對照 Cargo workspace、實際檔案與開發文件盤點：

| 範圍 | 收錄內容 |
| --- | --- |
| `crates/` | 7 個 Rust crates、`test-support/`、crate 內的整合測試與小型 Fixture |
| `python-worker/` | Worker、測試、依賴清單、wheel 版本／雜湊及 lock |
| `script/` | 建置、依賴查核及回歸工具的原始碼 |
| `tests/` | 行為基準 JSON、必要的小型文字 Fixture、打包整合測試 |
| `docs/` | 架構、開發、技術文件及文字驗證報告 |
| `.cargo/` | Rust 建置設定 |
| `.github/` | 若存在，收錄 Actions 及相關文字設定；本次盤點時尚不存在 |
| `resources/` | 明列的設定範本、資產 manifest／lock、Windows 說明，以及 `licenses/`、`manifests/` 的文字檔 |
| 根目錄 | Cargo／uv manifest 與 lock、README、LICENSE、`.gitignore`、打包 CMD，以及允許清單中的開發文件／設定 |

ZIP 直接以這些相對路徑為根，不額外增加外層目錄，也不生成或寫入任何專案檔案。

根目錄採檔名與目錄允許清單；選定的目錄還會限制文字副檔名。若新增未知的根目錄、根目錄檔案、文字資源或原始碼副檔名，會停止並提示維護允許清單，避免默默漏收重要程式碼。請在 CMD 內的 `$sourceRoots`、`$rootFiles`、`$resourceFiles`、`$textExtensions` 更新必要項目。不要將使用者資料目錄加入清單。

## 排除與敏感資訊

不收錄 `target/`、`.venv*/`、`dist/`、`.git/`、`.test-output/`、`__pycache__/`、各類快取、使用者 `config/`、`cache/`、`build-inputs/`、`pdf2zh_files/`、個人 `test/` 資料夾、編輯器／索引狀態、模型、字型、執行檔、DLL、編譯產物、PDF、Office 文件及其他非必要二進位檔。位於 `crates/config/` 的 Rust 原始碼仍完整收錄。

在允許範圍內遇到 `.env`、憑證私鑰、`credentials.*`、`secrets.*`、私人／本機設定檔名時，排除該檔並顯示排除數量。`.env.example` 等範本必須通過內容檢查。

所有候選文字檔會檢查常見 API／服務 Token 格式、JWT、私鑰區塊、帶密碼的 URL、認證標頭及憑證欄位的固定值，包括常見程式字串中的 JSON／設定內容。發現疑似憑證就停止，僅顯示檔案相對路徑，不印出匹配內容或原始例外訊息。空值、環境變數參照與 GitHub Actions 的 secrets 參照可保留；既有測試的明確合成憑證使用受限標記或逐檔固定值雜湊辨識，沒有整份檔案免掃描的例外。這類啟發式掃描不能辨識所有自訂／混淆密碼，新增私人資料仍應放在排除目錄。

`crates/core/tests/fixtures/compression/` 有 9 個測試需要的壓縮二進位 Fixture，總計不到 5 KiB；它們只在 SHA256 與已審查值完全相符時收錄，其他壓縮檔一律排除。HTML／JSON 明文對照亦收錄並掃描。Fixture 若更新，須先確認內容為合成測試資料，再維護 `$fixtureHashes`；不會自動信任新二進位資料。

檔案必須是有效 UTF-8 或帶 BOM 的 UTF-16，且沒有二進位控制字元。單檔上限 5 MiB，候選快照總計上限 64 MiB；超限會停止，避免誤收大型內容。允許範圍內的符號連結／目錄接合點會停止，不跟隨到其他目錄。

## 發布與錯誤處理

1. 只讀取允許的原始碼，保留本次位元組快照，全部完成敏感資訊檢查後才壓縮。
2. 使用 Windows 內建 PowerShell／.NET，在專案上一層建立唯一的暫存 ZIP。
3. 重新開啟 ZIP，逐檔解壓，核對項目名稱、數量、長度、SHA256，並再次檢查敏感資訊。
4. 驗證通過後才以 `File.Replace` 原子取代舊檔；首次輸出使用同目錄 `File.Move`。不會先刪除舊 ZIP。

失敗時保留舊 ZIP 並清理本次暫存檔。若 ZIP 被開啟或目的地不支援原子替換，會回報錯誤，不退回可能損壞舊檔的覆寫方式。腳本不呼叫 Git、不修改工作樹／索引，不執行被打包的程式。`ExecutionPolicy Bypass` 僅用於本次 PowerShell 子行程，不更改電腦的執行政策。

| 退出碼 | 意義 |
| --- | --- |
| `0` | ZIP 已完成驗證並發布 |
| `1` | 初始化、讀寫、壓縮或原子替換發生例外 |
| `2` | 缺少必要檔案、未知項目、編碼／大小／連結／Fixture 安全檢查失敗 |
| `3` | 發現疑似憑證，停止打包 |
| `4` | ZIP 項目、長度或 SHA256 驗證失敗 |

## 維護驗證

一般使用直接雙擊。自動驗證可加 `--no-pause`，保留相同檢查與退出碼，僅略過最後暫停：

```powershell
cmd /d /c '.\打包原始碼.cmd --no-pause'
powershell -NoProfile -ExecutionPolicy Bypass -File tests/source-review/test_packaging.ps1
```

整合測試在唯一的系統暫存目錄建立合成專案，實際透過 CMD 驗證含特殊字元的中文路徑、ZIP 解壓及內容一致性、原始檔與時間戳不變、排除項目、重複更新及移除舊項目、憑證攔截且不洩漏內容、Fixture、二進位偽裝、未知項目、檔案上限、接合點，以及目的地被占用時保留舊 ZIP／清理暫存檔。測試不使用個人設定、PDF、模型或實際憑證，也不執行 Worker／建置工具。
