# 第八階段：翻譯設定與用量責任分離

基準 `aca28e6`。先新增並執行 **18 項 Characterization Tests，18 passed**，才搬移程式。第一次在受限制環境執行至非同步測試停住；終止該次測試行程後，在允許本機事件迴圈的執行環境重跑成功，沒有視未完成那次為通過。

## 實際變更

- `translation/configuration.py`：抽出 BabelDOC 設定組裝及 CSV glossary 載入。用明確參數傳入 translator factory、config type、glossary loader 及 logger；`high_level.create_babeldoc_config`／`_get_glossaries` 保留原簽名、相容入口與原本的測試替換位置。
- `translation/usage.py`：抽出完成事件的 Token 用量計算，把主翻譯器及術語翻譯器重複的四種 counter 讀取集中。保留分開計數、共享翻譯器扣除術語用量、零 fallback 省略及缺少 counter 預設 0 的規則。
- `high_level.py` 繼續擁有跨行程生命週期、取消、Timeout、IPC、進度傳送與公開例外類別。沒有搬移會影響 pickle module path 的例外，也沒有統一不同供應商的重試策略。

## 驗證

拆分後，設定映射／水印模式與未知值 fallback、主／術語設定分流、建議 QPS／pool 的原有副作用、可選表格模型、glossary 順序、Token 完成事件及 pipe／queue 關閉，連同 Retry、CLI、日誌測試：**70 passed**。供應商原始碼、限流器、子行程取消／終止與 Timeout 算法均未改動。

這一輪以可驗證的整合邊界分層；API client 建立時既有的測試請求、各服務例外及 Tenacity decorators 維持原樣。更動它們需要新的服務別測試，不能用這 18 項整合測試推論安全。
