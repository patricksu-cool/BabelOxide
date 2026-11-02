# 翻譯診斷與實測（2026-11-02）

這次將「上游 HTTP 500」與「找不到 Python Worker」分開處理，不能用其中一個解釋所有版本。

使用者已補充：從完整解壓後的舊 Windows GUI 啟動。該路徑就是本機已測試成功的下載包；裸 Cargo Build 缺少 Runtime 的獨立案例，不能用來解釋這個正式包的原始故障。

## 已確認的失敗條件

- Cargo Release Build 只有 Rust CLI／GUI，`target/release` 不會自動帶入 CPython、Worker、BabelDOC、模型及字型。直接執行其預設路徑會找不到 `runtime/python/python.exe`（Mac 為 `runtime/python/bin/python3`）或 `babeldoc/worker.py`。已用真實 PDF 及本機 API 重現。
- 舊流程先做主服務及術語服務的健康檢查，才檢查 Worker 路徑。Runtime 缺失時仍會發出 API 請求；若上游先回 HTTP 500，缺檔問題還會被遮住。現在先檢查 Runtime，顯示實際路徑及完整解壓／開發啟動方式，缺檔不會呼叫 API。
- HTTP 500 是翻譯服務回應。保留狀態與原始安全錯誤鏈，補上稍後重試／改用其他 Provider 的提示；不更改原有 Retry／Backoff，也不以原文或假譯文當成功結果。
- 真實 SiliconFlowFree 診斷輪確實捕捉到 HTTP 500：PDF 2 有 **5 次 500、53 次 429、56 次 200**；PDF 1 有 **41 次 429、43 次 200**。兩份最終均成功。500 出現在 Worker 已啟動後的服務請求，並非找不到 Worker 或 PDF 解析失敗。這能證明上游不穩定，但無法從 Client 得知服務端的內部 Exception。
- Worker 先前把 BabelDOC `error` 事件改成泛用 `RuntimeError`，最外層只傳例外種類，Rust 又忽略訊息。現在保留原始例外與因果鏈、Traceback、處理階段，並在 Rust 再依兩組 Provider 的憑證做遮蔽。啟動階段的 Worker 錯誤及非零退出碼也會保留。
- 執行沙箱曾以 Socket 10013 拒絕 loopback；這是本次測試環境限制，與使用者原始 HTTP 500 不同。HTTP 診斷現在保留 I/O 分類和 OS 代碼，不印出 reqwest URL、Header 或憑證。
- `--debug` 會在 CLI stderr 記錄每次 HTTP 回應的狀態碼，包含預檢和重試；5xx 另保留經既有憑證／來源文字／URL 遮蔽的錯誤訊息。缺少可解析 JSON 錯誤欄位時，記錄最多 4,096 字元的安全本文，避免純文字／HTML 原因在重試恢復後遺失。不記錄 Request Header；Worker 的詳細日誌仍留在輸出資料夾。

## 正確啟動原始碼版本

先建置完整執行包，或準備 BabelDOC **0.6.4** 的獨立 Worker Runtime。單獨的 Cargo Build 不能當作完整安裝包。使用既有完整包的 Runtime 可以避免改動系統 Python：

```powershell
cargo build --release --locked
# $bundle 是完整解壓後的 Windows 包，$paper 是輸入 PDF。
# 第一次先把該包的固定資源還原到獨立開發快取。
target/release/BabelOxide.exe --python "$bundle/runtime/python/python.exe" --worker python-worker/worker.py --worker-home .test-output/source-home --restore-offline-assets "$bundle/engine-assets/offline_assets_b485a82328a20d1e4b5c37bb9c6472e7af9943333bdffd39a05b1f57d6d7a1da.zip" --json
target/release/BabelOxide.exe $paper --python "$bundle/runtime/python/python.exe" --worker python-worker/worker.py --worker-home .test-output/source-home --pages 1-2 --output .test-output/source-pdf --json
```

GUI 可用相同 `--python`、`--worker`、`--worker-home` 選項啟動。macOS 用 `.app/Contents/MacOS` 作為包根目錄、`runtime/python/bin/python3` 作為 Interpreter；需要原生 Apple Silicon Mac 實測。

正式包應保留 EXE／App 旁的 `runtime`、`babeldoc`、`resources`、`native`、`engine-assets` 等完整目錄，不能只複製 EXE。缺檔時重新解壓完整包；不要把私人設定檔覆蓋成範本。

## 回歸驗證方式

`script/rust_migration_smoke.py` 使用獨立設定、合成 Key 與 loopback API。可加 `--auto-extract-glossary` 檢查預設術語流程，或 `--http-status 500` 重現上游失敗。成功必須有文件翻譯請求（不能只有 API 健康檢查）、唯一 `finish`、單語及雙語輸出，並以鎖定 Runtime 的 PyMuPDF 重新開啟及渲染每一頁、確認有可擷取文字；缺檔、損壞、需要結構修復或洩漏合成 Key 均使 smoke 失敗。原始 PDF 雜湊也必須保持不變。

Windows/macOS CI 都會檢查包內 Worker 與目前原始碼一致，分別測試打包 CLI 與指定完整 Runtime 的原始碼 CLI。成功與失敗的 CLI、Worker 及 PDF 驗證日誌進入既有 `always()` 上傳目錄；不放入使用者 PDF。維持 `workflow_dispatch`、兩個既有 Runner 與 7 天 Artifact 保存。

## 原始故障的界線

修正前，下載的舊 Windows 完整包與原始碼 Release CLI 已能透過 Mock 完成兩份指定 PDF 的前兩頁、產生單語及雙語 PDF；舊 Windows GUI 也完成第一份的全六頁 Mock。獲授權後，原始碼／舊包分別透過目前的 SiliconFlowFree 完成兩份 PDF 各一頁（該輪先關閉術語擷取以隔離服務測試），有服務重試。

因此不能宣稱「所有舊包的 BabelDOC 0.6.4 或中文路徑壞掉」。已重現同一服務的 HTTP 500，但使用者當時的完整錯誤文字及實際 Worker 啟動狀態未留下紀錄，不能斷定每次故障都是相同原因。Mac 缺少本機執行環境，也沒有本機 macOS Artifact 可供原生驗證；程式與打包檢查不能代替 Mac CLI／GUI 驗收。

### 完整解壓啟動的追加診斷

- 使用者已確認舊 GUI 的完整 EXE 路徑，排除直接在 ZIP 內開啟、只複製 EXE，以及誤開裸 Cargo EXE 的說法。
- 唯讀核對該舊包 12,162 個 EXE／Runtime／Worker／原生庫／資源檔案，以及 182 個既有快取資源：均無缺檔或雜湊差異。只核對隨包 manifest 中的程式與固定資源，沒有讀取私人翻譯快取內容或改寫設定。
- 已再次重現真實失敗：SiliconFlowFree 的服務初始化／預檢收到 **nginx/1.22.1 的 HTML `500 Internal Server Error`**，終止於翻譯健康檢查的既有外層重試耗盡（三次），CLI Exit Code 1、沒有 PDF。此時本機 Worker 尚未啟動、BabelDOC 尚未初始化，因此這次第一個失敗點在上游 API。
- 真實回應沒有「找不到 Python Worker 路徑」文字。原始那段訊息仍未重現，不能把它判作本機缺 Runtime，也不能斷言服務端的內部 Python 發生了什麼錯誤。
- 本機分類測試另外確認：Runtime 完整時，服務端的 JSON／純文字 HTTP 500 可以包含 Worker 路徑文字；診斷仍須標示為上游預檢錯誤，並遮蔽憑證。這只是受控測試，並非真實服務回傳該句文字的證據。

追加證據位於本機 `.test-output/translation-followup/`。上游持續回 500 時，需要等待服務恢復或由使用者改用其他已配置的服務；本機修正保留原有重試，不會偽造譯文、自動改寫私人設定或宣稱已修復遠端 nginx。

## 修正後的實測結果

| 項目 | 結果 |
|---|---|
| 原始碼 Release CLI：PDF 1／PDF 2 前兩頁 | Mock：單語、雙語全部成功，Exit Code 0 |
| 新 Windows 完整包 CLI：PDF 1／PDF 2 前兩頁 | Mock：單語、雙語全部成功，Exit Code 0 |
| Mock：原始碼與新包輸出比較 | 兩份文件的文字、字元座標、圖片、嵌入字型、頁面幾何皆相同；Poppler 96 DPI RGB 每頁像素差異為 0 |
| 真實 SiliconFlowFree：兩份各一頁 | 原始碼測 PDF 1，新包測 PDF 2；保留術語擷取，單語／雙語全部成功，Exit Code 0；另以無 debug 標記的一般模式完成排版驗收 |
| 中文路徑、Unicode 檔名、中文輸出資料夾 | 新完整包實測通過，輸入為原始 PDF 的副本 |
| 原始 PDF／私人設定 | SHA256 驗證未變動 |
| 原始碼／包內 Worker、CLI、GUI | Worker bytes、CLI／GUI SHA256 一致；完整包 13,134 個檔案、85 個 Python distributions，BabelDOC 0.6.4、CPython 3.13.16 |
| 格式、嚴格 Clippy、Release Build | 全部通過 |
| Rust／IPC／Windows 行程清理 | 310 個一般測試通過（含追加的上游 Worker 訊息／遮蔽測試）；預設 ignored 的原生 PDFium 測試另明確執行，1 個通過 |
| Worker／Builder／CI／引擎回歸 | Worker 37、Windows Builder 44、跨平台 Builder 7、CI 9、引擎回歸 4 個通過；Windows 原始碼封裝 22 個整合檢查通過 |
| GUI | 新 Windows GUI 編譯及包內版本檢查通過；舊包 GUI 已操作第一份全六頁 Mock。最終 GUI 操作留待使用者驗收 |
| macOS ARM64 | 相同 Worker、Rust 路徑、84 個 wheels 的閉包與打包測試已檢查；原生 CLI／GUI、實際 Mac Artifact 仍待驗證 |

Mock 測試選擇前兩頁翻譯，輸出仍包含其餘未翻譯頁；驗證會重新解析、渲染所有輸出頁。沒有聲稱真實 API 全文翻譯或翻譯品質已人工校對。詳細日誌與 PDF 保存在本機 `.test-output/translation-fix/`、`.test-output/rust-migration/`、`.test-output/real-api-clean/`，不加入 Git 或發行包。

## 修改檔案

- `crates/babeldoc-bridge/src/lib.rs`：Runtime 先檢查、共用服務階段、Worker 啟動／退出及引擎錯誤鏈。
- `crates/core/src/lib.rs`、`crates/core/src/providers/mod.rs`、`crates/core/src/transport.rs`、`crates/config/src/providers.rs`：重用憑證遮蔽、保留 HTTP 原因及安全狀態碼診斷。
- `python-worker/worker.py`：保留 BabelDOC 原始 Exception、Traceback 及處理階段。
- `script/rust_migration_smoke.py`、`script/ci.py`：真實 PDF 有效性、術語 Callback、來源／完整包測試及失敗日誌。
- `python-worker/tests/test_boundary.py`、`crates/babeldoc-bridge/tests/lifecycle.rs`、其 `fixtures/fake_worker.py`、`crates/core/src/poc_tests.rs`：錯誤、缺檔、遮蔽與既有 IPC 契約回歸。
- `crates/core/tests/provider_compatibility.rs`：追加保留上游 Worker 錯誤及憑證遮蔽的回歸。
- `docs/BUILD.md`、本文件：實測界線與開發啟動方式。另保留工作開始前已存在的 `Cargo.lock` 依賴排序調整。

需重新執行兩平台 GitHub Actions，才能取得含修正的新遠端 Artifact。本次僅建立本機包與 Commit，不 Push、合併、建立 Release 或 Tag。
