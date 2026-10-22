# 第二輪第三階段：敏感資訊日誌遮罩（2026-10-23）

## 問題重現與掃描

- 對 `pdf2zh_next/` 的 237 個日誌呼叫位置做字面掃描，重點檢查設定物件、CLI／環境設定、HTTP Header、Token、錯誤與 subprocess 輸出。`translate_engine_model.py` 的 18 個 `GUI_PASSWORD_FIELDS` 包括 OpenAI、DeepL、Azure、Tencent 等 API Key／Secret；自訂 CLI 翻譯器命令也可能含密鑰參數。
- 使用合成 `--openai-api-key` 及 `--debug --version` 重現主程式設定日誌會輸出金鑰片段；RichHandler 會把長字串折行，故只搜尋完整字串會漏判。Config Debug 字典與 CLI 後處理 stderr 也由先失敗的測試證實可輸出合成值。沒有讀取或顯示真實 API Key。

## 修復

- 新增 `pdf2zh_next/security_logging.py`，統一依欄位名遮罩巢狀設定、登記已載入的密鑰值，並在日誌處理器輸出前清理常見 Authorization／Bearer／Basic、URL 密碼及 `key=value` 型式。涵蓋 root RichHandler、GUI 入口與跨行程 QueueHandler；例外保留型別及堆疊位置，省略原始例外訊息。短於六字元的未標記密鑰不加入全域值登記；有欄位名的值仍會遮罩。
- `ConfigManager` 在讀取設定與 CLI／環境變數後登記憑證；Debug 日誌使用遮罩後的副本，不修改 `SettingsModel` 或設定檔格式。設定讀取、轉換與驗證錯誤只輸出例外型別，避免例外內容引用原始設定。
- 主程式不再直接輸出 `SettingsModel`。未處理的 CLI 例外經遮罩後記錄，仍以非零狀態結束；日誌格式與原始 traceback 文字有所變化，但退出狀態沒有改變。
- Claude Code 與自訂 CLI 翻譯器不再將命令、輸入文字、stdout 或 stderr 原文寫入日誌，只保留輸入／輸出長度、參數數量與退出碼。實際 subprocess 呼叫、回傳與 Retry／Backoff 演算法未改。

## 驗證

- `tests/test_security_logging.py` 共 **11 passed**：18 個敏感欄位、巢狀設定不被修改、自訂 CLI 命令、Header、URL 密碼、普通／Debug CLI、可識別與未知例外、Claude Code 輸入輸出，以及 CLI 後處理錯誤均使用合成字串驗證沒有輸出其值。部分測試先確認原實作失敗，再於修復後通過。
- 完整測試 **151 passed、68 warnings、0 failed**；`ruff check .`、新增模組與測試的格式檢查、`git diff --check` 通過。PDF、GUI 與 EXE 的完整實際流程留待第四階段重跑。

## 界限

遮罩以已辨識的設定欄位、已登記值及常見 Header／URL 格式為準。未經本應用程式日誌處理器的第三方直接寫入 stdout/stderr，以及完全未知、無標籤又未登記的敏感字串，無法由此機制全面保證；真實供應商故障訊息也未測。安全報告將保留這些風險。CLI／GUI 顯示給操作使用者的錯誤內容屬另一條路徑，本階段只修日誌。
