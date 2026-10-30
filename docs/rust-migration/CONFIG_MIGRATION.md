# Rust 設定管理遷移

更新：2026-10-30。本階段新增 `crates/config`（`pdf2zh-config`），不刪除或改寫原 Python 設定實作。行為來源為 `pdf2zh_next/config/{model,translate_engine_model,cli_env_model,main,storage,cli}.py`、`const.py`、`translation/configuration.py` 與 translator factory；SDK / provider 實作的個別界限仍見 `PROVIDER_TEST_MATRIX.md`。

## 資料與公開介面

`Settings` 保留舊 `CLIEnvSettingsModel` 的 TOML 結構：`basic`、`translation`、`pdf`、`gui_settings` tables，頂層 provider 選擇旗標與各 `*_detail`，以及 `term_*` 旗標／detail。內部保留完整 JSON tree，未知 key、table、array 都可讀寫後保存，不經只容許已知欄位的 struct 丟棄它們。

同一份 `src/schema_data.json` 提供 235 個可列舉欄位與 24 個設定選項。它由本階段檢讀的 Python model source AST 擷取欄位型別、default、help 與原始順序，成為 Rust CLI／後續 GUI 共用的 runtime metadata；建置及執行不需要 Python，也不 import 設定模組或讀取使用者檔案。Python model 新增欄位時，仍須更新並審查此 snapshot 與 typed sections，不能假設會自動同步。

| API | 合約 |
|---|---|
| `field_specs() -> &'static [FieldSpec]` | `path` 是 TOML dotted path；`flag` 不含 `--`；`env` 是完整 `PDF2ZH_*` 名稱；另有 `kind/default/nullable/secret/help`。`FieldType` 為 Bool/Integer/Float/String/StringSet。 |
| `engine_specs() -> &'static [EngineSpec]` | `name/flag/detail_key/supports_llm`；保留 Python metadata 順序。24 個選項中有 8 個轉為 OpenAI，因此對應 16 種核心 provider。 |
| `Settings::from_value(Value)` / `from_layers(&[Value])` | 補預設值、正規化已知型別、保留未知欄位；layers 依最高到最低優先序提供。解析結構與驗證可執行的 translator 是不同步驟。 |
| `basic()/translation()/pdf()/gui()` | 回傳 `Result<BasicSettings/TranslationSettings/PdfSettings/GuiSettings>` owned snapshot；公開欄位與 flatten extra，修改 snapshot 不會暗中修改原 Settings。 |
| `get(path)` / `set(path, Value)` | 讀寫 dotted path；set 只改記憶體、對已知欄位做型別檢查。true 的 provider flag 選定該 engine，清除同組 peer flags；main 與 term 分開。literal key 自含 `.` 時可透過 `as_value()` 取得原 tree。 |
| `as_value()` / `into_value()` / serde | 完整資料供明確 persistence／UI 取值；可能包含 credentials，不是 logging API。 |
| `validate()` / `parse_pages()` | 純設定檢查，不檢查輸入 PDF 是否存在、不建目錄、不連網、不找外部 CLI。後續 bridge／task boundary 負責真實 input 與資產檢查。 |
| `main_translator_config()` / `term_translator_config()` | 分別回 `Result<TranslatorConfig>` / `Result<Option<TranslatorConfig>>`；敏感資料只留 Rust host。 |
| `pdf_job(PathBuf)` | 回傳供共用 bridge 使用的 `PdfJob`，不依賴 bridge crate；不包含 API key、provider endpoint 或 Rust ignore-cache policy。 |

`Settings`、四個 typed sections、`ConfigStore` 與 FieldSpec 的 Debug 都不輸出設定值。錯誤只保留安全 kind 與已知欄位名，不附 TOML parser excerpt、值、URL、OS error text 或原始 source error chain。

## 目錄、優先序與持久化

`ConfigStore::new(absolute_app_root)` 的位置固定為 `app_root/config/config.v3.toml`。相對 app root 是錯誤，工作目錄不參與預設 config 路徑。`for_current_executable()` 取可執行檔父目錄；開發版若也用此方法，位置就是該開發 executable 的父目錄，不會偷偷向上搜尋 repo。

`ConfigStore::from_environment(&BTreeMap<String,String>)` 可由呼叫端明確提供 `PDF2ZH_APP_DIR`，並以同一 map 的 HOME／USERPROFILE 展開 `~` 與設定 legacy migration。library 不自行呼叫 `std::env::vars` 或發現使用者 credential store。`new()`／`for_current_executable()` 本身不啟用 home migration；可用 `with_legacy_home(absolute_home)` 明確指定。

`load_with_overrides(env, cli)` 的順序為：

1. 明確 CLI values。
2. 提供 map 內的 `PDF2ZH_*` environment values。
3. 上兩層指定的 `config_file`。
4. program-root 的 `config/config.v3.toml`。
5. model defaults。

CLI 呼叫端只能提供真正出現的選項，不能把 clap default 填入 override tree，否則會蓋掉檔案值。`parse_cli_overrides` 同時接受舊 flat field 名／kebab flags 與已 nested 的 section values。engine 的選擇先依高優先序找第一個 true flag，再做 deep merge，避免低優先序仍為 true 的其他 engine 反客為主；同一層多個 true 則保留 Python metadata 順序。term engine 另行選擇。dict 遞迴合併，array 依低到高延伸；input_files 轉成 string set 去重。

環境變數轉換保留 Python 的特點：bool 只有 true／1／yes／y／on 為 true，其他字串為 false；無法轉換的 int/float/set override 忽略，且不回顯值。input_files 接受 JSON array 與安全解析的 Python quoted list/set、None／set() 空集合，不執行 eval、shell 或任意程式碼。TOML 中的 nullable 與 environment 字串不可混為一談：`PDF2ZH_OPENAI_API_KEY=null` 是字串 `null`，不是儲存 codec 的 None。

`load()` 不自動 save 或補寫 default 檔；缺檔只在記憶體產生 defaults。malformed TOML／非法已知型別回傳安全錯誤並保留原檔，不沿用 Python「忽略壞檔」後可能被下一次自動儲存覆蓋的行為。

`save(&Settings)` 是明確的完整 document 寫入，不 mutate caller。`save_user_defaults()` 對 clone 清空 basic.input_files，再儲存，對應舊 GUI default-save 的暫存輸入處理。未知欄位保留；語法、排序與 comment 不保證逐 byte 保留。未知 TOML datetime／nonfinite float 使用原 TOML sidecar 保型，JSON accessor 將它們顯示為字串；變更為其他值後依新的 JSON 值序列化。

每次 writer 在目標同目錄建立唯一 tempfile，寫完 flush、`sync_all` 後才 persist replacement。Windows 使用 tempfile 的 replace-existing 路徑；短暫 5／32／33 錯誤最多嘗試五次，等待10／20／40／80ms，失敗絕不改成 truncate-in-place，並刪除私有 tempfile。Unix 另 sync 目標父目錄。多個 writer 仍是最後一份完整 document 勝出，沒有跨程序的逐欄位交易／merge 保證。

`migrate_legacy_once(home)` 只在目標不存在時，將 `home/.config/pdf2zh/config.v3.toml` 原 bytes 複製到同目錄 tempfile，sync 後用 persist_noclobber 發布；另一 writer／migrator 先建立的目標永遠不被覆寫。保留舊檔，也不更新、刪除或移動使用者的 legacy 目錄。migration 不驗證或重排原始 TOML；隨後 load 才解析，壞檔保留供修復。

## 舊 null 格式

`decode_toml_content`／`encode_toml_content` 精確保留 `storage.py` 的 dictionary-only 規則：object 值的字串 `"null"` 解為 null，null 編回字串 `"null"`；object 內的 object 遞迴處理，array 裡的字串或 object 不遞迴。

這個格式無法區分 table 值中的字面字串 `null` 與 None，是原有歧義。array 中若新加入 JSON null 而 TOML 無法表達，save 會報 Serialize 錯誤，不悄悄刪除或改成另一種值。

## Provider、term 與 PDF 轉換

| 設定選項 | Rust provider / 主要欄位 |
|---|---|
| OpenAI | `openai`：model/base_url/api_key；timeout 未給時600並標 timeout_explicit=false，顯式 float 則傳實值與 true。保留拼字 `openai_send_temprature`；只有對應 send flag true 才傳 temperature／reasoning_effort。 |
| OpenAICompatible、AliyunDashScope | 轉 `openai`；各自 prefix 的 URL/key/model/timeout/temperature/json mode。DashScope 預設500秒與0.0字串，不擅自改成 OpenAI 預設。 |
| DeepSeek、ModelScope、Zhipu、Gemini、Grok、Groq | 轉 `openai`，使用原 transform 的固定 endpoint、model/key/json mode；DeepSeek v4 thinking.type 與啟用時的 reasoning_effort 保留。 |
| AzureOpenAI | `azure-openai`：azure endpoint/deployment model/key/api_version；SDK 分層 retry 與 timeout 在 core。 |
| SiliconFlow、SiliconFlowFree | `siliconflow`／`siliconflowfree`，各自 json mode，前者 thinking 與 send flag。main Free 在 model validation 階段的初始 qps20 保留；prepare 後的推薦 qps/pool 由 core／bridge 處理。 |
| QwenMt | `qwen-mt`：qwenmt URL/key/model、ali_domains、lang_in/out；不當作支援 LLM 的 term engine。 |
| Ollama、Xinference | `ollama`／`xinference`：host/model；Ollama num_predict。未擅加 timeout。 |
| Google、Bing | `google`／`bing`，不要求 API key 或 model；原預設 endpoint 由 core 明確處理。 |
| DeepL、Azure | `deepl` key；`azure` endpoint/key。DeepL Free/Pro endpoint 由 core 按 key suffix 決定。 |
| AnythingLLM、Dify | 原完整 URL 與 apikey，不自行改寫 workspace 或 workflow path。 |
| TencentMechineTranslation | `tencent`：顯式 secret_id 放 options，secret_key 放 api_key；見下方 credential-chain 界限。保留舊設定名稱的 Mechine 拼字。 |
| CLITranslator、ClaudeCode | command/timeout/postprocess，或 path/model；只轉設定，不在 builder 啟動程序。prepare／retry／取消後清理仍由 core 負責。Python metadata 把這兩者視為非 LLM term engine，Rust 沿用該選擇能力。 |

term provider 有獨立的 `term_*` detail；轉 core config 時去掉 field 前綴，但不混入 main credential。term_qps 未給時繼承 main 的有效初始 qps，main 本身支援 LLM 且沒有明確 term 時沿用 main 設定。沒有可用 LLM term engine 時，PdfJob 關閉自動術語擷取。既有 glossary + 非 LLM main 的限制仍保留，即使另選 LLM term 也不能繞過。

PdfJob 使用確認過的 worker wire 名稱：

- `glossary_files` 是 CSV path 字串陣列，bridge canonicalize、worker 才用 `Glossary.from_csv` 載入成 engine `glossaries`；不得直接把 path array 當 BabelDOC Glossary objects。
- `max_pages_per_part` 由 worker 建 split strategy；0 與 None 不傳。
- `report_interval`、已允許的 PDF engine options、term_pool_max_workers 直接傳；term_qps 只由 Rust bridge 消費。
- no_merge_alternating_line_numbers／no_remove_non_formula_lines 反轉為 engine 的 merge_alternating_line_numbers／remove_non_formula_lines，確保 baseline 的兩項 default true。
- enhance_compatibility 同時啟用 skip_clean／disable_rich_text_translate；auto OCR 啟用時取消手動 OCR 與 skip_scanned_detection。
- ignore_cache 是 Rust translator option，不進 PdfJob extra。未知設定與 provider secrets 也不進 extra。

`tests/settings.rs::pdf_job_matches_the_actual_worker_whitelist_and_keeps_glossaries_as_typed_paths` 直接讀 repo 的 worker source，核對 ENGINE_OPTIONS／required 名單與 glossary path 型別，避免另抄一份過時白名單。

## 尚未完成的相容範圍

1. **Tencent 自動 credential chain 沒有移植。** 舊 translator 先呼叫 DefaultCredentialProvider，只有 OSError 才使用設定內的 id/key，因此即使明確設定也可能被 machine chain 優先。Rust 只使用明確欄位；缺任一欄位回 UnsupportedCredentialSource，不讀使用者 profile、系統 environment credential、雲端 metadata 或真 API。這是相容性缺口，不能稱成原 SDK chain 等價。
2. **SDK environment 僅使用顯式 map。** `PDF2ZH_*` override 已實作；選中的 OpenAI／SiliconFlow／Qwen 在 URL=None 時可從提供 map 的 OPENAI_BASE_URL fallback，AzureOpenAI 同樣接受 AZURE_OPENAI_ENDPOINT。它們只存在不序列化的 runtime sidecar，明確 URL 優先，固定 transform endpoint 不受影響；SDK-level URL 不重跑應用 clean_url。沒有提供 map 就不讀 machine environment，也沒有實作任意 SDK credential chain、所有 transport/proxy environment 或 HTTPX write/pool timer。
3. **rpc_doclayout 儲存但未連接 RPC。** 原 Python `build_babeldoc_config` 與現 worker 都固定 doc_layout_model=None；全專案該欄位只有 model／GUI／UI 寫入，沒有可觀察 engine 使用。Rust 保留 metadata、載入、覆寫與 roundtrip，不傳 unknown option 或宣稱 RPC layout 已可用。
4. **部分 validation 移到正確執行邊界。** input 是否存在／為 PDF 由 task／bridge 檢查；Python regex pattern 由 Python engine 使用處理，Rust 不用不支援 lookaround 的 regex 方言誤拒合法舊 pattern。CLI command quoting 由 core preflight 驗證。port/auth/warmup/offline-assets/GUI 的設定都保留，但本 crate 不代表這些產品流程已實作。
5. **新安全檢查保留可見差異。** 非有限 timeout/float 拒絕；主 pool 必須為正32-bit整數以符合已實作 bridge（舊 model 未明確拒絕0／負值）；term pool0沿 bridge 的跟隨 main 語意。AnythingLLM／Dify／AzureOpenAI 缺 endpoint 會在 builder 安全失敗，不等到 HTTP 呼叫。這些不冒稱原 Pydantic／SDK 所有錯誤完全相同。
6. **格式與自動 default snapshot。** 讀取不更新 `config/default/2.9.0.toml`，避免非明確操作寫檔；default metadata 可列舉，但舊版版本化 default 檔的產品輸出流程尚未接。save 保留資料與未知 TOML 型別，沒有完整 comment/format editor 或跨程序欄位交易。
7. **明確 Rust extension。** provider detail 的 `core_options` object 供已支援的 core 政策／headers；`core_endpoint` 可明確覆寫無舊 URL 欄位 provider 的 endpoint，便於離線測試。未知欄位不自動當成網路參數，兩項 extension 不宣稱 Python 原格式已識別。

## 測試證據

本階段所有 storage fixture 只用 TempDir／synthetic values；沒有讀取本機使用者的 config 或 credential，也沒有執行真 provider API／CLI。

| 測試 | 具體 oracle |
|---|---|
| `store::application_path_is_independent_of_cwd_and_no_default_is_written_on_load` | 真子測試程序切換到另含壞 config 的 CWD，仍讀 absolute app root；load 缺檔不建立設定。 |
| `store::legacy_config_is_copied_once_byte_for_byte_and_never_replaces_local_settings` | 精確原 bytes、一份舊檔保留、target 存在後忽略損壞 legacy。 |
| `store::racing_legacy_migrators_have_one_winner_and_leave_no_shared_tempfile` | 八 thread 同時遷移只一個 winner，目標完整、沒有私有 temp 遺留。 |
| `store::explicit_cli_environment_and_file_precedence_never_saves_runtime_secrets` | 四層真 TOML／map 合併、main 選擇與 credential 優先序、兩個來源檔均未被改寫。 |
| `store::supplied_sdk_url_fallbacks_apply_only_to_none_and_are_not_persisted` | 顯式 map 的 SDK URL fallback、main/term 獨立、明確 URL 優先；修改 model／語言後 sidecar 保留，SDK endpoint／無關 credential 不寫回 TOML。 |
| `store::null_codec_matches_dictionary_only_legacy_format_and_unknown_fields_roundtrip` | array null 字串不轉、unknown table/key/datetime/nan 保留，已知欄位可改後保存重讀。 |
| `store::malformed_or_invalid_configuration_is_not_replaced_or_echoed_in_errors` | 真 parser／型別錯誤不回顯 synthetic secret、不改原 bytes。 |
| `store::concurrent_atomic_saves_never_expose_partial_or_mixed_documents` | 四 writer 與 reader，marker/payload 必須來自同一完整 document；writer panic 可由 join 傳回，不用成功 counter 無限等待。 |
| `store::windows_replace_failure_preserves_old_file_and_cleans_its_private_tempfile` | 真 Windows handle 允許讀但拒 replace，save 失敗保留舊 bytes、清 temp；解鎖後成功。 |
| `store::user_default_save_clears_only_the_persisted_input_files_and_preserves_caller` | caller 保留 input，磁碟 clone 清 input，unknown field 不丟失。 |
| `settings::all_sixteen_core_providers_map_without_network_or_machine_credentials`／`all_eight_openai_aliases_keep_their_pinned_endpoints_models_and_options` | 每個 provider 實際 builder output 的 type、URL、model、options；DeepSeek v4 special body。 |
| 其餘 settings fixtures | 235欄 schema 的 flag/type/default/secret、Python bool/set 轉換、獨立 main/term 與單次選擇、timeout implicit/explicit、PDF baseline defaults、worker 白名單、safe Debug／typed errors。 |

主代理於2026-10-30完成統一驗收：完整 Rust workspace **167 passed、0 failed**，其中本 crate **22/22通過**（settings11、store11，包含 Windows replace failure 與真正切換 CWD 的子測試）；fmt check、clippy all-targets `-D warnings` 及 Worker26項測試均通過。Rust結果位於 `.test-output/rust-migration/phase6-core-cli-tests.log`，本文件已核對各suite結果行。glossary／worker白名單與 SDK sidecar 編輯後保留均在此通過快照內。

本子代理未另行執行 cargo 或修改 Cargo.lock；上述執行結果來自主代理統一驗收。測試通過不擴張上節列出的 credential-chain、RPC layout、GUI／assets產品流程、SDK timer 與格式編輯界限。
