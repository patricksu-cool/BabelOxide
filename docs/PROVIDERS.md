# 翻譯服務相容性與重試證據

更新：2026-10-31。原版 16 個 translator 都有 Rust 路徑，另保留 8 個 OpenAI 相容服務設定選項。以下契約涵蓋應用程式的非 streaming 翻譯呼叫、請求／解析／錯誤與各層重試；沒有呼叫真實付費 API，也不宣稱完整上游 SDK 等價。重試契約見 [行為基準](RETRY_COMPATIBILITY.md)，獨立 oracle 位於 `crates/core/tests/fixtures/legacy_retry_contract_v1.json`，不依賴已刪除的 Python App。

| Original translator name | Rust route | Representative local evidence / qualification |
|---|---|---|
| `openai` | `openai` | Nested SDK/application retries, request/usage and error fixtures; SDK write/pool timeouts remain different. |
| `azure-openai` | `azure-openai` | OpenAI family policy and options fixtures; no real Azure deployment. |
| `qwen-mt` | `qwen-mt` | Raw text, language/options and application retry fixtures; translate only. |
| `siliconflow` | `siliconflow` | OpenAI family policy and provider-specific options fixtures. |
| `siliconflowfree` | `siliconflowfree` | Separate nested 429 and HTTP failure retry policies and preflight fixtures. |
| `ollama` | `ollama` | Local nonstream shape, dynamic prediction and response-error retry fixtures. |
| `xinference` | `xinference` | Authentication shape and one-round model fallback fixtures. |
| `google` | `google` | Loopback request, 400 sentinel, retry and cancellation fixtures. |
| `bing` | `bing` | Session/token refresh, redirect cookie and predicate retry fixtures. |
| `deepl` | `deepl` | Nested 30-attempt, backoff, language and wire fixtures; prepared-request cookie boundary below. |
| `azure` | `azure` | Independent SDK retry budgets, redirect and wire fixtures; deadline boundary below. |
| `anythingllm` | `anythingllm` | Payload/response, retry and request-scoped redirect cookie fixtures; non-string output is a typed error. |
| `dify` | `dify` | Payload/response, retry and request-scoped redirect cookie fixtures; non-string output is a typed error. |
| `tencent` | `tencent` | Synthetic TC3 signature and loopback request plus five-attempt predicate fixture; credential limitations below. |
| `clitranslator` | `clitranslator` | Controlled local process stdin, postprocess, exit, timeout and cancellation fixtures. |
| `claudecode` | `claudecode` | Controlled process startup, output, timeout conversion and cancellation fixtures; no real Claude invocation. |

The settings factory also maps `OpenAICompatible`, `AliyunDashScope`, `DeepSeek`, `ModelScope`, `Zhipu`, `Gemini`, `Grok`, and `Groq` to the `openai` route with their respective endpoints/options. These are settings choices, not eight additional original translator implementations. See the mapping assertions in `crates/config/tests/settings.rs`.

## Current changes and boundaries

* Tencent still requires configured ID and key, as did the full Python application's settings validation. A complete injected `TENCENTCLOUD_SECRET_ID` / `TENCENTCLOUD_SECRET_KEY` pair now takes precedence, is checked for surrounding whitespace, remains out of serialized settings/debug output, and does not consume `TENCENTCLOUD_TOKEN`, matching the pinned environment provider. A partial pair falls back to configured credentials. The sidecar survives GUI-style settings edits. `tencent_environment_credentials_override_config_without_persistence` covers these cases and a complete TKE environment produces a visible unsupported-source error instead of silently pretending to assume a role.
* Tencent profile files, CVM metadata credentials and refresh, TKE OIDC/STS and refresh are **not implemented**. In particular, a pre-existing profile or CVM role may have overridden configured credentials in Python; Rust currently uses the configured pair when no complete environment pair is present. It does not inspect profile files or metadata. Users of those sources must supply static credentials; full TKE indicators result in an explicit error. No claim of complete SDK chain parity is made.
* DeepL's prepared initial request excludes the session jar, while redirects can use its accumulated cookies. `deepl_prepared_requests_omit_session_cookies_until_redirect` verifies this on a local socket across multiple operations. Other providers retain their existing cookie scopes.
* Azure's seven-day operation budget now constrains connection timeout independently of its 300-second read timeout; expiration distinguishes preceding connection errors from response errors. `azure_deadline_limits_connect_only_and_tracks_last_error_kind` uses injected I/O and a controlled clock; it does not exhaustively characterize Azure's complete pipeline or wall-clock behavior.
* No retry algorithm was changed. Existing per-provider application and SDK-layer policies retain the exact evidence and remaining gaps listed in the matrix. In particular, some original wire/clock oracle cases are incomplete, and HTTPX write/pool timers, proxy/CA/environment behavior, and every requests redirect/cookie corner are not fully reproduced.
* Local fixtures use synthetic credentials, loopback servers, controlled clocks and controlled child programs. No paid or real translation service, metadata endpoint, STS endpoint, real credentials or real Claude executable was used. Remote compatibility cannot be concluded from these fixtures.

## Frozen retry oracle

`crates/core/tests/fixtures/legacy_retry_contract_v1.json` preserves the 22 original Tenacity policy tuples (extracted with Python AST rather than importing the old application), the R23–R34 observations, T01–T05 transport observations and pinned SDK constants. It is a versioned independent expectation, not an assertion that every Python observable is equivalent. `provider_compatibility` and `legacy_providers` consume selected values in real service/executor tests, so removing the Python translator sources does not remove those expectations.

| Oracle rows | Present Rust evidence and remaining qualification |
|---|---|
| R01–R22 | All 22 original policy rows are preserved. OpenAI-family R03/R04/R13–R17 and Ollama R11/R12 runtime tests now consume each matching row's attempts and initial waits, and check final causes. Other provider runtime fixtures in the matrix exercise applicable operations and last causes; they do **not** reproduce every Python decorator introspection assertion. Xinference's positive RuntimeError path is unreachable in the original provider flow. |
| R23–R28 | Tencent signature/loopback; Google 429, 503 and timeout executor exhaustion now read their attempt/sleep expectations from the frozen oracle; Google sentinel/admission and Bing session refetch/interrupt retain separate fixtures. |
| R29 | `frozen_r29_outer_recovery_and_direct_server_error` checks 429/429/success with sleeps 1/2, and a directly propagated 500, with SDK status retries disabled. Separate tests cover 100 outer attempts and timeout SDK retries; these are synthetic executor observations. |
| R30 | The nested Free fixture reads the frozen attempt and wait values and now asserts the terminal 503 cause, alongside inner 429 exhaustion and mixed-layer reset. |
| R31–R32 | Controlled process fixtures check timeout/exit attempts and resource cleanup. Rust intentionally reaps processes; it does not reproduce Python's exact `communicate` counts. |
| R33 | `frozen_r33_three_admissions_preserve_half_second_spacing` reproduces both qps=2 intervals and invalid zero. Existing concurrent fixture independently checks peak in-flight 2. |
| R34 | Controlled worker/process cancellation fixtures cover cleanup; the original Python queue/pipe/thread calls were mocked and are not claimed as API-equivalent. |
| T01–T05 | T01 now separately checks frozen model and 8 tokens at first success. T02/T03/T05 are covered with injected HTTP/timeout observations and separate wire tests as detailed in the matrix, not identical end-to-end wire oracles; T04 deliberately returns a typed malformed-response error. |

The HTTP client cache is capped at 32 entries, including clients with distinct Azure connection budgets. Eviction does not invalidate in-flight cloned clients or the shared cookie jar. `changing_connection_budgets_do_not_grow_client_cache_without_bound` checks 100 distinct budgets without network access.

Reproduce the preserved Rust checks with `cargo test -p pdf2zh-core --test provider_compatibility`, `cargo test -p pdf2zh-core --test legacy_providers`, and `cargo test -p pdf2zh-core --lib client_cache_tests`. Before removal of the historical Python application, the isolated original baseline can be run with `.venv\Scripts\python.exe .test-output\rust-migration\run_isolated.py pytest -q tests/test_retry_characterization.py tests/test_api_transport.py`; after removal the frozen JSON and Rust tests remain self-contained. Run loopback/process tests in an environment that permits local sockets and synthetic child processes.

The original isolated Python command passed **39 tests** on 2026-10-31. The Rust provider suites passed **33** and **44** tests respectively, and the cache-bound unit test passed. All used synthetic or local fixtures.
