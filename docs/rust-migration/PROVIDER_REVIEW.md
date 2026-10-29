# 第 5 階段 provider 可靠性審查

日期：2026-10-27；續審：2026-10-29。此文件記錄 Astra 審查的 source 證據、具體修正要求、測試 oracle 與父代理提供的執行結果；**不是 Rust 已完整等價或可發行的證明**。實作仍由各 Sol worker 整合中。

## 範圍與證據

本輪唯讀檢查 `crates/core/src/{lib,error,retry,limiter,metrics,transport,process}.rs`、`providers/{mod,openai,local}.rs`、`providers/legacy.rs` 及 `providers/legacy/{web,azure,deepl,tencent,cli}.rs`，並檢查 `tests/provider_compatibility.rs`、`tests/legacy_providers.rs` 及 bridge `src/lib.rs`、`tests/lifecycle.rs` 的相關 oracle。未修改實作，未執行 cargo 或重跑既有 Python pytest。

既有行為基準採 [RETRY_COMPATIBILITY.md](RETRY_COMPATIBILITY.md) 的 R01–R34 與 T01–T05，並對照現行 Python provider、`BaseTranslator`、鎖版 SDK 原始碼。靜態圖不能證明動態 decorator、SDK 分支或程序清理，因此關鍵判斷另讀 source。2026-10-27T07:33:51Z 的 coverage 對已落檔的共用模組、legacy 五檔及 process 均回 `no_recorded_issue / metadata_match`；持續寫入中的檔案以本輪讀到的內容為準，不把一次 coverage 當永久有效。

新增 OpenAI、lib、legacy.rs 的 coverage 於 generation `2026-10-27T07:40:08Z` 皆為 `metadata_match`。另直接檢讀本機 registry 的 reqwest 0.13.5 `Response::chunk`，確認 body frame 錯誤經 `error::decode` 包裝，不能只用 `is_body()` 分類。

local.rs、legacy_providers.rs、Python ollama.py／xinference.py 在 generation `2026-10-27T07:42:41Z` 亦為 `metadata_match`。Ollama 非串流解析與 Xinference auth／model fallback 另核對 `.venv` 的鎖版 SDK。

續審 coverage generation `2026-10-28T12:07:00Z` 對 core process、provider_compatibility、providers/mod／local／openai 及 bridge lib／lifecycle 均為 `metadata_match / no_recorded_issue`；遮蔽修正與 provider_compatibility 於 generation `2026-10-28T12:10:23Z` 再查亦相符。10/29 的 redirect 續審以 generation `2026-10-28T12:19:25Z` 檢查 core factory／transport／Azure、兩份 provider 測試與 CLI，亦相符；最後 Azure body-error source 與測試直接重讀，並於 generation `2026-10-29T09:35:32Z` 再查為 `metadata_match / no_recorded_issue`。

父代理10/29最終回報：redirect與Azure body-error修正後，`cargo test --workspace --locked` **102/102通過**（framing3、bridge13、CLI1、core14、legacy40、provider31）；fmt check與clippy all-targets、`-D warnings`通過。結果記錄於 `.test-output/rust-migration/phase5-verified-tests.log`。Python 279/279（tests與worker共271，加cache8）是此前同階段已通過的回歸快照；本輪Rust redirect改動未重跑Python。這些是父代理提供的執行結果，並非本審查者另行執行。

## 已送修事項

| ID | 具體觸發與影響 | 修正／驗證要求 | 本輪狀態 |
|---|---|---|---|
| P01 | `sanitize` 先截 4096 字再替換完整 credential；跨邊界的 key 只剩前綴，無法匹配而洩漏 | 在原始訊息搜尋完整值，再遮蔽可輸出區間；key、token、來源文字各做跨界 case | 已關閉；oracle 檢查實際洩漏的前6字及遮蔽尾端，父代理回報通過 |
| P02 | 自訂 `headers.X-Auth` 不符合欄名猜測；`Authorization="Bearer TOKEN"` 只收完整字串時，錯誤內裸 `TOKEN` 未遮蔽 | 所有支援的 header 容器值納入遮蔽；認證 scheme 後的 payload 另納入；驗完整值與裸 token | 已關閉；header 遞迴收集與 payload 遮蔽，父代理回報通過 |
| P03 | cross-origin redirect 去除已知 Authorization，卻保留 Tencent `X-TC-Token` 或自訂 `X-Auth` | 移除 credential 與使用者自訂 header；兩個 loopback origin 的 307/308 驗第二站無 credential | 已關閉；真 loopback 驗跨 origin 移除，父代理回報通過 |
| P04 | A 在長時間 preflight；B 等 `OnceCell::get_or_try_init` 時自身取消，仍等 A 完成 | 可取消地等待初始化鎖；初始化者 cooperative await，不能因外層 select 丟棄 CLI kill/reap future | 已關閉；gate oracle 驗 B 可取消、A 不被丟棄且結果仍快取，父代理回報通過 |
| P05 | OpenAI 未設定 timeout 與顯式 float 混成單一 mandatory 值，令 connect 預設與 read 相同 | 未設定：read/write/pool 600、connect 5；顯式 float：各 stage 同值。CLI 與 core 必須保留此區別 | 部分關閉；CLI 改 Option，None 對應 read600/connect5，顯式 float 保留；core factory/request 與 CLI parse oracle 已核對且父代理回報通過。read/write/pool的完整stage語意仍未證明等價 |
| P06 | DeepL 把一般 `Transport`／`ReadTransport` 全視為 SDK 內層可 retry；Python 的 malformed chunk／redirect loop 屬其他 RequestException，`should_retry=False` | transport 提供足夠分類；`Response::chunk` 的 `is_decode()` 亦須歸 ReadTransport；真 loopback 短 body／redirect loop 驗不進六次內 retry | 已關閉此分類問題；真 body／redirect 與修正後 timeout oracle 父代理回報通過；redirect 次數改30的後續驗證另見 P15 |
| P07 | DeepL 直接把 `lang_out="zh"` 映為 `ZH`，略過 Python BaseTranslator 先映 `zh-CN` 的步驟 | 完整 public translator 語意應送 `ZH-HANS`；不能只比較 `_map_target_lang` | 已關閉；source 與 request fixture 核對，父代理回報通過 |
| P08 | 自訂 `Authorization` 與預設 `authorization` 同時保留，reqwest `.header` 會 append，造成重複或錯誤認證 | 以大小寫不敏感規則合併，後者覆寫；驗 wire 只有一份最終值 | 已關閉；真 wire oracle 驗只有一份最終值，父代理回報通過 |
| P09 | Free preflight 把 endpoint futures 放入 Vec 後逐一 await，與註解及 Python 並行探測不同；且缺 status 的200 JSON被誤當fatal | endpoint之間並行、各自3次串行；缺status／invalidJSON算失敗探測；明確非ok+message才fatal | 已關閉；gate 證明不同 endpoint 並行，缺欄位／無效 JSON／fatal 分支已測，父代理回報通過 |
| P10 | bridge 的 shutdown timeout 後 `requests.abort_all` 可能 drop 正在 CLI kill-and-reap 的 future | process service 必須 cooperative join 到完成；alias 判斷須同 canonical 規則。整合測試穿過 bridge，以取消前取得的固定 HANDLE 驗回傳時 root／descendant 已退出 | 已關閉 Windows 受測路徑；process JoinSet 不 abort，Job 等 ActiveProcesses=0，固定 HANDLE 立即驗證，父代理回報通過 |
| P11 | Ollama 非串流200含error欄位被誤轉成 ResponseError，造成100次；缺message.role反而可能成功 | 對照 SDK 非串流 ChatResponse；error-only／缺role為 malformed，不 retry；完整有效回應附error欄位仍按內容返回 | 已關閉；三種回應分支及 HTTP 次數 oracle 父代理回報通過 |
| P12 | 依序替換 source 或 key 會破壞另一個重疊匹配；交換順序仍可能洩漏來源或 key 前後段 | 在未改寫的原始訊息定位全部區間並合併；測互相包含、部分重疊、同值自重疊 | 已關閉；source 重讀為原始區間 union，四類 oracle 精確比對完整安全訊息，父代理回報通過 |
| P13 | Xinference auth preflight 的200缺auth欄位被當false並繼續；Python初始化會失敗 | 缺auth應malformed，不能再送model請求；保留404相容分支 | 已關閉；缺 auth 初始化失敗且不續送 model，父代理回報通過 |
| P14 | Free推薦預設10/100被無條件套用到原limiter；Python只在/config有效時改既有limiter，失敗只保留推薦值 | 分開recommended值與limiter更新；原qps2+/config500應檢查實際0.5秒admission | 已關閉；apply_qps 分離、bridge 不覆寫此決定，實際0.5秒 admission oracle 父代理回報通過 |
| P15 | 所有 provider 共用20次 redirect，會改變 requests 的30次上限、Free不follow及Azure獨立 RedirectPolicy | factory 按SDK選20/30/0；Azure自行在SDKretry外協調30次，303後按GET規則，按target重置budget | 已關閉受測範圍；source與oracle已核，父代理102項統一測試通過。HTTPX20／Xinference30目前是接線 source 證據，未聲稱每個provider皆有boundary runtime test |
| P16 | Azure 303後GET遇截斷／malformed body，被當 read budget並增加history，造成錯誤sleep及混合序列次數 | 對照鎖版 RequestsTransport：HttpResponseError／IncompleteReadError 消耗status budget；PipelineRequest無response，不append history | 已關閉；真短body×3→成功驗5HTTP/零sleep，body與503混排驗25HTTP/僅outer sleeps，父代理102項統一測試通過 |

錯誤需保留 typed cause 與最末 retry layer；保留 cause 不表示可以保留未遮蔽的 provider body、URL、credential 或文件文字。Rust error chain 與 Python IPC generic error 分開，不能以 wire 已泛化推論 Rust log 也安全。

## 本輪 source 已核對的正向設計

- `RetrySelector` 在所有 selector 前排除 `Cancelled`；predicate 對 `Interrupt` 的相容行為與任務取消分開。E/P/H/F 的初值、封頂及 N attempts／N−1 sleeps 形式符合基準。
- process selector 不以外層 cancellation select 丟棄 operation；CLI `run_process` 收到取消／timeout 後先終止 Windows Job，等待 ActiveProcesses=0，再 wait root，才回傳。Claude translation timeout 分類為 Value，CLI timeout 分類為 ProcessTimeout，保留 R31/R32 差異。
- limiter 持鎖等待、通過後才更新 `next=max(previous_next,now_after_sleep)+interval`；取消 waiter 不預約未來時槽。QPS admission 在 provider 與所有 retry 之前一次；額外 concurrency semaphore 是獨立選項，預設無 inflight cap。
- Azure POST 沒有 response 時不符合 SDK method allowlist，因此 network error 直接交 outer retry；POST 500/503/504 與有 Retry-After 的 >=400 回應另有 status retry 分支。這與本機 azure-core 1.41.0 source 相符。
- DeepL 使用預先建立的 deadline 扣除 HTTP 耗時，再等待剩餘時間，並以 `1 + 0.23*(2*u-1)` 產生 jitter；未以 Retry-After 取代這個 SDK timer。
- OpenAI SDK loop 最多3次，outer100只選429；`retry_after` 的 ms 優先、finite >120 先拒絕、overflow sentinel、`x-should-retry`、向下 jitter 分支已對照 source。可控 executor 已驗429為300 sends、299 sleeps，>120優先拒絕內層 retry，及固定 wall clock 下的日期邊界；這不等於真網路 timeout 的所有 stage 已等價。
- Xinference 依分號順序只跑一輪模型 fallback，取消及 Interrupt 不被吞掉；全部失敗為普通 Provider error，不重跑100輪。Ollama 預設無timeout、num_predict按文字字元數乘5單調提高，selector僅ResponseError。
- Google 400 sentinel、Bing 每個 attempt 重新取得 SID、Tencent SDK 無內層 retry、CLI postprocess 失敗重跑整段翻譯的分界已在 source 中保留。

上述敘述以 source review 為基礎；有執行證據的項目另見表格與父代理結果。service／bridge 上層已確認 main 或 term 任一 service requires_reap 時 cooperative join 到完成；僅純 HTTP 路徑在 grace 到期後 abort。此保證不涵蓋外部強制終止 Rust host，也不聲稱作業系統異常時仍有絕對完成時限。

新增 legacy tests 已包含 DeepL 30次／deadline／jitter、真短body與redirect loop、Azure分層／日期header、Google無timeout取消、CLI與Claude受控native程序。原本 task 返回後才 eventually 等 descendant 的 oracle 已改為取消／超時前開啟固定 HANDLE，task 返回後立即 GetExitCodeProcess；OpenProcess 必須先成功，不能將開啟失敗當成已退出。父代理回報 native process 各項與穿過 bridge 的真 CLI case 通過。

DeepL oracle 已修正並在父代理90項快照通過：鎖版 SDK 的 request timeout 是 `max(deadline-now,10)`；零 HTTP 耗時、RNG=0.5 時，六次應為 `[10,10,10,10,10,10.48576]`，不是全部10。這次修正的是測試預期，未改 provider 行為。

## 10/29 redirect 續審

核對本機鎖版 `httpx/_client.py`、OpenAI／Ollama client、Xinference 的 requests Session、Azure `_redirect.py`／`_retry.py`／RequestsTransport。core factory 現在給 OpenAI family及Ollama上限20，requests providers（含Xinference）上限30，Free與Azure的底層transport不follow。Free原本是 `httpx.Client(timeout=100)` 且未開 follow_redirects；實際307 fixture驗原站3次、目的站0次、最後HttpStatus307，outer H只睡30/30。

Azure的RedirectPolicy位於SDK RetryPolicy外：POST301/302不follow，300/303/307/308可follow；303轉GET但保留原body；相對Location由origin根目錄解析，例如 `/prefix/translate` 收到 `child` 應到 `/child`。每個新target重新建立total/connect/read/status budget。GET可重試無header的408/429/502與506–998，且connect/read各有獨立budget；POST的network例外仍直接交outer。30次redirect之後仍收到redirect時，每outer共31HTTP，5個outer共155。

已檢查新增fixture的精確斷言：POST301/302各只回原站5次；前target先503兩次再307，新target503三次仍能成功，SDK sleeps為1.6/1.6/3.2；303後GET429無header可在原target成功、body仍相同；GET的connect/read/status混合各3次消耗獨立budget；短body例外改用status budget且不新增history，混503序列不多睡；307/308兩origin的目的站無configured key、region及Authorization。

跨origin移除所有credential與自訂header是刻意的安全差異。舊Azure SDK的authentication policy可能在下一站重新加入subscription key；Rust不重加。同origin保留subscription key。HTTPX20與Xinference30接線已讀source，但本輪未新增每個canonical provider／alias的完整boundary oracle；不可由Free、DeepL或Azure的runtime結果推論所有接線均經同樣測試。

## 不可簡化的測試 oracle

| 範圍 | 必要斷言 |
|---|---|
| R01–R22 policy | 每列都測 selector 正反例、attempt N、sleep N−1、最後 cause；不能只測共用 enum 常數而沒驗 provider 選對 policy |
| R23–R28 legacy HTTP | Google 429/503/timeout 各 5 attempts 與 1/2/4/8；400 sentinel；Bing 每 retry 重新 SID；一次 logical translate 只 admission 一次 |
| R29、T01–T05 OpenAI | 區分 mock SDK calls 與真 HTTP attempts；無效／空 choices 失敗；400 保留安全訊息；timeout 是 SDK 3 attempts，outer 不重試；429 可 SDK 3 × outer 100 |
| OpenAI Retry-After | `120` 可用、`120.001` 先拒絕 SDK retry，且優先於 `x-should-retry:true`；outer 429 仍獨立運作。ms 優先於秒／date；固定 wall clock。缺值、無效字串、零、負值、NaN、Infinity literal、數字 overflow 分開測 |
| OpenAI jitter | 固定 RNG 0、0.5、接近 1，驗 `min(0.5*2^k,8)*(1-0.25*u)`；outer E 不加 jitter |
| R30 SiliconFlowFree | 純429只有100 inner attempts，exhausted 不符合 outer HTTP selector；純503只有3次；429→HTTP error→429 重進 inner 時 counter 重設 |
| R31/R32 process | 用受控真 child 測 timeout、cancel、postprocess；回傳前 root 已 wait；Windows descendant 也已退出。Claude timeout 一次、CLI 三次及 2/4 waits |
| R33 QPS | qps2 開始間隔 0.5；idle 不累積 burst；set QPS 不重設舊 next；取消排隊不留時間洞；預設允許兩個翻譯同時在途；HTTP retries 不再 admission |
| R34 bridge | 取消須解除 reader/writer、outstanding Futures、provider waits，並清理受控程序樹；舊 Python mocked case 不足以證明 Rust OS lifecycle |
| DeepL SDK | 5 retries = 6 attempts；outer predicate5 可到30；HTTP 耗時消耗 backoff；初始1、後乘1.6、jitter±23%；RequestException 子類不同；不讀 Retry-After |
| Azure SDK | POST429 無header只outer5；有header進SDK status budget；500/503/504每outer最多4 sends；首次backoff0不呼叫sleep，其後1.6/3.2；network在POST不進SDK重試 |
| redirect／Azure GET | 區分HTTPX20、requests30、Free0與Azure自行30；303轉GET保留body、每target重置SDK budget；GET body exception耗status但不append history；跨origin不重加key |
| Ollama/Xinference | Ollama非429 ResponseError仍retry，connection不同；Xinference逐模型fallback全敗後一般 error，不應再跑100輪；取消不被fallback吞掉 |
| metrics | calls、admissions、http_attempts、retry_sleeps、inflight／peak 與 usage 各有明確定義；SDK inner sleep 若納入 retry_sleeps，不能漏記，也不能把QPS等待算入 |
| error safety | 已知key、所有customheader值、Authorization裸payload、source、URL及控制字元；跨截斷邊界；nested RetryExhausted 的最末 cause 同樣安全 |

時間、wall clock 與 RNG 必須可控；long backoff 測試不可真睡。fake executor 可精準核對操作序列，但 transport 的 timeout、redirect、chunk decode、credential header 與程序清理仍要 loopback／真子程序測試，不能只靠直接注入已分類的錯誤。

## 尚未驗證與明確界限

本輪已取得上列父代理測試結果，未測真實 provider API。bridge 的 bounded progress、main／term metrics、200 inflight 與 CLI 取消已有受控 fixture 證據；不能把這些視為所有 BabelDOC 任務或完整 UI 路徑的證明。Cache 與其 get/set 失敗語意、完整設定與 CLI 預設、所有 provider request shape 仍需後續階段驗證。R01–R34／T01–T05 是遷移基準，現有測試總數不代表已逐列完成相同層級的覆蓋。

Rust 型別化輸出會把 AnythingLLM 缺 textResponse、Dify 缺 text 而回 None／list 的舊行為改為 malformed error；這是需明列的相容性決策。CLI 在 Windows 的文字 stdin 換行轉換與 native process oracle 已補；非 Windows 的 ProcessScope 目前未提供 descendant tree 等待，不能外推 Windows 結果。CLI 的未設定 OpenAI timeout 已修正，read/write/pool各stage與Python requests/HTTPX的精確語意仍需要獨立fixture或明確記錄差異。P05剩餘範圍及完整設定／cache等後續階段尚未關閉，因此本階段不是可發行等價版本。本次審查截至上述102項通過快照凍結，未見本輪限定範圍內的剩餘阻擋。
