# 最終狀態

更新：2026-11-01（Asia/Taipei）。本輪 GUI 改善與 Windows 自用版已完成驗證；Git 最終合併檢查列於下方。新版本位於 `dist/gui-ux-20261101/`，原 `dist/final/`、使用者設定、快取及 PDF 均保留。

## 使用方式

執行 `C:\Users\user\Documents\PDFMathTranslate-next-2.9.0\dist\gui-ux-20261101\pdf2zh-gui.exe`。選服務、模型／API 網址／Key 與語言，加入 PDF，按「開始翻譯」。可預覽與另存單語／雙語結果，或從「匯出批次 ZIP」選全部、譯文、雙語 PDF。

請複製完整資料夾，保留 `runtime/`、`babeldoc/`、`resources/` 和 `native/`，不需另外安裝 Python、Rust、uv 或 BabelDOC。仍需系統 VC++ x64 Runtime；未宣稱乾淨 Windows VM 或零系統依賴驗收。舊包的私人設定沒有自動複製到新包。

按「保存設定」才寫入 EXE 旁 `config/config.v3.toml`；PDF 輸入清單不保存。快取為 `cache/translations.v2.db` 與 `cache/babeldoc-home/`；模型／字型種子保留在 `resources/`。唯讀位置沿用既有的使用者資料備援機制。API 網址也依原 schema 的敏感標記遮罩；開始翻譯的健康檢查可能計入服務用量。

## 本輪完成項目

- 語言改為官方 153 項清單的下拉、名稱、交換及進階自訂代碼；保留舊代碼。Qwen-MT／DeepL 依本地核心可證實的限制過濾，不臆測其他服務的支援範圍。
- 重新盤點 235 個設定：100 Bool、120 String、10 Integer、4 Float、1 StringSet。基準有 196 個通用呈現欄位，其中 62 個原已為勾選框，並非 235 個文字框。新介面提供布林／三態、列舉、數值、遮罩、路徑、多行與型別驗證，按服務及模式顯示並分組進階欄位。
- `enabled_services` 套用正文及術語選單，可管理服務；停用不刪除 profile。DeepSeek thinking／reasoning、發送參數等採條件式控制；無效既存設定保留修正入口。
- 新增全部／第一頁／前五頁快捷及自訂範圍，保留不連續及開放端語法。術語顯示相容性、啟用及限流提示；保留術語 pool 的 0／空值繼承語意。
- 新增公開 HTTP(S) PDF 下載：100 MiB、總逾時 60 秒、連線 10 秒、最多五次重新導向、標準連接埠、DNS 與遠端位址檢查、公網限制、PDF 驗證及取消。修復冷啟動首次下載的 TLS 初始化；不使用 Python 網路程式碼。
- 新增 Rust ZIP 的全部／單語／雙語匯出、重名序號、CRC／內容驗證與原子保存。同一批次、同一輸出資料夾中的同名 PDF 使用獨立 `pdf2zh-batch-…/job-…` 子資料夾，避免 BabelDOC 覆寫前一份結果。
- PDF 整批加入先驗證後更新；保存／提交使用經驗證的 snapshot，錯誤草稿保留，保存回覆不覆寫較新編輯。修復重載語言、下載取消控制與移除前方輸入後預覽選取的同步問題。
- 預設繁體中文，固定 UI、全部 schema 欄位名稱及已知引擎階段集中管理。保留原生桌面、左右布局、PDFium 與 Rust＋BabelDOC 架構。

`crates/core/`、`crates/tasks/`、`crates/babeldoc-bridge/` 和 `python-worker/` 相對本輪 main 基準沒有修改；Retry／Backoff、限流／並行、快取、IPC 與 BabelDOC 演算法沿用原版。Builder 僅新增暫時 Windows 存取／共用鎖的有界重新命名重試，不修改 ACL，也不覆寫舊包。

## 驗證結果

| 項目 | 結果與範圍 |
| --- | --- |
| Rust | Workspace 292 passed、0 failed、1 預設 ignored；其中 GUI 64 項。fmt、Clippy `-D warnings`、Release workspace build 通過 |
| PDFium | 原生 ignored 測試已明確執行：1 passed；新版 GUI 也實際載入四頁 PDF、翻頁、縮放、切換單語／雙語及另存 |
| Worker／Builder | Worker 35 passed；Builder 35 passed，包含暫時目錄鎖重試與不覆寫既有成品 |
| GUI 操作 | 語言／交換、服務切換與 profile 保留、停用 Google、nullable bool、保存／重載／重開、PDF 加入／移除、頁面快捷與範圍、術語啟用欄位、冷啟動 W3C HTTPS PDF 下載通過 |
| 批次及匯出 | 1 頁與 4 頁的兩份同名 PDF 均完成；全部 ZIP 4 檔、mono ZIP 2 檔、dual ZIP 2 檔，均可解壓，CRC／唯一名稱／來源 SHA256 一致；單份 PDF 另存雜湊一致 |
| 錯誤與取消 | HTTP 400 後同批次重新執行成功；延遲 API 在翻譯段落階段取消後成為 Cancelled；設定／日誌沒有合成 Key 洩漏 |
| 程序與設定 | 完整流程 GUI 兩次 exit 0，重開後模型／Key／繁中保留、輸入清單為空；最終完整包在 Worker 運作中關閉亦 exit 0，GUI 24224／Worker 14520 均退出 |
| 最終包 CLI | 中文＋空格完整複本、非原始碼 CWD、預設包內 Python／Worker／離線資產；指定 PDF 四頁完整翻譯，exit 0、未逾時，mono 471,435 bytes、dual 771,107 bytes |
| PDF 回歸 | 對 main `06ac769` 固定 identity 回應：mono／dual 各四頁，文字、逐字元座標、圖片及每頁字型資料／嵌入字型串流 SHA256 相同；八頁 Poppler 96 DPI RGB 渲染像素差 0；公式與圖表無新增差異 |
| 完整成品 | 發行資料夾及隔離複本各 13,070 個 manifest 項目、1,248,861,672 bytes（不含 manifest）；全檔 size／SHA256／集合相符；交付包沒有私人 config、輸出 PDF 或可變 cache |

PDF 的實際位置為 `Downloads\論文\High_speed_buffer_latch_ISCAS03.pdf`，與指定檔名相同，SHA256 為 `d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f`。全部翻譯使用本機固定回應，沒有付費服務呼叫；外部網路僅實測公開 W3C PDF 下載，不代表翻譯品質驗收。

完整桌面流程使用同名輸出修正的 Release（GUI SHA256 `a4a5967641a9ac65821b68fb0e36e38223e5d1a54b4b1fa75307ba3e954f3de0`）。之後只補齊四個引擎階段中文；最終包 GUI SHA256 為 `c8831d251bddc964b64f58f31234018c86cec66eeb9f67ffa418c64e9314c797`，已另行驗證原生選檔、四頁 PDFium、包內保存、中文引擎進度與處理中關閉；其完整包 CLI 及 PDF 回歸亦通過。

量化證據見 [本輪驗證資料](validation/gui-ux-verification.json)，操作範圍見 [GUI 實測](validation/GUI.md)。235 欄對照與官方來源見 [設定盤點](GUI_SETTINGS.json)、[相容性稽核](GUI_UX_AUDIT.md)。2026-10-31 的驗證 JSON 留作舊包歷史紀錄，不是新包的成品雜湊。

## 官方差異、版本與限制

官方 v2.9.0 比較依 tag 原始碼，未下載或操作官方大型 Windows 包。保留原生布局與 PDFium；官方術語 CSV 內容預覽及術語 ZIP 仍未移植，目前支援 CSV 路徑選擇與單份術語檔另存。第三方服務回傳的技術文字與未知階段保留原文。URL 採公網限制，不接受本機／內網 URL；下載取消、DNS、重新導向及錯誤邊界由受控測試驗證，未逐一透過真實外網手動重演。

本輪保留 **BabelDOC 0.6.3**。0.6.4 的光柵預算及混排行距修正有後續評估價值，但會涉及排版變動；Worker 握手、依賴與資源仍鎖定 0.6.3，沒有足夠理由在 GUI 任務中強制升級。完整 tag 差異及來源見稽核文件。

未逐一連線各服務真 API、操作全部 235 欄或全部模型組合；未測乾淨 Windows VM、不同實體磁碟與真實唯讀 ACL。既有 Tencent 動態憑證鏈、HTTPX timer／proxy／CA 等相容邊界見 [服務相容性](PROVIDERS.md)。超長 GUI 路徑仍受既有短檔名機制限制。`BUILD_INFO.release_ready=false` 保留公開再散布／乾淨機 gate，本次交付為已在此機驗證的個人使用版本。

## Git 與重建

程式碼在 `feature/gui-ux-improvements` 分階段提交；原 main 基準為 `06ac769`，最終包程式碼提交為 `a9d6dd1`。原 main 沒有額外未整合修改；最終 fast-forward 及合併後檢查尚待完成。未 Push、未建立 GitHub Release。

重建方式見 [DEVELOPMENT](DEVELOPMENT.md)；排解見 [TROUBLESHOOTING](TROUBLESHOOTING.md)。
