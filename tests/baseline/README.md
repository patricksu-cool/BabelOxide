# Frozen Python characterization baseline

Historical record from the old Python application; preserved without interpreting it as a current test result. Executable source and the original test remain in Git at recovery tag `codex/baseline-final-integration-20261031`. Later retry characterization changes and Rust contracts are recorded separately; the older Tencent xfail in this historical report must not be read as the current status.

# 第五階段：Retry / Backoff 原始行為紀錄（2026-10-22）

## 盤點範圍

`translator/translator_impl/` 的 16 個服務含 **22 個 Tenacity 裝飾器**（SiliconFlowFree 同一方法疊兩層）；已逐一盤點並在 `tests/test_retry_characterization.py` 以原始裝飾器查驗次數、等待與可重試例外。Tencent 的目前 SDK 無法匯入，因此該服務只以 AST 固定原始碼中的策略，另用 strict xfail 明示執行障礙。掃描其他程式檔時未發現額外的本專案 Tenacity 裝飾器或明確的 `Retry-After` 解析；第三方 SDK 自有重試不在這項結論內。

| 服務／方法 | 本專案最大嘗試次數（含首次） | 失敗後等待秒數 | 可重試例外 |
|---|---:|---|---|
| AnythingLLM、Azure、Dify、Google 的 `do_translate` | 5 | 1, 2, 4, 8 | 所有 `Exception` 子類 |
| Bing、DeepL、Tencent 的 `do_translate` | 5 | 1, 2, 4, 8 | `retry_if_exception(Exception)` 的結果；原始寫法甚至對 `KeyboardInterrupt` 判為應重試 |
| AzureOpenAI、OpenAI、SiliconFlow 的一般／LLM 方法；QwenMt 一般方法 | 100 | 1, 2, 4, 8, 後續最多 15 | `openai.RateLimitError` |
| Ollama 的一般／LLM 方法 | 100 | 同上 | `ollama.ResponseError` |
| Xinference 的一般／LLM 方法 | 100 | 同上 | `RuntimeError` |
| ClaudeCode、CLITranslator 的一般方法 | 3 | 2, 4 | `CalledProcessError`、`TimeoutExpired`（ClaudeCode 內部會把逾時改成 `ValueError`，實際因此不重試） |
| SiliconFlowFree 的 LLM 方法，內層 429 | 100 | 4, 4, 4, 8, 16, 32, 64, 後續最多 120 | 自訂 `RateLimitError` |
| SiliconFlowFree 的 LLM 方法，外層 HTTP | 3 | 30, 30 | `httpx.HTTPError`；內層 429 耗盡後的 `RetryError` 不會觸發外層 |

以上策略使用 `wait_exponential`，**本專案裝飾器沒有 jitter**；同一次策略呼叫的等待值可重現。停止前最後一次失敗一般由 Tenacity 封裝為 `RetryError`，其 `last_attempt.exception()` 保留原錯誤；不符合策略的錯誤直接傳遞。SDK 內部的最大次數、隨機等待或 `Retry-After` 尚未由這些測試控制。

## Mock 執行結果

- Google：429、503、`requests.Timeout` 都嘗試 5 次，虛擬等待為 1/2/4/8 秒；429 的 `Retry-After: 120` 不改本層等待。HTTP 400 回傳原程式的 `IRREPARABLE TRANSLATION ERROR` 字串而不重試。`Session.get` 沒有 timeout 參數。
- Bing：429 嘗試 5 次，1/2/4/8 秒；每次重試重新取得 SID。Mock 中 `KeyboardInterrupt` 也會再試，證明現有 `retry_if_exception(Exception)` 的不尋常語義。`Session.get`/`post` 沒有 timeout 參數。
- OpenAI：外層裝飾器只對 `RateLimitError` 重試，100 次上限；HTTP 500 的 `InternalServerError` 和 `APITimeoutError` 直接傳遞。模擬 `Retry-After: 120` 時，本層仍等 1/2 秒。Mock 取代 SDK 呼叫，因此未驗證 SDK 自己的重試層。
- SiliconFlowFree：429 在內層至多 100 次，等待最長 120 秒；503 所引發的 `httpx.HTTPStatusError` 由外層至多 3 次，等待 30/30 秒。兩層沒有簡單相乘成 300 次；內層失敗的 `RetryError` 不符合外層 `httpx.HTTPError` 條件。
- CLITranslator：逾時後殺掉並回收 Mock 子行程，至多 3 次，等待 2/4 秒，最終 `RetryError` 保留 `TimeoutExpired`。ClaudeCode 逾時後殺掉 Mock 子行程，但改拋 `ValueError`，只呼叫一次；其清理路徑沒有再次 `communicate`，需另行檢驗真實行程資源。
- `BaseTranslator.translate` 只在進入 `do_translate` 前等 QPS 一次；服務內部重試不再向限流器取令牌。QPSLimiter 以單調時間、鎖與睡眠間隔排定開始時間（2 QPS 的虛擬等待 0.5/0.5 秒），但不限制已開始請求的並行數；並行數另由 BabelDOC 工作池設定控制。
- 模擬取消 `_translate_in_subprocess` 的 async 工作：收到 `CancelledError` 後會通知取消、結束 Mock 子行程、關閉已列的 pipe/queue 並等待兩個接收 thread；測到 callback timeout 是 30 分鐘。這是隔離資源的單元測試，不等於真實 Windows 行程強制中止的完整驗證。

## 已發現但暫不改行為的問題

1. **Tencent 服務無法匯入**：現有 `.venv` 的 `tencentcloud-sdk-python-tmt 3.1.129` 在 `tencentcloud.tmt.v20180321.models` 缺少 `TextTranslateRequest`／`TextTranslateResponse`；原始碼匯入即失敗。基準套件未涵蓋此服務。應獨立釐清 SDK 相容版或服務介面，先補可驗證的離線初始化與回應測試，再作修復。
2. Bing、DeepL、Tencent 的 `retry_if_exception(Exception)` 會把中斷判為重試；其餘一般服務的 `retry_if_exception_type(Exception)` 不會。取消語義有服務差異。
3. 遇到 429/5xx 時，某些服務連 4xx 非限流錯誤也會重試；每段文字最多 100 次的服務在高並行時可能有大量重複請求。本地 QPS 間隔不覆蓋每一次重試。
4. Google/Bing HTTP 呼叫缺少明確 timeout；外層最大重試次數不會保證單次請求能及時完成。
5. 未發現本專案自身依 `Retry-After` 決定等待的邏輯；不同 SDK 或伺服器端可能另有行為。

## 測試與保護結論

- 指令：`python -m pytest -q tests/test_retry_characterization.py --tb=short --show-capture=no --disable-warnings`，結果 **33 passed、1 xfailed**。xfail 嚴格標記 Tencent SDK 匯入障礙；若日後修復會轉為需重新評估的 XPASS。
- 全部等待透過 Tenacity `sleep` Mock 或單調虛擬時間檢驗；沒有真實 API、付費呼叫或長時間睡眠。取消測試使用 Mock process、pipe、queue 與 thread。
- **未修改任何 Retry / Backoff 演算法或執行路徑。** 第七階段若無新增充分測試，這些服務保留原實作。使用真實供應商時的 SDK 內部重試、配額與網路行為仍未驗證。
