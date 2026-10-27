# Retry / Backoff 相容性基準

日期：2026-10-26。Python 基準：`d18165e`；開發分支：`feature/rust-babeldoc-hybrid`。

## 驗證狀態

本文件是移植契約，**不是 Rust 等價或正式發行證明**。目前 Rust core 是 OpenAI-compatible PoC，尚未移植完整 retry、持久快取、token accounting 或全部 provider。下表 Rust case 名稱是待實作對照，不代表已有通過結果。

已檢讀現行 `tests/test_retry_characterization.py`、`tests/test_api_transport.py`、16 個 provider、`BaseTranslator`、`QPSRateLimiter`、`high_level` 與 `AsyncCallback`。相關專案檔案的 codebase-memory Tier 2 coverage 為 `no_recorded_issue / metadata_match`；provider scope 只有 `__pycache__` 排除。動態 decorator/generator 另讀 source。`.venv` 不索引，SDK 結論以本機鎖版原始碼補查。

既有 retry 是 **34 cases = 22 個 policy 參數案例 + 12 個行為案例**；另有 5 個 API transport cases。設計審查以隔離 `USERPROFILE` 的 `pytest --collect-only` 確認共 39。父代理已回報同一基準完整 pytest 253、retry 34 通過；本文件不另宣稱重複執行成功。`reports/05_retry_characterization.md` 的 33 passed / 1 xfailed 是舊紀錄，不能沿用；Tencent 相容 SDK fixture 現在是普通案例。

## 計數、等待與錯誤

`max_attempts=N` 包含首次呼叫；全部失敗只有 N−1 次 sleep。最後失敗不再 sleep。本地 Tenacity wait 沒有 jitter，也不讀 Retry-After；SDK 內層可能兩者都有。

| 代號 | 本地 wait，n 從 1 開始，單位秒 | policy probe 序列 | 真正耗盡前等待 |
|---|---|---|---|
| E | `min(15,max(1,2^(n-1)))` | 1、2、4、8、15 | 5 attempts 只睡 1、2、4、8；100 attempts 共 99 次 |
| P | `min(15,max(2,2*2^(n-1)))` | 2、4、8 | 3 attempts 只睡 2、4 |
| H | `min(60,max(30,2^(n-1)))` | 30、30、30 | 3 attempts 只睡 30、30 |
| F | `min(120,max(4,2^(n-1)))` | 4、4、4、8、16 | 100 attempts 共 99 次，封頂 120 |

| family | Python selector | 必須保留的差異 |
|---|---|---|
| all | `retry_if_exception_type(Exception)` | 一般 Exception 符合；KeyboardInterrupt 不符合 |
| predicate | `retry_if_exception(Exception)` | 呼叫 `Exception(error)` 作 predicate，連 KeyboardInterrupt 都 truthy；不是上列型別檢查 |
| openai | `openai.RateLimitError` | 應用層只選此型別；500、timeout 不由應用層重試 |
| ollama | `ollama.ResponseError` | 所有 ResponseError，並不限 429；ConnectionError 不同類 |
| process | CalledProcessError / TimeoutExpired | 必須看方法是否把例外轉型；Claude timeout 就會轉型 |
| httpx | `httpx.HTTPError` | SiliconFlowFree 外層 status/transport error |
| free429 | 自訂 RateLimitError | SiliconFlowFree 內層，不是 OpenAI SDK 類別 |
| runtime | RuntimeError | Xinference 方法內會 catch Exception，最後改拋一般 Exception |

現有 decorator 沒設 `reraise=True`。符合 selector 耗盡後是 `RetryError`，保留最後例外；不符合的直接傳出。Rust 可用自己的型別，但需區分 `RetryExhausted { layer, attempts, last_error }` 與直接錯誤，不可都壓成字串或空翻譯。

IPC cancel 是獨立控制事件，不能偽裝成 provider predicate 會重試的普通錯誤。原 predicate 捕捉 KeyboardInterrupt 的 policy fixture 與新任務取消使用不同事件型別。

## 34 cases 對照

R01–R22 都來自 `test_all_service_retry_policies`。每列均驗證 N−1 不 stop、N stop；兩次 wait 查詢相同；正例符合 selector；KeyboardInterrupt 只在 predicate 符合；非 all/predicate 排除 ValueError。這組測 **policy 物件**，不代表完整 SDK HTTP 行為。

| ID | Python module / method / wrapper depth | attempts / wait / family | 待移植 Rust case |
|---|---|---|---|
| R01 | anythingllm / do_translate / 0 | 5 / E / all | `policy_anythingllm_translate` |
| R02 | azure / do_translate / 0 | 5 / E / all | `policy_azure_translate` |
| R03 | azureopenai / do_translate / 0 | 100 / E / openai | `policy_azureopenai_translate` |
| R04 | azureopenai / do_llm_translate / 0 | 100 / E / openai | `policy_azureopenai_llm` |
| R05 | bing / do_translate / 0 | 5 / E / predicate | `policy_bing_translate` |
| R06 | claudecode / do_translate / 0 | 3 / P / process | `policy_claudecode_translate` |
| R07 | clitranslator / do_translate / 0 | 3 / P / process | `policy_cli_translate` |
| R08 | deepl / do_translate / 0 | 5 / E / predicate | `policy_deepl_translate` |
| R09 | dify / do_translate / 0 | 5 / E / all | `policy_dify_translate` |
| R10 | google / do_translate / 0 | 5 / E / all | `policy_google_translate` |
| R11 | ollama / do_translate / 0 | 100 / E / ollama | `policy_ollama_translate` |
| R12 | ollama / do_llm_translate / 0 | 100 / E / ollama | `policy_ollama_llm` |
| R13 | openai / do_translate / 0 | 100 / E / openai | `policy_openai_translate` |
| R14 | openai / do_llm_translate / 0 | 100 / E / openai | `policy_openai_llm` |
| R15 | qwenmt / do_translate / 0 | 100 / E / openai | `policy_qwenmt_translate` |
| R16 | siliconflow / do_translate / 0 | 100 / E / openai | `policy_siliconflow_translate` |
| R17 | siliconflow / do_llm_translate / 0 | 100 / E / openai | `policy_siliconflow_llm` |
| R18 | siliconflowfree / do_llm_translate / 0（外） | 3 / H / httpx | `policy_siliconflowfree_outer` |
| R19 | siliconflowfree / do_llm_translate / 1（內） | 100 / F / free429 | `policy_siliconflowfree_inner` |
| R20 | tencentmechinetranslation / do_translate / 0 | 5 / E / predicate | `policy_tencent_translate` |
| R21 | xinference / do_translate / 0 | 100 / E / runtime | `policy_xinference_translate` |
| R22 | xinference / do_llm_translate / 0 | 100 / E / runtime | `policy_xinference_llm` |

下面 Python 名稱省略共同的 `tests/test_retry_characterization.py::`。同一 case 的多個斷言不可只挑一個移植。

| ID | Python case | 現行 observable oracle | 待移植 Rust case |
|---|---|---|---|
| R23 | `test_tencent_translator_works_with_compatible_sdk_without_api_call` | SourceText=source；TextTranslate 1 次；TargetText=translated；無真實 API | `tencent_request_response_contract` |
| R24 | `test_google_retries_429_5xx_and_timeout_without_retry_after[429]` | GET 5 次；sleep 1、2、4、8；忽略 Retry-After 120；RetryError 留 RequestException；呼叫未帶 timeout | `google_429_exhaustion` |
| R25 | 同上 `[503]` | GET 5 次、同 R24 等待與錯誤；503 不另換政策 | `google_503_exhaustion` |
| R26 | 同上 `[timeout]` | 模擬 requests.Timeout 5 次；同等待；最後 cause 是 RequestException | `google_timeout_exhaustion` |
| R27 | `test_google_400_returns_sentinel_and_limiter_waits_once_for_retries` | 400 回 IRREPARABLE TRANSLATION ERROR、零 sleep；另一情境429→200：2 GET、sleep1、BaseTranslator limiter只1次 | `google_sentinel_and_single_admission` |
| R28 | `test_bing_retries_429_and_interrupt_and_refetches_session` | 429：find_sid及POST各5次、sleep1/2/4/8、無timeout；find_sid先KeyboardInterrupt再成功：2次、sleep1 | `bing_retry_refetch_and_predicate` |
| R29 | `test_openai_retries_only_429_and_propagates_final_retryerror` | mock SDK 429/429/成功：3 calls、sleep1/2；持續429：100 calls、99新增sleep、cause RateLimitError；500及APITimeoutError各直接傳出、1 call | `openai_outer_classification_and_exhaustion` |
| R30 | `test_siliconflowfree_nested_429_and_http_error_policies` | 純429：100 POST、99內sleep（前5=4/4/4/8/16）、零外sleep、cause自訂RateLimitError；純503：3 POST、外sleep30/30、cause HTTPStatusError | `siliconflowfree_nested_policies` |
| R31 | `test_cli_timeout_retries_and_reaps_child` | 3 processes；各timeout後kill且再communicate（共2次）；sleep2/4；cause TimeoutExpired | `cli_timeout_retry_and_reap` |
| R32 | `test_claude_timeout_becomes_non_retryable_valueerror` | 1 process、communicate1次、kill；拋含timed out的ValueError；零sleep。原方法不再communicate | `claude_timeout_conversion` |
| R33 | `test_qps_limiter_spacing_and_no_concurrency_cap` | qps2前三次sleep0.5/0.5；set_max_qps(0)失敗；兩翻譯可同時在途、peak2 | `qps_spacing_without_inflight_cap` |
| R34 | `test_async_cancellation_releases_mocked_subprocess_resources` | Callback timeout1800；保留CancelledError；送True、close cancel sender/progress receiver、terminate child、join兩thread、queue送None並close | `cancel_releases_bridge_resources` |

R31/R32 的舊資源缺陷不應照搬：Rust仍須wait/reap整個受控程序樹，同時保留可觀察retry與錯誤分類。R34只有mock，不證明真實Windows的blocking I/O、child或grandchild都退出。

## 額外 5 個 API transport cases

`tests/test_api_transport.py` 用 loopback HTTP 與真實 OpenAI SDK。T02 刻意帶 `x-should-retry:false`，只測應用層；R29 mock SDK 不代表實際HTTP次數。

| ID | Python case | 必要 oracle |
|---|---|---|
| T01 | `test_sdk_parses_translation_and_usage` | 去空白為「測試譯文」；total_tokens8；1 POST；model=offline-fixture |
| T02 | `test_http_429_retries_then_parses_success` | 429→200共2 POST；SDK status retry由server header關閉 |
| T03 | `test_http_400_preserves_provider_error_without_retry` | 400保留local invalid model；1 POST；相當於BadRequestError |
| T04 | `test_empty_choices_is_not_reported_as_success` | 空choices失敗、1 POST；Python是IndexError，Rust可明確MalformedResponse，不能空字串成功 |
| T05 | `test_sdk_timeout_retries_are_distinct_from_application_policy` | ReadTimeout造成3 HTTP attempts；SDK2 retries；最終APITimeoutError；outer不再retry |

## SDK 內層差異

本次鎖版：OpenAI 3.36.1、HTTPX 0.28.1、DeepL 1.32.0、Azure Text Translation 1.0.1 / azure-core 1.41.0、Ollama 0.6.3、Tencent common 3.1.192 / tmt 3.1.121、xinference-client 4.0.0、Tenacity 9.2.1。升版須重做characterization。

### OpenAI / AzureOpenAI / QwenMT / SiliconFlow

四者未關閉SDK預設2 retries。一個outer attempt可有3個普通可重播HTTP attempts；持續429且SDK允許retry時，100 outer attempts可到300 HTTP。transport timeout/connection、HTTP408/409/429/>=500可由SDK重試，與outer僅RateLimitError不同。

本機`openai/_base_client.py`及`_constants.py`顯示：

- 3.36.1 的 Retry-After 上限是 **120秒，不是舊版60秒**。優先retry-after-ms，其次retry-after數字（可小數），最後HTTP date。finite且`0<delay<=120`採server delay。
- finite且>120會先拒絕SDK retry，優先於`x-should-retry`；否則true/false覆寫通常status判斷。outer RateLimitError仍可再次嘗試，所以拒絕SDK retry不是整份翻譯終止。
- fallback sleep=`min(0.5*2^k,8)*(1-0.25*u)`；k是已用SDK重試數、u來自random。外層E無jitter，內層向下0–25%。
- 缺header、無效文字、date、0、負值、NaN/Infinity、數字overflow需分開fixture；date用wall clock，其餘deadline用monotonic。
- 預設SDK timeout600秒、connect5秒；OpenAI模組可由openai_timeout覆寫。整個translation deadline不得冒充每次connect/read timeout。

### DeepL

`deepl/http_client.py`另有max_network_retries5（可6 transport attempts），再被本地predicate5包住；持續可重試network/429/5xx可達30 HTTP。初始backoff1秒，後續乘1.6、cap120、乘`1+0.23*uniform(-1,1)`。它先建deadline，再扣HTTP耗時才sleep，不是失敗後固定睡完整backoff；request timeout至少10秒。該retry路徑沒有把Retry-After交給計時器。語言mapping失敗也可能被本地predicate重試。

### Azure Text Translation

實際client用azure-core RetryPolicy，預設total budget10，connect/read/status各budget3；不是10+3，也不是通用「三次」。backoff factor0.8、cap120，第一個連續失敗為0，其後1.6/3.2；Retry-After有另一分支。POST/PATCH對500/503/504有特殊允許規則；status>=400且有Retry-After又走不同判斷。已確認source分支，但尚無本應用完整Azure HTTP fixture，移植前須補connect/read/status混合與POST429有無header。

### 其餘 provider

| provider | 已確認的差異 |
|---|---|
| AnythingLLM、Dify | requests POST timeout60；本地all5；response shape也可能回非字串/None，改嚴格型別需另列決策 |
| Google | GET無顯式timeout；截5000字元；400 sentinel算返回值；429/5xx/parse錯誤可由all5重試 |
| Bing | find_sid GET與POST皆無顯式timeout；每attempt重新sid；截1000字元；predicate含KeyboardInterrupt |
| Ollama | SDK client預設timeout=None，app沒覆寫；ResponseError不僅429；ConnectionError不同；兩方法會動態調num_predict |
| SiliconFlowFree | client timeout100但translation POST60；內429 F100、外HTTPError H3；純429耗盡的RetryError不符合外selector，所以不是300次；混合429→HTTPError可重進inner，不能合併counter |
| Tencent | app未給retryer，實際common預設NoopRetryer；HttpProfile timeout60；本地predicate5。credential lookup不是translation retry |
| Xinference | 每方法依分號逐模型fallback，catch Exception；全敗後一般Exception不符合RuntimeError decorator；不能改成100輪模型fallback |
| CLI Translator | translation與postprocess共用timeout；postprocess失敗可使整個translation重跑；timeout kill後communicate/reap |
| Claude Code | translation timeout120→kill→ValueError，所以不retry；nonzero exit仍可CalledProcessError；啟動檢查timeout5另算 |

可共用policy執行機制，但不能用一個通用retry預設替代不同layer、selector、counter reset、Retry-After、jitter與最終cause。

## Cache、QPS、併行及取消

BaseTranslator兩入口皆是：呼叫計數+1 → 可用時cache.get → hit直接返回 → limiter.wait一次 → provider方法及全部retry → 可用時cache.set → 返回。instance或本次ignore_cache任一為True都略過快取。get例外被忽略，set例外傳出；hit不耗QPS。這些尚未由34案例全部覆蓋。

QPS不是每次HTTP retry重取token；它只在logical translation入provider前一次。QPSRateLimiter用monotonic，第一次立即，之後next=`max(previous_next,now_after_sleep)+1/qps`；閒置不累積burst，set_max_qps不重設既有next。QPS僅間隔開始，不限制inflight；BabelDOC pool、IPC outstanding128、HTTP pool與QPS是不同限制。

現PoC Rust limiter先預約slot再async等待，另加concurrency semaphore；這不是完整舊版語意等價證明。未來需測取消不留時間洞、動態QPS、閒置與併行cache順序，再決定相容實作。

AsyncCallback的1800秒是每次queue.get等待下一事件的 **idle timeout**，不是PDF總時限。step_callback的sleep0.05用來讓出GIL，不是provider backoff；Rust不用照抄但需持續消費progress。舊finally送cancel、關pipe、join2秒、terminate/join1秒、必要時kill/join1秒，並join threads與close queue。EOF/不明訊息為IPCError，未捕捉非零exit為SubprocessCrashError。外層先yield error event再raise；新UI不得計成兩次失敗。

新cancel須能中止QPS、inner/outer sleep、HTTP、pending Future及writer背壓。worker不可另加retry/cache/controller；fatal IPC不可被BabelDOC fallback掩蓋成成功，必須毒化任務並解除所有pending。現PoC worker每次RPC等待上限1800秒，是額外transport護欄；完整100-attempt provider移植前須協調deadline，不能宣稱與舊版相同。

## 待完成測試與取代條件

1. R01–R22除了policy常數，還以fake operation驗證N attempts、N−1 sleeps、最後cause。
2. 用Tokio pause/advance或注入Clock/Sleeper控制時間，注入Rng驗證OpenAI的0/0.5/接近1、DeepL uniform序列；HTTP date另固定wall clock。不要真睡長backoff。
3. loopback mock HTTP記錄body、header、每次HTTP時間與數量、解析及最終錯誤；關閉時assert腳本已排空且沒有背景retry。
4. 補OpenAI雙層300次、408/409/500/timeout/connection、x-should-retry與Retry-After邊界；DeepL耗時/jitter/30次；Azure分budget；SiliconFlowFree混合層重置；Xinference fallback。
5. 補cache get/set錯誤與hit、動態QPS、閒置、取消、inflight限制、同key併行等語意。
6. 真實受控child測cancel/timeout/EOF/crash/stderr/孫程序清理；R34 mock不足。詳見IPC_PROTOCOL。
7. 每個provider依「Sol實作 → Astra審查 → tests → fix → commit」留證據後才取代Python對照。真實API未授權時用mock，不使用付費或個人憑證。
