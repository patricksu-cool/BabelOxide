# 第二輪第八階段：架構計畫與執行邊界

前置基準：`63074ab`，160 測試通過、Retry 34 通過，Python GUI 與本機 EXE／指定 PDF 已驗證。依賴風險與既有文件問題見第六、七階段報告。

## 分析依據

使用 codebase-memory 的 Tier 2 查詢、雙向呼叫追蹤與相關檔案 coverage 檢查，再讀取實際定義；Serena 已確認正確專案及 LSP ready。GUI、high_level、Config 和 translator 主要來源沒有記錄的索引缺口。最初猜測的 `config/location.py` 並不存在，設定路徑實際在 `const.py`，已改讀來源。圖的 package/layer 分群結果不適合本專案，未拿來判定完整分層或沒有循環依賴。

現況：`gui.py` 4474 行，包含 45 個已索引函式與大量模組層介面建立；`high_level.py` 826 行；`config/main.py` 692 行；設定模型 483 行與服務模型 1157 行。高風險集中在 GUI 全域元件／設定、跨行程取消與各供應商重試。重構以明確責任邊界為單位，保留既有入口。

## 執行順序

1. **Config 寫入競爭修復**：先以可控制並行測試重現共用暫存檔競爭；把 TOML 編解碼與原子寫入放入 `config/storage.py`。採同目錄唯一暫存檔及原子替換，失敗保留原檔；測試失敗清理、並行完整性、格式與重載。獨立 Commit。
2. **GUI 檔案與 Session 狀態**：建立上傳、清除、逐檔移除、預覽、下載輸出與取消 Characterization Tests；抽至 `ui/files.py`／`ui/session.py`，集中重複的空狀態建立。`gui.py` 保留相容名稱與 Gradio 綁定；測試回傳順序、數量、同一 state 物件及取消次數。獨立 Commit。
3. **GUI 設定轉換**：把元件參數正規化、欄位顯示規則與設定建構抽至 `ui/settings.py`，以明確 context 傳入設定管理器與服務欄位清單；保留原設定格式與公開函式。Rate/QPS 算式原樣搬移並比對邊界測試，不統一供應商策略。獨立 Commit。
4. **GUI 資源分離**：將 CSS／JavaScript 和翻譯資源放入 `ui/resources/`，使用套件資源路徑，保留 CLI／EXE 入口。以獨立修正處理已重現的頁面 JavaScript 函式語法錯誤，檢查瀏覽器預覽和 wheel 內容。獨立 Commit。
5. **翻譯整合分層**：把 BabelDOC 設定組裝與可驗證的資料轉換放入 `translation/`；`high_level.py` 保留相容入口與子行程生命週期。先補設定映射／translator 共用／例外序列化等必要測試，再拆分。IPC、終止、Timeout、Retry、限流器和供應商實作保留原邏輯。獨立 Commit。
6. **建置與結構核對**：核對 wheel 資源與 Windows PyStand 入口，讓建置使用正式 lockfile。依重構結果修正 import、建置設定與文件結構說明；避免無驗證價值的整體 src 目錄搬家。

每次遵循「測試原行為 → 修改單一責任 → 相關測試與 diff → Commit」。完成後重跑第九階段完整回歸，包含 GUI HTTP／瀏覽器、指定 PDF、CLI、Config、Retry 及重新打包 EXE。無法安全驗證的高風險邊界在最終報告明列保留原因，不用格式整理宣稱全面重構。
