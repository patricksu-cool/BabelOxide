# GUI 設定轉換分層

拆分前新增 17 項 Characterization Tests，全數通過：位置參數與主／術語服務欄位順序、RPM／執行緒／Custom 的八組邊界、四組拒絕條件，以及 never／always／follow_settings 的保存與當次翻譯值。

原 602 行的 11 個符號拆到 `ui/settings.py`、`ui/fields.py`、`ui/rate_options.py`。設定建構以 `SettingsContext` 明確接收 ConfigManager、語言與頁碼對照、敏感欄位限制及保存後回呼；位置參數正規化接收實際服務欄位清單和 glossary 轉換函式。`gui.py` 保留原函式簽章及 SaveMode 名稱，保存後更新全域設定的行為仍由相容包裝函式執行。

六個欄位 helper 與兩個 Rate 轉換函式的 AST 與搬移前完全相同；未更動 QPS、Concurrency、Timeout、供應商 Retry 或 Backoff。完整測試 **191 passed、67 warnings**，ruff 與 diff 檢查通過。真實 Gradio HTTP 事件另完成上傳、設定保存／重載、翻譯、單語與雙語各一頁及含兩份 PDF 的 ZIP 下載（3 次本機 Mock 請求）。

設定轉換仍保留原來詳細的欄位映射，避免在同一次搬移中改寫各服務型別與敏感設定處理。
