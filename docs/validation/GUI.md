# Rust Release GUI 實測

日期：2026-10-31。被測程式為 `dist/hybrid-diagnostic-20261030-v3/pdf2zh-gui.exe`，SHA256 `a46c882eefd1e8139f749157b3bd85084ed862fd8911a329f5f7c1a80f4f3108`。這是正式最佳化的 Release EXE 桌面操作，不是 Debug 或只建立視窗。

指定 PDF 現位於 `Downloads/論文/High_speed_buffer_latch_ISCAS03.pdf`，SHA256 `d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f`，與第四輪基準相同。以 `script/gui_smoke.py` 的固定回應 loopback API 測試，設定、HOME、TEMP、CWD 與輸出隔離。未使用真實 Key、遠端翻譯 API 或付費服務。

| 功能 | 實際觀察 |
| --- | --- |
| 啟動、關閉、重開 | 兩次正常退出，exit 0；重開後正確載入保存的模型、合成 Key 與 zh-TW 語言設定 |
| PDF 選擇 | 原生選檔對話框實際選入四頁指定 PDF；保存設定不保留此次輸入檔 |
| 服務與 API 設定 | OpenAI → AliyunDashScope → OpenAI；模型與合成 Key 可編輯，Key 保持遮罩；設定保存成功 |
| 語言設定 | en → zh-TW 保存重開核對，再還原 zh-CN 以對照既有 PDF 基準 |
| 翻譯、進度與輸出 | 完整四頁顯示 Complete 1/1、100%；單語與雙語輸出各四頁 |
| 預覽 | 原文、Translated、Bilingual；兩種結果的第二頁可顯示；1.25 → 1.75 與 Fit page 可用 |
| 另存 | 雙語結果另存至新檔 `saved-dual.pdf`，SHA256 與原輸出相同（`a22686c295ed10c819702dbd0888c0bf90fa6c5b3491b366153d906346fda2c6`） |
| 錯誤與再次執行 | 本機回傳 HTTP 400，GUI 顯示 Failed 0/1 與錯誤原因；之後能再次開始翻譯 |
| 取消 | 引擎載入中按 Cancel，先顯示 Cancelling and cleaning up，之後為 Cancelled；仍能預覽原文 |
| 處理中關閉 | 下一輪引擎載入時關閉，顯示 Cancelling tasks and closing safely，清理後視窗消失；被測 GUI 與包內 Worker 無殘留 |

合計 112 次本機請求：111 次固定回應、1 次受控錯誤，36 batch；兩次 GUI exit 均為 0，log 無合成 Key。證據位於 `.test-output/final-validation/v3-gui-host/summary.json`。

單語／雙語對照 `.test-output/round4/rust-main-baseline`：文字、字元座標、圖片一致；八頁 Poppler 96 DPI RGB 像素差為零，見 [比對數據](v3-gui-pdf.json)。人工檢視了原文與結果頁面、公式及圖表；未修改 BabelDOC 排版。

限制：本輪沒有原生桌面拖放操作，既有 egui drop 測試仍保留；未逐一操作所有進階欄位或呼叫所有真實服務。首次在沙箱中啟動的測試無法正確操作原生對話框，已停止且不計為通過；以上結果來自正常桌面權限的隔離測試。這是開發機驗證，沒有宣稱乾淨 Windows VM 或未安裝 VC++ Runtime 的條件已通過。最終新包另須回歸。
