# Rust provider 測試證據矩陣

更新日期：2026-10-29。Python 行為基準為 `d18165e`；案例定義見 [RETRY_COMPATIBILITY.md](RETRY_COMPATIBILITY.md)。本文件映射目前 Rust 測試中的實際斷言，不把政策常數、來源檢讀或測試函式存在視為完整等價證明。

`RETRY_COMPATIBILITY.md` 的「目前是 PoC」段落記錄該文件建立時的狀態；此矩陣補充後續 provider 實作的證據，不改寫原始 34+5 個 oracle。主代理在最後 Azure body-error 修正後執行 `cargo test --workspace --locked`：**102 passed，0 failed／ignored**（core14、legacy40、provider31、bridge framing3／整合13、CLI1）。結果位於 `.test-output/rust-migration/phase5-verified-tests.log`，本文件已核對各 suite 的結果行。主代理另確認 fmt check 與 clippy all-targets `-D warnings` 通過；本次文件整理沒有另跑 cargo。

完整回歸與 PDF 證據見 [TEST_REPORT.md](TEST_REPORT.md)。本輪四頁 PDF 產生 mono／dual，105 個 loopback API requests、36 batches；文字、字元座標、圖片及 8 頁 96 DPI RGB render 與 Python 基準一致，render 像素差 0。這證明受測 PDF 流程保持品質，不能用來補足下列 provider oracle 的缺口。

## 證據層級與記法

下表中的 `C::name` 等記法均指實際 Rust 測試函式，檔案位置如下。

| 前綴 | 檔案 | 實際執行邊界 |
|---|---|---|
| `C::` | `crates/core/tests/provider_compatibility.rs` | 透過公開 `TranslationService`。多數使用注入的 `HttpExecutor`、Clock、RandomSource：會真正執行 service、provider、retry、解析與 metrics，但不經 reqwest/socket。下文另標明真正 loopback HTTP 的案例。 |
| `L::` | `crates/core/tests/legacy_providers.rs` | 多數直接呼叫 `providers::legacy::dispatch(ProviderContext)`，隔離 service admission 與初始化。`Harness::run_service()` 才涵蓋公開 service；`claude_real_process_stream_json_contract_and_startup_is_cached` 的兩次 `prepare()` 也使用公開 service。 |
| `P::` | `crates/core/src/poc_tests.rs` 的 `tests` 模組 | 公開 service 與 reqwest，連到本機 loopback HTTP server；不使用真實 provider API。 |
| `U::` | `crates/core/src/lib.rs` 的 `service_unit_tests` 模組 | 檢查 service factory 建立的 connect-timeout 設定，不是 connect 階段故障的 socket 測試。 |
| `B::` | `crates/babeldoc-bridge/tests/lifecycle.rs` | 真實 bridge、受控假 worker、本機 model server 或原生子程序；不以真實 BabelDOC/PDF 輸出替代 provider oracle。 |

注入的 executor 一次呼叫只代表一個 HTTP attempt，不隱藏 retry 或 redirect；實際 loopback fixture 才驗證 wire 上的 method/header/body、redirect、cookie 或 reqwest 錯誤分類。長 backoff 使用注入時間或 Tokio pause/advance；少量真實 I/O、程序啟動與 timeout 測試仍有實際短等待。

下文沿用原契約的等待代號：`E=1/2/4/8/15…`、`P=2/4/8…`、`H=30/30…`、`F=4/4/4/8/16…`。attempt 數含首次，耗盡後不再 sleep。SDK 內層等待另列，不能與應用層合併計數。

## R01–R22：policy 宣告與 runtime 證據

**共同界限 G01：** Python `test_all_service_retry_policies` 逐列檢查 stop 物件的 N−1/N 邊界、重複 wait 查詢、正例、KeyboardInterrupt 與 ValueError 負例。Rust 沒有逐列複製這整組 policy-object oracle；以下是實際操作序列的 runtime 證據，並非「22 列 policy 斷言全部已移植」。來源中選擇器與政策分別在 `src/retry.rs`、`src/providers/openai.rs`、`src/providers/local.rs`、`src/providers/legacy/**`；來源相符不能補作缺少的測試斷言。Rust `Cancelled` 為獨立控制事件，不等同舊 predicate 的 Interrupt。

| ID | 來源政策 | 實際 Rust 測試與觀察 | 尚未由本列證明 |
|---|---|---|---|
| R01 | AnythingLLM translate：5 / E / all | `L::anythingllm_and_dify_retry_http_parse_and_nonstring_output`：HTTP 503、JSON/shape 失敗各 5 calls、4 次 E、最後 cause；`L::anythingllm_and_dify_real_http_preserve_distinct_payloads_and_whitespace` 驗真 HTTP payload。 | G01；非字串輸出改為明確錯誤，見下方行為差異。 |
| R02 | Azure translate：5 / E / all | `L::azure_post_network_errors_do_not_consume_sdk_retry_budgets`：POST connect/read 等錯誤只有 5 個應用 attempts；`L::azure_sdk_status_budget_and_retry_after_have_distinct_counters`：503 為每 outer 4 HTTP、共 20。 | G01；HTTP transport 階段 timeout 不等於整個 Python SDK 行為已證。 |
| R03 | AzureOpenAI translate：100 / E / 429 | `C::each_openai_family_method_keeps_the_100_attempt_outer_policy`：此 provider/method 100 sends、99 sleeps、一次 admission；server header 關閉 SDK status retry。 | G01；此分支斷言 attempt 數，不逐一重測所有 SDK header/錯誤。 |
| R04 | AzureOpenAI llm：100 / E / 429 | 同上，獨立執行 `llm_translate` 分支。 | 同 R03。 |
| R05 | Bing translate：5 / E / predicate | `L::bing_refetches_sid_every_attempt_and_predicate_retries_interrupt`：5 GET+5 POST、E；另一次 Interrupt 之後重新取 sid 並成功。 | G01；Interrupt 是注入的 typed error，真 HTTP 不會產生 Python KeyboardInterrupt。 |
| R06 | Claude Code translate：3 / P / process | `L::claude_real_process_nonzero_retries_but_missing_translation_does_not`：3 次 nonzero exit、P；無翻譯只 1 次且 Value。timeout 轉型見 R32。 | G01；使用編譯的受控 CLI fixture，沒有執行使用者 Claude 或外部 API。 |
| R07 | CLI Translator translate：3 / P / process | `L::cli_real_process_timeout_retries_three_times_and_reaps`：3 次 timeout、P、ProcessTimeout cause；`L::cli_real_process_stdin_postprocess_and_failure_restarts_translation`：postprocess 失敗重跑整輪。 | G01；Rust wait/reap 取代 Python `communicate` 呼叫次數，見 R31。 |
| R08 | DeepL translate：5 / E / predicate | `L::deepl_nested_exhaustion_has_thirty_http_attempts_and_exact_sleeps`：5 outer × 6 SDK attempts、各層等待與最後 cause；`L::deepl_invalid_mapping_and_malformed_count_are_outer_errors_only`：mapping/shape 不進 SDK network retry。 | G01；未逐列重建 predicate 對 Interrupt 的政策正例。 |
| R09 | Dify translate：5 / E / all | `L::anythingllm_and_dify_retry_http_parse_and_nonstring_output` 對 Dify 亦獨立跑 5 次錯誤序列、E、cause；另有真 HTTP payload/空白輸出 fixture。 | G01；非字串輸出嚴格化的差異仍保留。 |
| R10 | Google translate：5 / E / all | `L::google_status_timeout_and_parse_failures_have_exact_outer_attempts`：429/503/ReadTimeout/parse 各 5 次；Interrupt 直接傳出。R24–R27 補不同 observable。 | G01；其中 timeout 是注入錯誤，不是 socket timeout。 |
| R11 | Ollama translate：100 / E / ResponseError | `C::ollama_both_methods_retry_all_response_status_errors_100_times`：此 method 的 HTTP 400 亦 100 次、99 sleeps、OllamaResponse cause；`C::ollama_nonstream_shape_dynamic_predict_and_response_error_selector`：Connect 與 200 malformed 各 1 次。 | G01；非 HTTP status 的完整 SDK/Pydantic 驗證域沒有逐欄位複製。 |
| R12 | Ollama llm：100 / E / ResponseError | 同上，獨立執行 `llm_translate` 的 100 次耗盡分支。 | 同 R11。 |
| R13 | OpenAI translate：100 / E / 429 | `C::each_openai_family_method_keeps_the_100_attempt_outer_policy`；`C::openai_300_http_attempts_wrap_only_the_application_layer` 另驗 100 outer × 3 SDK HTTP 與 outer last cause。 | G01；R29 列出原 mock-SDK oracle 尚未一比一重建的部分。 |
| R14 | OpenAI llm：100 / E / 429 | `C::each_openai_family_method_keeps_the_100_attempt_outer_policy` 的 llm 分支；`P::llm_translation_preserves_raw_prompt_and_requests_json_when_enabled` 驗真正 wire raw prompt。 | G01；300 sends 的 fixture 只跑 OpenAI translate，不能當每 method 各自 300 的證據。 |
| R15 | QwenMT translate：100 / E / 429 | 同一 family-method fixture 的 QwenMT translate 分支；`C::azure_qwen_and_siliconflow_preserve_their_wire_options` 驗 raw text、translation_options、語言與 model 限制。 | G01；QwenMT 不支援 llm method，未以其他 method 代替。 |
| R16 | SiliconFlow translate：100 / E / 429 | family-method fixture 的 SiliconFlow translate 分支；wire-options fixture 驗 provider 特有 payload。 | G01；wire-options 在這裡指 executor 收到的 HttpRequest，不是 socket capture。 |
| R17 | SiliconFlow llm：100 / E / 429 | family-method fixture 的 SiliconFlow llm 分支。 | G01；未對此分支重跑全部 OpenAI SDK 邊界。 |
| R18 | SiliconFlowFree 外：3 / H / HTTPError | `C::free_nested_layers_do_not_multiply_rate_limit_exhaustion`：503 共 3 sends、H、外層耗盡；429→503→429→成功的等待為 4/30/4。 | G01；混合序列證明 counter reset，未窮舉所有 HTTPError subtype。 |
| R19 | SiliconFlowFree 內：100 / F / 自訂 429 | 同上：純 429 共 100 sends、99 sleeps、前五為 4/4/4/8/16、內層 FreeRateLimit 耗盡，不被外層乘成 300。 | G01；未逐一斷言全部 99 個 F 值與 cap 邊界。 |
| R20 | Tencent translate：5 / E / predicate | `L::tencent_outer_predicate_retries_http_provider_error_and_interrupt`：HTTP 500、SDK 型 provider error、Interrupt 各 5 次、E、最後 cause。 | G01；真網路與真 Tencent SDK 服務未使用。 |
| R21 | Xinference translate：宣告 100 / E / RuntimeError | `C::xinference_both_public_methods_make_one_fallback_round_without_repacking_raw_input`：此 method 只跑一輪逐模型 fallback，全敗為 Provider error、無 sleep；`C::xinference_runs_one_model_fallback_round_and_rejects_missing_auth` 驗初始化 auth shape。 | 原 catch-all 最後拋一般 Exception，正向 RuntimeError retry 在此路徑不可達；沒有捏造 100 輪來湊 policy 正例。G01 仍適用。 |
| R22 | Xinference llm：宣告 100 / E / RuntimeError | 同一 public-method fixture 獨立驗 llm：成功 fallback 3 HTTP、全敗 2 HTTP、無 sleep；message 等於原始 input，未重包翻譯 prompt。 | 同 R21；這是實際一次 fallback 的證據，不是不可達 decorator 正例的執行證據。 |

## R23–R34：行為 oracle

| ID | 原 oracle 與 Rust 函式 | 現有斷言與證據層級 | 剩餘差異或缺口 |
|---|---|---|---|
| R23 | `L::tencent_real_http_signing_and_response_without_sdk_retry`；`providers::legacy::tencent::tests::signing_payload_matches_python_json_dumps` | 真 loopback 一次 POST；SourceText/TargetText、timeout、SDK_PYTHON header、簽名格式；unit fixture 用固定 synthetic credential 驗 Python json.dumps 對應 body 與確切 HMAC 簽章。 | 取代原 mock compatible-SDK 的 Rust wire 合約，不代表真 Tencent API 驗證。 |
| R24 | `L::google_status_timeout_and_parse_failures_have_exact_outer_attempts` 的 429 分支 | executor 觀察 5 GET、E 1/2/4/8、RetryExhausted/HttpStatus(429)、每 HttpRequest timeout=None；Retry-After 120 不影響等待。 | requests.RequestException 對映為 Rust typed cause；此耗盡序列不是 5 次 socket capture。 |
| R25 | 同一函式的 503 分支 | 同樣 5 次、E、HttpStatus(503)，與 429 分開執行。 | 同 R24。 |
| R26 | 同一函式的 ReadTimeout 分支 | 5 次注入 ReadTimeout、E、最後 ReadTimeout cause、timeout=None。 | 無 timeout 的 Google HTTP 不會自發產生同一 read-timeout fixture；不宣稱真 wire 製造 requests.Timeout。 |
| R27 | `C::google_public_service_retries_once_per_logical_admission_and_returns_400_sentinel`；`L::google_real_http_query_html_entities_controls_and_sentinel`；`L::google_real_http_429_recovery_ignores_retry_after` | 公開 service：400 sentinel 時 calls/admissions/http_attempts=1/1/1、零 sleep；429→200 為 1/1/2、sleep1。另用真 HTTP 驗 sentinel 與 429 recovery。 | 公開 service 的 metrics assertion 使用注入 executor；真 wire 與 metrics 是分開的 fixture。 |
| R28 | `L::bing_refetches_sid_every_attempt_and_predicate_retries_interrupt` | 429：sid GET 與 POST 各 5、E、所有 request 無 timeout；Interrupt recovery 兩次 sid 嘗試、一次成功 POST、sleep1。 | Interrupt 為注入 typed error；`L::bing_real_http_redirect_cookie_last_iid_and_unicode_limit` 另驗 redirect 後 URL、cookie、最後 IID 與 1000 Unicode 字元截斷。 |
| R29 | `C::openai_300_http_attempts_wrap_only_the_application_layer`；`C::each_openai_family_method_keeps_the_100_attempt_outer_policy`；`C::sdk_status_retries_and_transport_retries_are_distinct_from_outer_429` | SDK 未禁 retry 的 429 為 300 HTTP、100 outer、最終 429 cause；禁 SDK status retry 的每 family/method 為 100 sends/99 sleeps。408/409/500/503 的兩次錯誤後成功只走 SDK；Connect/ConnectTimeout/ReadTimeout/ReadTransport 三次後直接 typed error。 | 原 mock SDK 的「429/429/成功，outer sleep1/2」未獨立逐樣重建；終端 500 耗盡後不再 outer retry 也未另設專用 fixture。不要把 SDK 3 HTTP 當成原 mock SDK 的 1 call。 |
| R30 | `C::free_nested_layers_do_not_multiply_rate_limit_exhaustion` | 純 429 為 100 sends、99 內 sleep、內層 cause；純 503 為 3 sends、30/30、外層耗盡；混合序列另驗重新進入 inner。 | 503 分支直接斷言 outer layer/attempts，未單獨再解構其 last cause；不能聲稱原 case 每個斷言均逐項重建。 |
| R31 | `L::cli_real_process_timeout_retries_three_times_and_reaps` | 真原生 fixture 啟動 3 processes，各次 1 秒 timeout，等待 2/4，最後 ProcessTimeout；Windows 在子程序仍活著時持有固定 HANDLE，回傳後立即斷言全部已退出。 | 不重建 Python「兩次 communicate」方法呼叫；Rust 用 kill、程序樹終止及 wait/reap 實現資源 oracle。 |
| R32 | `L::claude_real_process_timeout_converts_to_value_without_retry_and_reaps` | 原生 fixture、Tokio 時間推進超過 120 秒：只有 1 process、Value 且含 timed out、零 retry sleep；固定 HANDLE 證明回傳前已退出。 | Rust 加強 reap，不保留 Python timeout 後不再 communicate 的舊資源缺陷。 |
| R33 | `C::qps_admission_does_not_impose_an_inflight_cap`；`C::qps_dynamic_updates_idle_spacing_and_cancelled_waiters_preserve_the_next_slot`；`P::rate_limit_waits_in_rust_and_can_be_cancelled` | 公開 service 兩次翻譯可同時在途、peak2，qps2 間隔 0.5；limiter 拒絕 0、動態改 4 不重設既有 next、閒置不累積 burst、取消 waiter 不留時間洞。R27 另證 retry 只 admission 一次。 | 原前三次皆 qps2 的 0.5/0.5 序列未以完全相同三次呼叫重演；現有測試第二間隔同為 0.5，但期間改成 qps4，斷言的是舊 next 被保留。 |
| R34 | `B::cancellation_force_kills_worker_that_ignores_stdin`；`B::cancellation_force_kills_the_entire_contained_tree`；`B::cancelling_a_cli_request_waits_for_native_process_tree_cleanup`；`L::windows_job_cancellation_reaps_child_and_descendant_with_inherited_pipes` | 真受控 worker/CLI/native tree；取消後等待 root 與 descendant 退出。bridge shutdown_timeout=0 的 process-provider fixture 仍需 cooperative await，不能直接 abort 掉清理 future。 | 這是 Rust 資源合約替代證據，不是原 Callback 的 queue True/None、close、兩條 thread join 呼叫逐項等價。1800 秒 idle 是 bridge 設定來源值，未有完整 1800 秒事件序列 oracle。 |

## T01–T05：OpenAI transport

這五列區分「完整 service 經注入 transport」與「真正 loopback」。Python 對照用了真 OpenAI SDK；Rust 沒有載入 Python SDK，因此錯誤類別使用 `ErrorKind`，不宣稱 Rust 物件是 Python SDK exception。

| ID | Rust 函式 | 目前直接斷言 | 未補齊的原 oracle |
|---|---|---|---|
| T01 | `C::openai_success_preserves_prompt_and_accounts_usage_before_malformed_choices`；`P::translate_posts_legacy_prompt_and_removes_only_the_initial_thought` | C 的第一次呼叫輸出「測試譯文」，再一次 malformed-choices response 的 usage 仍記帳，累計 total12=8+4、calls/admissions/http2；P 真 wire 驗一次 POST、model 與完整舊 prompt。 | C 沒在第一次呼叫後獨立斷言 total_tokens=8，也沒直接斷言 model=offline-fixture；P 驗的是 synthetic-model。不得把這些聯合證據標為原 T01 每個值完全重建。 |
| T02 | `C::x_should_retry_false_and_excessive_server_delay_still_allow_outer_429` | x-should-retry:false 的 429→成功為 2 HTTP、sleep1；另一分支驗 >120 秒先拒 SDK retry，outer 仍重試。 | 是 executor 收到的 2 HTTP requests，不是實際 socket 的兩個 POST；沒有真 wire header-control 序列。 |
| T03 | `C::p01_p02_safe_provider_errors_scrub_boundaries_headers_and_overlapping_input`；`C::source_and_credentials_use_original_overlapping_ranges_in_both_directions`；`P::errors_do_not_expose_provider_body_or_credentials` | 結構化 400 保留 local invalid model、只有 1 request；source、key、自訂 header、bare bearer token、URL 被遮罩；邊界與重疊另有具體斷言。P 真 wire 的 401 非結構化 body 只回安全 status。 | 400 保留訊息的案例使用 executor；沒有在真 socket 上逐樣重建原 400 JSON fixture。 |
| T04 | `C::openai_success_preserves_prompt_and_accounts_usage_before_malformed_choices`；`P::malformed_empty_and_null_responses_fail_without_echoing_content` | C 空 choices 回 MalformedResponse，該呼叫只有 1 HTTP；P 真 wire 各跑 invalid JSON、空 choices、null/missing content，均失敗而非空字串成功。 | Rust 明確錯誤型別取代 Python IndexError，屬刻意可觀察差異。 |
| T05 | `C::sdk_status_retries_and_transport_retries_are_distinct_from_outer_429`；`P::request_timeout_is_bounded_and_safe` | 注入 ReadTimeout 的 3 attempts、SDK 2 sleeps、最後直接 ReadTimeout，沒有 outer 100。P 真 stalled response 可安全 timeout。 | P 未觀察三次實際 HTTP attempt；不能將兩個 fixture 合稱「真 socket ReadTimeout 3 次」的完整 T05。 |

## SDK 與共享機制的補充驗證

| 主題 | 函式與具體證據 | 驗證邊界 |
|---|---|---|
| OpenAI retry header / jitter | `C::retry_after_numeric_overflow_dates_and_jitter_follow_pinned_sdk`：ms 優先、小數、120/120.0001、numeric overflow、0/負數/NaN/Infinity/invalid、固定 wall-clock date；`C::sdk_jitter_extremes_apply_to_real_retry_sequences`：u=0、0.5、接近 1 的實際重試序列。 | 使用可控 Clock/RandomSource 與 executor；不是外部 SDK 互操作測試。 |
| DeepL SDK | `L::deepl_jitter_deadline_consumes_http_time_and_sdk_resets_per_outer`：扣 HTTP elapsed 再 sleep；`L::deepl_jitter_uses_injected_extremes_without_outer_jitter`：jitter 三個極值；30-attempt fixture 驗每 outer 重設與 timeout 至少 10 秒。 | HTTP elapsed 是注入值；真 truncated-body 與 redirect-loop fixture 另外驗分類，分別為 ReadTransport 與 Redirect，不誤進 6 次 inner retry。 |
| Azure SDK budgets | `L::azure_sdk_status_budget_and_retry_after_have_distinct_counters`、`L::azure_post_network_errors_do_not_consume_sdk_retry_budgets`、`L::azure_retry_after_date_zero_invalid_and_post429_ms_alias`。新增 redirect 後 GET 的 `L::azure_303_get_connect_read_and_status_budgets_are_independent` 用 3 connect+3 read+3 status 後成功；`L::azure_303_get_body_errors_share_status_budget_with_http_errors` 驗 body/status 共用 budget。 | connect/read 故障以 executor 注入；`L::azure_303_get_truncated_bodies_use_status_budget_without_backoff_history` 才是真 reqwest body failure。仍未把完整 azure-core pipeline、所有狀態碼與 operation deadline 全部窮舉。 |
| Ollama nonstream | `C::ollama_nonstream_shape_dynamic_predict_and_response_error_selector`：動態 num_predict 單調 max、timeout=None、usage、Connect 不 retry；200 只有 error 或缺 role 為 malformed 且一次；合法 message 上的額外 error 欄不強制失敗。 | 驗 app 使用的 nonstream 形狀；不涵蓋 stream SDK。 |
| 一次初始化與取消等待 | `C::p04_cancelled_preflight_waiter_does_not_drop_or_repeat_initializer`：A pending 時 B cancel 即回，A 保持、初始化只 1 HTTP；Claude native fixture 兩次 prepare 只有一次 --version。 | `TranslationService::new()` 不做 async network/process；呼叫者須 `prepare()`，或第一次 translate 自動 prepare。bridge 已在建立 worker config 前呼叫。 |
| Free 初始化推薦值 | `C::p09_free_bootstrap_probes_endpoints_concurrently_and_reuses_recommendations`：一 endpoint pending 時另一 endpoint 的 3 probes 已前進，選定 endpoint 與 qps/pool 沿用；missing status、invalid JSON、explicit bad+message 分支另有 fixtures。`C::free_failed_config_keeps_original_limiter_but_returns_default_recommendations`：/config 失敗仍推薦10/100，實際原 qps2 admission 間隔保持0.5秒。 | 未授權真 Free endpoint healthcheck；全為本機腳本回覆。 |
| 取消與重試獨立 | `C::cancellation_interrupts_sdk_backoff_without_a_background_retry`、`C::cancellation_interrupts_active_http_without_becoming_retryable`；`L::every_legacy_http_provider_cancels_inflight_without_retry`、`L::cancel_during_sdk_and_application_backoff_leaves_no_background_retry`；`L::google_real_http_without_timeout_is_cancellable`。 | 分別驗 typed cancellation、沒有下一個 request；不把取消注入為會被舊 predicate retry 的 Interrupt。 |
| 安全錯誤與 headers | P01/P02 及 overlapping-ranges fixture 在原訊息上合併敏感區間後遮罩，含 4096 字元邊界前六字洩漏 oracle；`C::p08_custom_authorization_replaces_default_case_insensitively_on_the_wire` 真 wire 驗只一個覆寫後 Authorization。 | 安全 provider message 並非整段抹除；任意未知內容不可能由遮罩推得全部皆安全。已知 API key、source、支援的 custom-header 值與 token 按合約遮罩。 |
| reqwest body error 分類 | `C::p06_truncated_and_malformed_chunked_bodies_are_read_transport_errors` 真 loopback 驗短 Content-Length 與 malformed chunk 都是 ReadTransport。 | 防止 reqwest Decode 被誤當 connect/一般 Transport 進錯 SDK 重試。 |
| 程序樹與 bridge | `L::cli_and_claude_real_process_cancel_reaps_before_return` 與 Windows descendant fixture；B 的 crash/EOF/framing/bounded-progress/large-pool fixtures。`B::main_and_term_requests_use_separate_services_and_usage`、`B::shared_service_usage_is_reported_once` 驗 service routing 與 usage 不重算。 | Windows HANDLE oracle 只在 Windows 編譯執行；受控 native fixture 不等於對所有外部 CLI 的實際整合。 |

## Redirect 接線

普通 transport 不自帶 SDK retry，並關閉 reqwest 的隱藏 retry；redirect 由明確上限處理。`HttpTransport::with_redirect_limit(0)` 正常返回 3xx，不 follow。注入 executor 不替測試自動 follow。

| Provider / SDK 類型 | service factory 設定 | 實際 runtime 證據與缺口 |
|---|---|---|
| OpenAI、AzureOpenAI、QwenMT、SiliconFlow；Ollama 的 HTTPX | transport cap20 | `C::p03_cross_origin_307_and_308_strip_all_custom_credentials_on_the_wire` 真雙 origin 驗 follow 與 credential stripping。尚無 20/21 邊界，亦未逐 canonical provider/alias 對 factory 上限作獨立 runtime table；cap20 接線仍是來源核對。 |
| Google、Bing、AnythingLLM、Dify、DeepL、Tencent；Xinference 的 requests | transport cap30 | `L::legacy_service_requests_redirect_limit_allows_thirty_hops` 用公開 Google service 真走 30 hops 後成功（31 HTTP）；`L::deepl_actual_redirect_loop_is_not_an_inner_connection_retry` 走 31 responses ×5 outer=155 HTTP。尚無 Xinference 或每個 requests provider 各自的 cap boundary oracle。 |
| SiliconFlowFree 的 HTTPX 預設不 follow | transport cap0 | `C::free_httpx_no_follow_policy_returns_3xx_to_outer_retry_on_real_http`：來源3次307、外層30/30、目的 origin 零次、typed outer cause307。 |
| Azure Text SDK 自訂 RedirectPolicy | transport cap0；provider coordinator cap30，包在每個目標的 SDK retry 外面 | `L::azure_service_post_301_302_does_not_follow_redirect`；`L::azure_redirect_roots_relative_location_and_resets_each_targets_sdk_budget`；`L::azure_303_preserves_body_while_get_uses_its_own_status_selector`；`L::azure_redirect_loop_has_thirty_hops_per_outer_attempt` 真 wire 驗 POST301/302 不 follow、relative URL、target 換新 SDK budget、303→GET、保留原 body、31×5=155 HTTP。 |

跨 origin 僅保留 transport 明確允許的標準非憑證 headers，移除使用者 custom headers、Authorization、API key、Tencent STS token 等。這是安全收緊，不宣稱每個舊 SDK 的 redirect header 黑名單完全一致。普通 transport 的 P03 fixture 驗任意 X-Auth/Authorization/key；Azure 手動 coordinator 另以 `L::azure_cross_origin_redirect_does_not_forward_configured_credentials` 驗真307/308不轉送 subscription key/region/Authorization。

## 尚未證明或刻意保留的界限

1. **Timeout 階段不能合稱 SDK 等價。** OpenAI 未顯式設定時 connect5/read600，顯式 float 時 connect/read 使用同值：`U::p05_service_factory_preserves_sdk_default_connect_and_explicit_timeout` 檢查 factory，`C::p05_openai_implicit_and_explicit_read_timeouts_reach_actual_requests` 檢查 provider 發出的 HttpRequest（此測試名稱的 actual requests 指 executor 所見，並非 socket 故障）。目前 reqwest 使用 connect/read idle timeout，沒有完整重建 HTTPX 獨立 write/pool timer。Google/Bing、Ollama、Xinference 原本無 timeout 的路徑仍無隱藏 translation total deadline。Azure operation budget、requests 的完整 timeout/redirect/cookie 差異仍不能僅靠欄位相同宣稱等價。
2. **Persistent cache 尚未移植。** 保留 ignore_cache 的設定/請求契約，但尚無 Rust cache.get/set、hit 不耗 QPS、get 錯誤忽略、set 錯誤傳出、同 key 併行的完整流程與測試；`cache_hits` 為預留計數，不能宣稱持久快取相容。當前 calls/admissions/http/usage 證據限於無 cache 的邏輯翻譯。
3. **嚴格輸出型別有可觀察差異。** AnythingLLM/Dify 的 None/非字串輸出在 Rust 成為 MalformedResponse 並遵循原 all-exceptions outer policy，而非向上傳回非字串；空字串仍依 provider 原語意處理。OpenAI 空 choices 明確失敗。這些需保留為移植決策，不能寫成所有失敗物件完全一樣。
4. **初始化與資源安全更明確。** `prepare()` 將 async preflight 與同步 new 分開、每 service 緩存結果；等待初始化者可取消，進行中的 process initializer 不被外層 select drop。Windows 程序 timeout/cancel 的 fixtures 驗證 wait/reap 整樹；非 Windows `ProcessScope` 目前未提供同一 descendant-tree 等待保證，不能外推 Windows 結果。這替代舊資源缺陷，並非逐個 Python subprocess/queue API 呼叫相同。
5. **額外安全護欄是新行為。** body cap4MiB、bounded safe error message、跨 origin custom-header stripping、IPC framing/identity 與 fatal-channel 控制均不可冒稱舊 SDK 原本已有。舊 API key 不交給 Python worker；HTTP wire fixtures 只用 synthetic credential。
6. **不以測試數量替代驗收。** G01、R29/R30/R33 的未重建斷言、T01/T02/T03/T05 的 wire/數值缺口、HTTPX20/Xinference30 boundary 與完整 write/pool timeout 都仍列為缺口。沒有為不可達 RuntimeError 正例或純政策表新增照實作抄寫的測試。
7. **沒有真付費 provider API 驗證。** 鎖版來源為 OpenAI3.36.1/HTTPX0.28.1、DeepL1.32.0、Azure Text1.0.1/azure-core1.41.0、Ollama0.6.3、Tencent common3.1.192/tmt3.1.121、xinference-client4.0.0。來源核對與本機 fixture 不能保證遠端部署、認證、配額或後續 SDK 升版；完整 PDF PoC 證據另存，不用來替代本矩陣缺少的 oracle。
