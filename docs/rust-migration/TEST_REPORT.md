# 移植測試報告

更新：2026-10-30，Asia/Taipei。Python 穩定基準 `d18165e`，Rust 開發分支 `feature/rust-babeldoc-hybrid`。本報告驗收核心 PoC、第 5 階段翻譯服務及第 6 階段設定／快取／CLI／任務管理；不宣稱全面功能或所有 SDK 細節等價。

## 第 6 階段核心與 CLI 實測

| 檢查 | 結果 |
| --- | --- |
| Rust workspace | **167 passed，0 failed／ignored**：bridge 17、CLI 7、config 22、core 112、tasks 9 |
| 格式／Clippy | `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 通過 |
| Worker／Ruff | **26 passed**；worker 與 smoke 腳本 Ruff 通過。原 Python 全套 279 項沿用下列第 5 階段紀錄，未宣稱本輪重新執行 |
| Config | 優先序、24 服務選項、舊 TOML 保留、並行遷移／寫入、Windows 鎖檔原子保存、CWD 隔離；API key 以遮罩 Debug 與錯誤訊息處理 |
| Cache | 27 項 storage／service 測試，涵蓋真 SQLite／WAL、舊 JSON namespace、端點隔離、read-only、loopback HTTP 後重新開庫命中、busy／取消與無晚到 writer |
| Tasks／Bridge | 排隊取消、獨立並行批次、首檔失敗停止、shutdown 等待、未首次 poll 就關 runtime、取消與終態線性化；cache 清理 fixture 證明 grace=0 仍等待 cooperative cleanup |
| CLI process | help／version 不啟 Python、不建立設定；兩檔單一 terminal；獨立 main／term HTTP 設定與 usage；敏感錯誤不回顯 |
| 真實 BabelDOC PDF | exit 0；四頁原檔、105 個 loopback identity 請求、36 個 batch、68 個進度、1 個 finish |
| 與 Python main 比較 | mono／dual 各 4 頁；文字、逐字元座標、圖片一致；8 頁 96 DPI RGB 渲染像素差 0，見 [PHASE6_PDF_COMPARISON](PHASE6_PDF_COMPARISON.json) |

證據：`.test-output/rust-migration/phase6-core-cli-tests.log`、`phase6-worker-tests.xml`、`phase6-cli-full/summary.json` 與 `phase6-pdf-comparison.json`。測試隔離 HOME／USERPROFILE／TEMP／PDF2ZH_APP_DIR，不讀取使用者真實設定或憑證。GUI／PDFium 尚未包含在上述 167 項中，離線資產維護操作仍待實作。

## 第5階段實測

| 檢查 | 結果 |
| --- | --- |
| Rust完整workspace | **102 passed，0 failed／ignored**：core14、bridge framing3、process整合13、CLI1、legacy provider40、其餘provider31 |
| 格式／Clippy | cargo fmt --all --check、clippy --workspace --all-targets --locked -- -D warnings通過 |
| Python原功能與Worker | **279 passed**，分兩次執行：tests/與python-worker/tests共271（28.66秒），test/test_cache.py另8（2.81秒）；沒有跳過原先8個cache案例 |
| Worker新增邊界 | 26 passed，包含上列279內；角色、容量、glossary／split轉換、metric與native stdio隔離 |
| Rust provider | OpenAI-family SDK／outer分層、Free巢狀重試、Google／Bing、DeepL／Azure SDK、Tencent簽名、Ollama／Xinference、真CLI／Claude fixture；逐列證據及界限見PROVIDER_TEST_MATRIX |
| Redirect | Free不follow；requests服務30跳；Azure外層redirect／內層SDK counter、POST301/302、303後GET的budget及body error；跨origin307/308不轉送credential |
| Bridge | 200個同時在途RPC、獨立main／term、共享usage不重算、有界慢進度消費者；真正CLI取消後root／descendant固定HANDLE立即確認退出 |
| 指定四頁PDF | exit0；105個loopback請求、36個batch、68個進度、1個finish；無合成key外洩 |
| PDF與main比較 | mono／dual各4頁；文字、字元座標、圖片一致；8頁96 DPI RGB渲染像素差0，見PHASE5_PDF_COMPARISON.json |

Rust證據：`.test-output/rust-migration/phase5-verified-tests.log`。Python證據：`phase5-python-full.xml`、`phase5-python-cache.xml`；命令分別為隔離runner後接`pytest tests python-worker/tests -q`與`pytest test/test_cache.py -q`。所有HTTP驗證只用loopback，CLI只用現有rustc編譯的受控fixture，未啟動真Claude或使用付費／使用者憑證。

PDF證據位於`.test-output/rust-migration/phase5-full-recovered/`；原檔已移至`Downloads/論文`，SHA-256仍為`d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f`。先前原路徑不存在的`phase5-full-verified/`失敗輸出保留，沒有列為通過；smoke腳本現會在開始前檢查來源檔。新版PDF的少量二進位大小差異未造成文字／幾何／渲染差異。

本輪另修正兩個測試暴露的問題：HTML5複合entity不能只解碼第一個Unicode scalar；DeepL timeout依`max(deadline-now,10)`動態改變，第六次可為10.48576秒，不能用固定10秒的錯誤oracle。修正後維持精確sleep／attempt斷言。Rust test編譯早期缺Instant匯入亦已修正。原失敗log沒有覆寫成成功證據。

## 已提交PoC基準（0b33926）

| 檢查 | 結果 |
| --- | --- |
| 整合後 main Python 基準 | 253 passed；詳 GIT_INTEGRATION |
| 新 Worker 加原 Python 完整 pytest | **269 passed，20.39 秒，0 failed／skipped** |
| Rust unit tests | core 10、bridge framing 3，全部通過 |
| Rust process integration tests | 8 passed；含真正 Windows child／grandchild handle 退出驗證 |
| Rust Clippy／format、Python Ruff | 通過；Clippy 使用 all-targets 與 -D warnings |
| Rust CLI `--version` | `pdf2zh 2.9.0`，exit 0，不啟 Python |
| 指定 PDF debug 一頁 PoC | exit 0；31 loopback API requests、11 batch；mono／dual 皆產生 |
| 指定 PDF 全四頁 PoC | exit 0；105 loopback API requests、36 batch；mono／dual 皆產生 |
| 完整 PDF 與 main 比對 | 兩種輸出皆 4 頁；文字、字元座標、圖片相同；8 頁渲染像素差 0 |

Rust 指令：`cargo test --workspace --locked`。Python 指令：`.venv/Scripts/python.exe .test-output/rust-migration/run_isolated.py pytest -q`。Windows asyncio／loopback 使用允許本機通訊的執行環境，HOME／USERPROFILE／TEMP／設定目錄隔離；未使用付費或使用者憑證。

原始證據：`.test-output/rust-migration/rust-poc-tests.log`、`poc-python-full.xml`、`poc-private-stdin/`、`poc-full-verified/`。[PDF 比對數據](POC_PDF_COMPARISON.json)使用 Poppler 96 DPI RGB，以及 PyMuPDF 的文字、逐字元 bbox 與圖片 digest／bbox。人工檢視第二頁；既有公式附近的重疊仍與基準一致，未修改 BabelDOC 排版以掩蓋差異。

原 main fixture 共 107 次請求，Rust PoC 為 105 次；原 translator 工廠有忽略 cache 的 `translate("Hello")` health check（`translator/utils.py:73`），PoC 尚無完整初始化健康檢查。請求數差異不作 retry 等價證明。

## 已涵蓋的邊界

- UTF-8、多個 frame、分段 frame、select 中斷保留資料、超大／不完整／錯版本 JSON。
- 同步 callback 的逆序回覆、取消解除等待、Rust 錯誤阻止假成功、重複／未知 ID。
- Python／CRT／Win32 stdout 診斷隔離；標準 stdin EOF 與私有協定讀取分離；私有 fd 不可繼承。
- HTTP timeout、取消、QPS、空／無效回應、具／不具 Content-Length 的大小界限、錯誤不回顯合成 key。
- startup crash、exit0 無 terminal、合法 buffered finish、缺少要求輸出、錯 session／task、忽略取消的程序與持 pipe 孫程序。

## 失敗紀錄與修正

早期 Rust GNU linker 產物在 main 前發生存取違規，經最小 probe／LLDB 縮小後改用 Rust bundled `rust-lld`；之後 21 項 Rust 測試通過。早期 Python frame 參數測試產生過長 pytest ID，已用明確短 ID 修正。

第一次完整 PDF 與第一次一頁 PoC 都逾時，沒有列為通過。debug 堆疊顯示 NumPy 原生 import 期間停滯，標準 stdin 同時由協定 reader 阻塞讀取；隔離私有 stdin 後重新執行一頁、完整四頁及完整測試通過。舊失敗證據保留於測試目錄。

## 尚未驗收

原 34 項 Python retry 案例仍保留，Rust 現在已有對應 runtime／transport 證據，詳 PROVIDER_TEST_MATRIX；未把原 policy 宣告物件的每個負例重建成完整逐列矩陣，也未證明 HTTPX write／pool 逐階段 timeout 完全等價。GUI、預覽、資產維護、真實免費 API、長時間／大型／並行 PDF、乾淨 Windows、正式打包、安全與再散布授權仍需驗證。沒有將舊輪次數字當成新功能的測試結果。
