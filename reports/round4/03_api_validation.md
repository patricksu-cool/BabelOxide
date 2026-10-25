# 真實 API 與本機傳輸驗證

使用者回覆「可用 siliconfree」後，使用 SiliconFlowFree 完成短句、指定 PDF 第一頁的 GUI 與 CLI 真實翻譯。這個免費服務不需要 API Key；沒有使用查得的本機憑證，沒有修改使用者既有設定。完整文件真實翻譯未執行。

## 授權、服務與成本

依[專案官方 SiliconFlow 說明](https://pdf2zh-next.com/advanced/TranslationServices/SiliconFlow.html)，SiliconFlowFree 為免費服務，經專案維護者代理轉送 SiliconFlow。執行前以零費用、一則短句、兩次第一頁 PDF smoke test 說明範圍。實際代理回應沒有揭露模型版本，因此不把文件中的預設模型名稱寫成已確認實測模型。供應商未揭露的 token 數與 PDF 全部 HTTP 嘗試數不作推估。

短句測試記錄了 8 個 HTTP 200：兩個端點各三次 `/check`、一次 `/config`、一次翻譯。選用 api1，耗時 5.70 秒，回覆「高速緩衝鎖存器可減少傳播延遲。」，client 已關閉。見 `03_real_api_smoke.json`。PDF smoke 保留原有健康檢查、動態 QPS、並行及重試行為；沒有為真實服務故意製造 429、Timeout 或大量壓力。

最初唯讀檢查四份 TOML 的敏感欄位是否非空（包含範本／合成值），見 `03_credential_presence.json`；非空不代表有效或已授權。沒有輸出值或將其送往真實服務。

## 已執行結果

| 項目 | 結果 |
| --- | --- |
| 真實短句 | 通過；免費、無 key，8 個 HTTP 200 |
| 新 EXE GUI 第一頁 | SiliconFlowFree，mono 129,735 bytes、dual 152,058 bytes；各 1 頁、1,110 個 CJK 字元 |
| 原始碼 CLI 第一頁 | exit 0；mono 153,299 bytes、dual 175,610 bytes；各 1 頁、1,103 個 CJK 字元 |
| GUI 預覽、下載 | 雙語預覽可見；瀏覽器單語下載成功；mono／dual／雙檔 ZIP 經實際下載連結驗證 |
| 設定儲存／重新讀取 | 新測試 EXE 的獨立 config；191 個 UI 設定重新讀取成功 |
| SDK 本機 HTTP 成功、usage、400、429、模擬 Timeout | 新增 5 項通過 |
| Retry／Backoff／Rate Limiter／取消 characterization | 原 34 項通過；26 個檔案／函式 AST 與穩定基準一致 |
| 傳輸、重試、GUI 任務、CLI 失敗傳遞合計 | 54 passed，0 failed，9.29 秒 |
| 真實供應商 429／Timeout／取消壓力 | 未執行；錯誤情境使用本機／Mock |
| 所有服務、付費模型、完整文件品質 | 未驗收 |

PDF 指標見 `03_real_api_pdf.json`、GUI 下載見 `03_real_api_gui.json`，實際瀏覽器畫面見 `06_exe_real_api_preview.png`。GUI 預設輸出是 zh-CN，CLI 明確指定 zh-TW；不是相同譯文的排版對照。沒有假設兩次真實輸出應逐像素相同。

以 Poppler 96 DPI 目視檢查 CLI 單語及 GUI 雙語第一頁：中文文字、電路向量圖與公式均存在。向量繪圖數 mono 496、dual 993；embedded raster images 為 0，不能據此認定電路圖遺失。仍可見公式附近重疊及術語／斷句問題，CLI 有 formula placeholder fallback 警告；因此只確認翻譯鏈路與輸出可用，不宣稱中文品質、公式語義或排版已完整驗收。固定譯文的原版回歸分析見 `04_pdf_layout.md`。

CLI 日誌末端的 BabelDOC「Translation canceled」發生在 mono／dual 已寫出後的進度監聽清理；程序 exit 0、兩份輸出可解析且已渲染。實際上報翻譯呼叫為主翻譯 16 次、另一 translator 1 次，cache 0；這不是完整 HTTP 請求計數。

## 錯誤情境與行為保留

`03_services.json` 是 24 個已註冊服務的支援清單，不能視為全部供應商已驗證。Config typed model → translator factory → 各服務實作 → BabelDOC；factory 的 `Hello` 健康檢查另計請求。

`tests/test_api_transport.py` 使用真正 OpenAI SDK 與 loopback HTTP 驗證解析、usage、HTTP 429 後成功、400 診斷、空 choices 拒絕成功。Timeout 以 httpx MockTransport 注入，確認 SDK 三次嘗試後傳遞 APITimeoutError，外層不再重試。本機 429 fixture 的 x-should-retry=false 只用來隔離 SDK 與外層重試，產品設定沒有變更。finally 關閉 SDK client、server 及 thread。

OpenAI 外層仍只重試 RateLimitError，最多 100 次、1→15 秒；SDK 自身另有兩次預設重試。SiliconFlowFree 原有 HTTPError／RateLimit 重試、Timeout 與代理回傳並行限制保持。GUI 成功／失敗／取消測試及 Windows worker 取消驗證另見第五階段。沒有以真實付費 API 重複失敗來測試這些情境。

測試 harness 曾因 Windows spawn 重新執行未加 main guard 的 wrapper 而失敗；只修正 `.test-output/` 內 wrapper 後重跑，產品程式未修改。成功的 CLI 日誌是 `real-api-cli-verified.log`，不可混用先前失敗日誌。
