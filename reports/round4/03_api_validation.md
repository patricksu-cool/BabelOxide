# 真實 API 與本機傳輸驗證

本輪尚未取得指定服務／模型與費用授權，**真實 provider 請求 0 次**。沒有以預設 SiliconFlowFree、找到的非空欄位或服務名稱推定授權。真實 API 正常翻譯、供應商回應、真實 PDF 中文品質驗收均 **未執行**。

本機唯讀檢查四份 TOML 的敏感欄位是否非空（含範本／合成值），不輸出內容、不驗證 key 有效性、不變更原設定；見 `03_credential_presence.json`。已向使用者詢問可授權服務／模型；沒有回覆不能視為同意。後續授權後應先查官方模型價格與額度，再以一則短句及 PDF 第一頁執行。

## 服務與實際呼叫流程

`03_services.json` 列出 24 個註冊服務及 LLM 能力：包括 OpenAI／OpenAICompatible、DeepSeek、Gemini、AzureOpenAI、SiliconFlow、Ollama、Google、Bing、DeepL、CLI 工具等。這是程式支援清單，不是帳戶可用性或所有供應商已驗證的清單。

Config typed model → `translator.utils.get_translator`／`_create_translator_instance` → 各服務實作 → BabelDOC 翻譯。Factory 會忽略快取執行 `Hello` 健康檢查，因此不能只估算 PDF 文字請求。OpenAI 專案層只重試 RateLimitError，最多 100 次、1→15 秒；SDK 自身另有兩次預設重試。Timeout、Rate Limiting、Concurrency 均未改動。

指定 PDF 第一頁固定回應測試 **33 HTTP 請求（11 批）**，完整四頁 **107（36 批）**。真實譯文／術語／快取／失敗重試會改變數量，這不是付費請求上限。因尚未指定模型與單價，金額估算尚不可成立；費用公式為 input_tokens × input_rate + output_tokens × output_rate，另計重試與健康檢查。不得在取得這些資訊之前發出真實請求。

## 已執行與限制

新增 `tests/test_api_transport.py` 五項測試，以真正 OpenAI SDK 與 loopback HTTP 驗證成功解析與 usage、HTTP 429→成功、400 診斷、空 choices 拒絕成功；Timeout 以 httpx MockTransport 注入，確認 SDK 三次嘗試後傳遞 APITimeoutError，而外層不再重試。只在本機回應以 x-should-retry=false 隔離 429 的 SDK 與外層行為，沒有修改產品重試設定。

| 項目 | 結果 |
| --- | --- |
| SDK／本機 HTTP 成功、解析、400、429、模擬 Timeout | 5 項通過 |
| Retry／Backoff／Rate Limiter／取消 characterization | 原 34 項通過，政策與 AST 保持 |
| GUI 成功／失敗／取消、CLI 失敗傳遞 | 通過 |
| 上述合計 | **54 passed，0 failed，9.29 秒** |
| 第一頁本機 PDF | mono／dual 產生，Key 遮罩檢查通過 |
| 真實 API 及真實翻譯 PDF | **未執行：缺少指定授權／模型／成本核准** |
| 真實供應商 429／Timeout 壓力 | 未執行；依要求使用本機／Mock |

新 HTTP fixture 以 finally 關閉 SDK client、server、thread，確認釋放；既有取消測試檢查 task、IPC queue／pipe 與 worker 結束。這不能取代真實長任務或所有服務失敗後的資源測試。Gradio event loop/socket 警告另外在第五階段分析，不將它隱藏於本階段的正常測試結果。

開發期間新增測試曾誤用 AtomicInteger.get 而失敗；改用既有公開 value 屬性後重跑通過，產品 token 計數未改。完整日誌在 `.test-output/round4/api-local-tests.log`。
