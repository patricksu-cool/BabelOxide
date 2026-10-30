# Remaining provider gaps after the native GUI checkpoint

查核：2026-10-30。這是限定範圍的唯讀 source audit，不是新一輪 runtime 驗收。本輪只新增本文件；未修改 source、執行 Cargo／Python oracle、讀取真實 HOME／credential、呼叫 metadata／STS／翻譯 API。依主代理回報，GUI checkpoint 的205項 Rust、285項 Python、clippy 與 Astra review 已通過；它們不自動補足以下缺口。Graph coverage 本輪不可用，使用鎖版 repo／`.venv` SDK／Cargo registry source fallback。

`CONFIG_MIGRATION.md` 與 `PROVIDER_TEST_MATRIX.md` 是不同階段的快照。矩陣「persistent cache 尚未移植」及「set 錯誤傳出」已過時：現階段 Rust cache 已實作，Python `TranslationCache.set` 本身會 catch DB exception，Rust ordinary get/set error 也 best effort；取消另行傳播。正確 cache 證據見 `CACHE_COMPATIBILITY.md`，不能把舊文字當成剩餘功能。

## 1. Tencent DefaultCredentialProvider

### 鎖版實際順序與錯誤邊界

| 階段 | 實際 source 行為 |
|---|---|
| 應用設定檢查 | `pdf2zh_next/config/translate_engine_model.py:489` 的 `TencentSettings.validate_settings` 仍要求設定內 id/key，然後 clean string。**完整舊應用不是「只給 SDK environment 就能省略這兩個設定」。** 直接建 translator 才可能略過這層驗證。 |
| translator constructor | `pdf2zh_next/translator/translator_impl/tencentmechinetranslation.py:29` 先執行 `DefaultCredentialProvider().get_credential()`；只 catch `OSError` 才改用設定內 id/key。成功的 machine chain 可覆蓋已明確提供的設定值。SDK／INI／驗證錯誤不能全部轉成 explicit fallback。 |
| 1. environment | `.venv/Lib/site-packages/tencentcloud/common/credential.py:279`：讀 `TENCENTCLOUD_SECRET_ID`、`TENCENTCLOUD_SECRET_KEY`；任一 None／空字串為 unavailable，完整 pair 建 `Credential`。該 `Credential` 拒絕前後空白。這條分支**沒有**讀 `TENCENTCLOUD_TOKEN`。 |
| 2. INI profile | `credential.py:298`：先選 `~/.tencentcloud/credentials`，不存在才選 `/etc/tencentcloud/credentials`；不是合併兩檔。用 `ConfigParser` 的 `[default]`、`secret_id`、`secret_key`，value strip；讀到的 `role_arn` 並沒有用來自動 AssumeRole。缺 `[default]` 的現有檔案可能留下未定義 instance attribute，形成非 OSError；不能默默當作無檔或換讀第二檔。 |
| 3. CVM role | `credential.py:79,126,144,165`：`http://metadata.tencentyun.com/latest/meta-data/cam/security-credentials/` 先取 role，再取該 role JSON。欄位是 `Code=Success`、`TmpSecretId`、`TmpSecretKey`、`Token`、`ExpiredTime`。CVM 多數 fetch／parse failure 在內部吞掉，缺 credential 時回 None。`urlopen` 沒設 timeout；成功物件在距到期≤300秒時 refresh，並以 lock 取得一組一致的 id/key/token。 |
| 4. TKE OIDC | `credential.py:356,379,508`：chain docstring 只寫前三者，但實際還有 `DefaultTkeOIDCRoleArnProvider`。需要 `TKE_REGION`、`TKE_PROVIDER_ID`、`TKE_WEB_IDENTITY_TOKEN_FILE`、`TKE_ROLE_ARN`；缺項或 token file I/O 的 `EnvironmentError/OSError` 才會讓 translator constructor 使用 explicit fallback。成功建立 OIDC 物件時還沒打 STS。 |
| TKE 首次簽名／refresh | `credential.py:477–506`：首次 `get_credential_info()` 才以 `AssumeRoleWithWebIdentity`、version `2018-08-13`、`SkipSign=True` 呼叫 `sts.tencentcloudapi.com`。有 ProviderId／WebIdentityToken／RoleArn／以時間產生的 RoleSessionName／DurationSeconds=7200；有效期限採 `ExpiredTime - duration*0.1`。之後每次 refresh 會重讀 TKE token file。此時的錯誤發生在 `do_translate` 內，走其5次 retry；**不再回 constructor 改用 explicit key**。 |

`credential.py:356–376` 是完整選擇順序；`TencentCloudSDKException` 在 `common/exception/tencent_cloud_sdk_exception.py:5` 繼承普通 Exception，不是 OSError。一般非 TKE 本機走到缺 TKE_REGION 時才常見 explicit fallback，不代表 SDK chain 本身就是「所有失敗都 fallback」。CVM refresh 保留舊值／吞例外等舊細節若不複製，須明列新的 typed 邊界。

### Rust 現況及最小安全實作範圍

`crates/config/src/providers.rs:237–247` 缺明確欄位就回 UnsupportedCredentialSource；`crates/core/src/providers/legacy/tencent.rs:108–119` 只驗這組設定，簽名使用 config api_key，另可從 core option 送 static `tencentcloud_token`。沒有環境／INI／CVM／OIDC 選擇或 refresh。現有 TC3 body／signature 與5次應用 retry 的測試，不能證明這條 chain。

建議分成「明確輸入的 credential source plan」和「執行期 resolver」，不要把 profile／metadata I/O 塞進 `Settings::set`、`main_translator_config` 或 GUI 建構：

1. ConfigStore／Invocation 收到的 **explicit environment map** 與 explicit HOME／profile paths 放在不可序列化的 runtime sidecar，維持 pure builder。完整舊應用的設定欄位 required 規則與 core 直接使用 resolver 的能力要分清楚；放寬前者是 extension，不是還原舊行為。
2. core 新增可注入的 Tencent resolver／credential handle，於真實任務的 async prepare 選來源；保存來源狀態並在每次 TC3 簽名前 refresh。env/profile成功優先於 explicit key，只有 constructor-resolution 的 OSError 類失敗才使用 explicit fallback。CVM／STS 用既有單次 transport、cancel 與 clock；不引入藏有額外 retry 的 SDK。若只先做 env+INI，必須明列 CVM/TKE 仍未移植，不能改稱完整 chain。
3. resolver 的 id/key/token／OIDC token 必須加入同一安全遮罩範圍，且沒有 Debug／Serialize。現 `ctx.error` 只知道 config 內原 key，不能假定它會遮住新解析出的 credential。不得把解析值寫回 TOML、cache namespace plaintext、Worker start frame 或 stderr。
4. 呼叫端必須明確選擇 legacy discovery policy。測試與 dry UI 不用 `std::env`／真 HOME 猜測，metadata/STS 位址可注入 loopback；不能為省事先執行一次未 mock 的 DefaultCredentialProvider。若產品決定改成 explicit-first／禁 metadata，這是可文件化的安全決策，但不能宣稱 legacy 順序相同。

最小改動面：`config/{storage,providers,lib}.rs` 的 runtime sidecar／factory輸入；core 新 credential module、Tencent簽名／prepare與安全遮罩接線；共用 tasks factory只傳 plan。CLI／GUI各自實作一份 chain 或 Worker持有key都不是合適位置。

應補的 hermetic oracle：env pair 覆蓋 explicit＋不讀檔/HTTP；partial env→profile；profile HOME 優先／`[default]`與錯誤邊界；CVM role＋token與300秒refresh／並行鎖；TKE constructor沒有STS、首次簽名與到期refresh／重讀token；只有 constructor OSError fallback、SDK錯及refresh錯不fallback；解析中取消不送TMT；TMT簽章/X-TC-Token真loopback；所有解析credential不進錯誤／Debug／TOML／cache／Worker。檔案全用 TempDir，env全用明確map，clock可控。Python對照也需先 patch environment、expanduser／exists／file讀取與 urlopen／CommonClient，才可建 provider。

## 2. `translate("Hello", ignore_cache=True)` 初始化健康檢查

### 次數與順序

`pdf2zh_next/translator/utils.py:24–74` 在每次 `_create_translator_instance` 完成 constructor、讀推薦 qps/pool 後，用 **translate，不是 llm_translate** 送 `Hello`，`ignore_cache=True`、rate_limit_params=None。`BaseTranslator.translate`（`base_translator.py:84`）仍計一次 call、等一次 QPS、執行原 provider/SDK retry，且不 cache.get/set。OpenAI 等 token counters 正常記這次用量。健康檢查不是只驗 URL／版本，也不是零付費的本地驗證；失敗會阻止該 translator 回傳。

`translation/configuration.py:43–63` 的 main factory 先跑完並套用推薦設定，才決定 term：

| 舊設定情況（每個 PDF config） | Hello 邏輯呼叫 |
|---|---|
| 沒有可用 term，例如非LLM main且沒有明確 LLM term | main 1次，term 0次 |
| engine settings 相等，且 **raw term_qps == main 有效 qps** | 共用 main instance，共1次 |
| 不同 term engine／設定／QPS | main 1次，term 1次；先main後term |
| 預設 main是LLM、未明確term、term_qps=None | **通常2次**：Python的比較是 None==qps，並未先把None換成main qps。後續 `get_term_translator` 才用 `term_qps or main_qps` 建 limiter。 |

不要加 `no_auto_extract_glossary` 就略過已配置 term 的初始化：原 `build_babeldoc_config` 的上述分支沒有這個條件。`high_level.py:167,484` 每個 PDF 建 config，舊CLI／GUI多文件流程各次再建 translator；實際成功建立 instance 次數因此也隨文件數增加。SiliconFlowFree 的 main constructor 推薦值會先改 main qps，再影響共享判斷與 term limiter，不能只比初始整數。

### Rust 差異與落點建議

- core `TranslationService::prepare`（`crates/core/src/lib.rs:248`）只建空文字 context並進 provider preflight；`providers/mod.rs:262` 目前做 Free bootstrap、Xinference auth、CLI validation／Claude version等，**不會翻譯 Hello**。
- `tasks/src/settings.rs:10–43` 在pure factory先建兩份正規化 config，按完整 `same_service` 消重；term_qps=None先在 config轉換成main qps，預設會共用，與上表 raw None 情況不同。
- `tasks/src/lib.rs:284–303` 整batch共用同一組 Arc；bridge每文件呼叫 prepare（`babeldoc-bridge/src/lib.rs:197,219`），OnceCell使bootstrap每service只一次；結果在`:268`明列 service-lifetime metrics，與Python每文件新instance counters也不同。

最小安全落點是 **共用 tasks factory生成pure execution plan，TaskManager／共用PDF執行preflight在明確翻譯開始後await health**，並保留 bridge 在spawn Worker之前的順序。core可提供一個以既有公開 translate_typed 執行 Hello 的helper，但不要把翻譯呼叫塞進 `ensure_prepared`：它會再次呼叫 ensure_prepared，形成初始化遞迴／鎖死，也會改掉單獨core文字服務的API語意。

必須先選定並寫清楚相容目標：若要精確舊次數／狀態，保留raw term共享條件、main推薦值後再建term，且每PDF新建services或明確重演每PDF初始化。若繼續「每batch一組service，按有效config消重」的現設計，則可每新service只health一次，但它是減少探測／用量的可見差異，不能叫完全等價。單加一個每Arc OnceCell不會解決上表的2次及多PDF差異。

health request應固定 translate/Hello/ignore_cache=true/Null rate params，用既有limiter、retry、安全錯誤與usage，失敗保留typed cause。應先通過設定與input／glossary存在性檢查，再health，避免無效輸入也先消耗API；現bridge的prepare位置已在這些檢查後。取消必須cooperative await，尤其CLI／Claude，不把health future包進會drop cleanup的select。warmup／offline-assets、parse/load/save設定、列metadata、打開GUI／預覽、只建TaskSpec都不得health或啟Python。

應補：主無term1次／raw None預設2次／明確相等共享1次／不同term2次／多PDF次數；main成功才到term，失敗Worker零啟動；所有healthcache get/set零次且usage/QPS計一次；Free推薦值與term建立順序；HTTP／backoff／native-process health取消；dryUI與pure factory零HTTP零process。用注入executor／loopback與受控native child，不可執行真付費或免費API，也不可執行使用者Claude／command。

## 3. 除 HTTPX write/pool timers 外的刪除門檻

整合者後續更新：標準 gzip／deflate／Brotli／zstd 解碼已開啟，六個真 HTTP 測試涵蓋 JSON／HTML、OpenAI 接線、解壓後 4 MiB 限制、壞資料及取消；完整 workspace **211 passed**、Clippy 通過（`compressed-http-workspace-tests.log`、`compressed-http-clippy.log`）。下表保留原發現；這項主要缺口已修，raw 非標準 deflate／多層 encoding 尚未逐項比對。其餘列仍是待處理項。

再更新：AnythingLLM／Dify 已改成每次 HTTP operation 的獨立 cookie jar；同次 redirect 保留 cookie，下次呼叫／retry 不保留，兩個真 wire 測試通過。其他使用 persistent Session 的 provider 維持既有 jar。另加入 Worker `-I -B -u` 真程序旗標測試；最新完整 workspace **214 passed、1 ignored**，all-targets Clippy 通過，證據為 `transport-cookie-bytecode-verified-tests.log`、`transport-cookie-bytecode-clippy.log`。下表 Cookie 列的 AnythingLLM／Dify 差異已修，DeepL prepared-request 的細節仍待核對。

以下是本輪 source 找到的**具體行為差異**，不是把缺測試當成已證實bug；本輪未執行新fixture。

| 項目 | Source與影響 | 最小下一步 |
|---|---|---|
| HTTP content decoding | 根 `Cargo.toml:17` 關閉reqwest defaults，只開json/rustls/query/form；core只再開cookies。沒有gzip/deflate/br/zstd解碼feature。`transport.rs:263–273` 直接收chunk，JSON／text再解析。鎖版HTTPX `_decoders.py:381` 與requests `models.py:937` 會解content encoding。compressed JSON／Google-Bing HTML可能在Rust成為解析錯誤。 | core transport／manifest補已選decoder能力；以真loopback送gzip/deflate JSON與HTML、malformed compressed body、解壓後>4MiB及取消，維持**解壓後**body bound與正確retry分類。optional br/zstd按舊安裝的實際decoder能力決定，不能只加Accept-Encoding header。 |
| Cookie scope | `transport.rs:87,104,131` 每service共享persistent Jar。AnythingLLM `anythingllm.py:48`、Dify `dify.py:50` 每次用top-level requests.post；`requests/api.py:68` 每呼叫新建並關閉Session，只在該次redirect chain保留cookie，不跨attempt／邏輯翻譯保留。Rust會額外送前次Set-Cookie。 | factory／transport支援per-operation cookie scope；真loopback驗同一chain redirects需cookie、下次retry／翻譯不得帶上。Google/Bing等原Session不可一起關掉cookies。DeepL `_prepare_request`（`deepl/http_client.py:212`）直接Request.prepare而非Session.prepare_request，也需獨立核對initial request與redirect cookie，不宜只按「用了Session」套同一政策。 |
| Azure operation timeout邊界 | Rust `legacy/azure.rs:89–96` 已有604800秒budget，**不是完全缺deadline**；但remaining同時限制connect/read，耗盡一律ReadTimeout。鎖版 `_retry.py:359–389` 只縮connection_timeout，read仍RequestsTransport預設300（`azure/core/configuration.py:137`）；expiry按前一錯誤分類成ServiceRequestTimeoutError或ServiceResponseTimeoutError。 | HttpRequest支援獨立connect/read或provider override；可控clock推到剩餘<300，注入request/read history，驗不同timeout與typed最後cause。不能以普通300秒欄位或SDKbudget計數測試關閉此差異。 |
| Tencent chain與factory health | 前兩節的來源選擇、Hello數量／時機、Free main/term推薦值和instance生命期。 | 在授權範圍內補功能，或明確批准不同的產品政策；現有16provider mapping測試不是這些流程的替代證據。 |

還有 **證據缺口／產品邊界**：

- 矩陣已列的真wire T02/T03/T05、HTTPX20／Xinference30 boundary、G01與R29/R30/R33的精確舊序列，尚不是已證實runtime差異。優先補能跨過真transport／factory的fixture，不為照抄policy常數補假正例。沒有真API不是必須去花費才能補的阻擋，遠端驗證需另行明確授權。
- SDK environment/proxy/CA行為尚不能說完全等價：Python HTTPX/requests預設trust_env（含proxy／CA／部分netrc），Rust使用另一套TLS/HTTP實作，ConfigStore的explicit map只承諾特定SDK URL fallback。不能由「Rust沒有直接讀HOME」推論reqwest不讀任何process environment；`reqwest/client.rs:417`仍建立system proxy matcher。若發行承諾支援企業proxy／customCA，需要明確transport policy與synthetic env／loopback TLS測試；本輪沒有讀真proxy／CA／netrc或把它們斷言為全部不支援。
- typed malformed response、4MiB bound、跨origin credential stripping、安全錯誤、Windows整樹reap與cancel控制是已明列的移植決策；應保留，無須為逐Python物件等價恢復舊缺陷。非Windows程序樹保證仍不能引用Windows fixture代證。
- RPC doclayout原Python與Worker都沒有可觀察使用，保留欄位不代表缺少一個已工作的舊provider功能。cache與GUI已完成的項目應更新各自快照，勿再列為未移植。

**判斷：目前不宜以「全16provider完全等價」為由刪除舊Python翻譯／factory實作。** 至少先處理／明確接受本文件列出的chain、health與transport行為差異，更新逐項證據及保留的安全決策，再檢查舊入口與公開import相依。這不要求把每個SDK所有API搬進Rust，也不要求真API自動測試。即使應用翻譯層可以刪除，BabelDOC的Python engine closure仍是另一條既定runtime依賴，不能一起當作已Rust化而移除。
