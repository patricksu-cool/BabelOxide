# Rust Release GUI 實測

## 2026-11-01 GUI 改善

本輪實際操作及成品識別記錄於 [最終狀態](../FINAL_STATUS.md) 與 [驗證資料](gui-ux-verification.json)。使用 Computer Use 控制 Release 原生視窗、原生開啟／另存對話框及本機 identity／HTTP 400／延遲 API；沒有真實 API Key。

| 本輪操作 | 結果 |
| --- | --- |
| 語言／服務 | 下拉、交換、OpenAI／Google／DeepSeek 動態欄位、切回 profile 保留、停用 Google 後從選單消失、可空布林 |
| 編輯／設定 | 原生加入與移除 PDF；保存、重載及關閉重開；模型、合成 Key 與繁中保留，輸入清單不保存 |
| 頁面／術語 | 第一頁填入 1、前五頁填入 1-5、自訂 1,3-4、全部清空；自動術語勾選控制保存與限流欄位的可編輯狀態 |
| URL | 冷啟動後首次網路動作成功下載 W3C dummy.pdf 並以 PDFium 預覽；內網／重新導向／大小／下載取消邊界由受控測試涵蓋 |
| 同名批次／ZIP | 兩份 report.pdf 分別為 1、4 頁，完整翻譯兩次（其中一次在 HTTP 400 失敗之後）；三種 ZIP 均解壓與雜湊驗證成功 |
| 預覽／另存 | 單語及雙語四頁文件第二頁、縮放 1.25→1.75；單份雙語另存與來源雜湊一致 |
| 取消／退出 | 延遲 API 在翻譯段落階段取消成功；完整流程兩次 exit 0；最終包 Worker 14520 運作時關閉，GUI 24224 與 Worker 均退出、exit 0 |

完整流程 GUI 為 `a4a596…f3de0`；最後只補齊四個進度階段中文的最終 GUI 為 `c8831d…4c797`，再次實測原生選檔、包內設定保存、四頁 PDFium、中文載入進度及處理中關閉。最終完整包的 CLI 翻譯、逐檔雜湊及對 main 的 PDF 比對亦通過。未宣稱每個服務／進階設定均已手動測試。

## 2026-10-31 舊包歷史驗證

2026-10-31。最終 `pdf2zh-gui.exe` SHA256 為 `cb39d28285d2dc51d61d57bc587cbd1977105041dc93fdbddae1885708657a05`，與 `dist/final/` 相同；使用正式最佳化 EXE、內附 Python／Worker／PDFium 及本機 mock，沒有真實 Key 或外部翻譯請求。

指定四頁 PDF 的 SHA256 為 `d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f`。測試複本位於中文空格目錄，HOME、TEMP、CWD 與輸出隔離；未指定開發 Python、Worker、venv 或 app-dir，保存至複本自己的 config。

| 最終 EXE 桌面操作 | 結果 |
| --- | --- |
| 原生拖放與選檔 | 從原生對話框拖放隔離 PDF 至主視窗，取消對話框後清單為 1、原文預覽四頁；重開後再選入原始 PDF |
| 設定保存重載 | 模型改為 `offline-final-verified` 並保存，關閉重開後保持；輸入 PDF 清單沒有保存 |
| 翻譯進度與輸出 | Complete 1/1、100%；mono／dual 各四頁 |
| PDF 預覽與另存 | 原文、Translated 第二頁、Bilingual 第二頁正常；另存副本與雙語原輸出 SHA256 相同 |
| 失敗與再次執行 | 本機 HTTP 400 顯示 Failed 與原因；恢復 identity 後可再次執行 |
| 實際取消 | 引擎處理中按 Cancel，最終顯示 Cancelled／87%；視窗仍可操作 |
| 處理中關閉 | 重開後再次翻譯，Worker PID 16308／GUI PID 40112 活躍，Parse Page Layout 21% 時關閉；兩個 PID 均退出 |
| 結束與日誌 | 兩次 GUI exit 0；217 次本機請求，216 identity＋1 受控錯誤、72 batch；log 無合成 Key |

最終 mono／dual 與原 Python 基準的文字、逐字元座標、圖片一致，八頁 Poppler 96 DPI RGB 像素差 0。完整本機數據位於 `.test-output/final-validation/final-gui-v5/`、`final-gui-v5-pdf.json`；可攜與 artifact 身分見 [最終驗證資料](final-verification.json)。

v3 基準先前已實測 OpenAI → AliyunDashScope → OpenAI、模型／Key 編輯、en／zh-TW 保存重開、恢復 zh-CN、1.25 → 1.75 縮放與 Fit page，以及同等翻譯／錯誤／取消流程。當時 EXE SHA256 為 `a46c882eefd1e8139f749157b3bd85084ed862fd8911a329f5f7c1a80f4f3108`，112 次本機請求、兩次 exit 0；[v3 PDF 數據](v3-gui-pdf.json) 保留作為移植基準。

268 字元實體 GUI 路徑已透過既有短檔名自動開出視窗；桌面工具對短檔名 EXE 的應用程式授權逾時，該長路徑視窗完整操作不列為通過。上述完整流程在較短中文空格目錄進行，EXE／Worker hash 與正式包一致。未測乾淨 VM、不同實體磁碟、所有進階欄位或真實服務；唯讀備援由注入測試驗證。
