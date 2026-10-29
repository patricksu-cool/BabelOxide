# 混合架構的依賴、資源與 Windows 發行邊界

查核日期：2026-10-26。本表使用現有 `.venv` 的 `importlib.metadata` 與 `Requires-Dist`，未安裝、升級、移除或修改任何 BabelDOC 套件。Python 實際版本為 **3.13.16 / Windows AMD64**。BabelDOC 固定 **0.6.3**；PyMuPDF 固定 **1.26.7**；bitstring 固定 **4.4.0**。本文件是依賴盤點與分階段打包設計，不宣稱新的 Rust／Worker 發行包已完成。

## 盤點方法與可精簡範圍

由 `babeldoc` 與 `pdf2zh-next` 各自作為根，遞迴解析已安裝 distribution 的 `Requires-Dist`，按當前 Python／Windows marker 篩選，傳遞邊上的 extras（例如 `httpx[socks]`、`xsdata[cli,lxml,soap]`），以標準化 distribution name 去重。不使用 import 掃描冒充依賴解算，也不把所有開發環境套件都算入執行期。此次沒有啟用 BabelDOC 的 cuda／directml／memray extra。

- BabelDOC 必要宣告閉包：**85 個 distributions，包含 BabelDOC 本身**；本機均已安裝，沒有缺項。
- 舊 `pdf2zh-next` 必要執行期閉包：**134 個 distributions，包含應用自身**。
- 差集：**49 個 distributions（含 `pdf2zh-next`，即 48 個其他套件）**。這是新 Worker 環境可不宣告的候選集合，不代表現有環境已刪除它們，也不代表完整 GUI／服務移植已驗收。
- 這是保守的上游 metadata 閉包，不是經裁剪的最小 import 集；例如 BabelDOC 宣告 `xsdata[cli,lxml,soap]`，因此 `ruff` 也會保留。不能僅因套件名稱像開發工具就從發行包刪掉。

補充靜態 AST import 交叉檢查：安裝的 BabelDOC Python 檔案均可解析，但有不在主翻譯入口的額外工具路徑，例如 `tools/executor/babeldoc_adapter.py:10` 引用本機未安裝的 `pyarrow`，`pdfminer/ccitt.py:576–609` 的測試顯示入口引用 `pygame`。因此 Worker 直接使用 `format.pdf.high_level`，不改用上游 tools/executor adapter；placeholder 只是參考該 translator 的明確協定。此盤點沒有宣稱任意匯入上游所有工具模組均可成功。

建議建立獨立 Worker 鎖定環境，以 `babeldoc==0.6.3` 加上既有相容 pin `pymupdf==1.26.7`、`bitstring==4.4.0` 為根，將下列已驗證版本鎖定；本階段不重新解算成更高版本。root `pyproject.toml` 的完整應用依賴維持作為 Python baseline；不能讓新 Worker builder 仍對整個 root project `uv sync`，再聲稱已去除 Gradio。新的鎖檔、乾淨環境 `uv pip check` 與 Worker smoke test 是實際精簡打包的驗收，尚未由本盤點替代。

## 舊直接依賴對照

「保留」指 BabelDOC 宣告閉包仍需要此 distribution；即使其應用責任已移入 Rust，也不代表可從 Python wheel 安裝中移除。「移出 Worker」指不在此閉包，對應功能在 Rust 或後續 GUI 實作，沒有把舊 Python 模組搬入 Worker。

| 舊直接依賴 | 本機版本 | Worker 閉包 | Rust／引擎責任 |
|---|---|---|---|
| azure-ai-translation-text | 1.0.1 | 移出 Worker | Rust Azure provider；完整服務移植另驗收 |
| babeldoc | 0.6.3 | 保留 | 保留 PDF 解析、版面、公式、排版與輸出 |
| bitstring | 4.4.0 | 保留 | 引擎 parser；4.4.0 相容 pin，不能換 5.x |
| chardet | 7.7.0 | 保留 | 引擎編碼處理 |
| deepl | 1.32.0 | 移出 Worker | Rust DeepL provider；完整服務移植另驗收 |
| fastapi | 0.161.1 | 移出 Worker | 舊 Gradio web stack；Rust 原生 GUI 不依賴此 server |
| fonttools | 4.67.0 | 移出 Worker | 舊應用直接依賴；BabelDOC metadata 閉包未要求 |
| gradio | 6.30.0 | 移出 Worker | app 使用 eframe；完整 UI 行為仍須驗收 |
| gradio-i18n | 0.3.4 | 移出 Worker | Rust UI 的翻譯資源／語系映射另移植 |
| httpx | 0.28.1 | 保留 | Rust 翻譯 HTTP 用 reqwest；BabelDOC 資產等仍需要 HTTPX |
| legacy-cgi | 2.6.4 | 移出 Worker | 舊 Python 3.13 web stack 相容層 |
| numpy | 2.5.4 | 保留 | 引擎 ONNX／影像／數值運算 |
| ollama | 0.6.3 | 移出 Worker | Rust Ollama provider；完整服務移植另驗收 |
| openai | 3.36.1 | 保留 | Rust 呼叫 API；BabelDOC translator 模組 import／metadata 仍需要 SDK |
| peewee | 4.5.3 | 保留 | Rust 擁有翻譯 cache；BabelDOC cache 模組 import／metadata 仍需要 Peewee |
| pillow | 12.3.0 | 保留 | BabelDOC → scikit-image／imageio；移除 Gradio 不會移除 Pillow |
| pydantic | 2.11.10 | 保留 | Rust config 不使用它，但 BabelDOC／OpenAI 閉包仍要求 |
| pydantic-settings | 2.16.1 | 移出 Worker | Rust config 負責來源優先序、validation、命名映射 |
| pymupdf | 1.26.7 | 保留 | 引擎 PDF 讀寫與渲染；維持 1.26.7 |
| pyyaml | 6.0.3 | 保留 | Rust 設定另處理；BabelDOC → huggingface-hub 間接仍要求 |
| requests | 2.34.2 | 保留 | Rust 翻譯 HTTP 用 reqwest；tiktoken／xsdata 間接仍要求 |
| rich | 15.0.0 | 保留 | 引擎診斷與工具；worker stdout 不能輸出 rich 進度 |
| sse-starlette | 3.5.0 | 移出 Worker | 舊 web stack；stdio bridge 不使用 SSE |
| starlette | 1.9.0 | 移出 Worker | 舊 web stack；不帶入 Worker 根依賴 |
| tenacity | 9.2.1 | 保留 | Rust 擁有應用重試；BabelDOC import／metadata 仍要求 |
| tencentcloud-sdk-python-tmt | 3.1.121 | 移出 Worker | Rust Tencent provider；完整服務移植另驗收 |
| tqdm | 4.70.1 | 保留 | 引擎進度實作仍引用；Rust 顯示 IPC 進度 |
| uvicorn | 0.54.0 | 移出 Worker | 舊 Gradio HTTP server；Rust 原生 GUI 不需要 |
| xinference-client | 4.0.0 | 移出 Worker | Rust Xinference provider；完整服務移植另驗收 |

## BabelDOC 宣告閉包的實際版本（85）

來源為 `.venv/Lib/site-packages/babeldoc-0.6.3.dist-info/METADATA` 及各相依 distribution 的 metadata。「由誰引入」只列本次必要閉包內的直接父節點，不是完整開發環境所有反向依賴。以下版本是本機查核值，不是新發行環境已完成驗證的承諾。

| Distribution | 實際版本 | 閉包內由誰引入 |
|---|---|---|
| annotated-types | 0.8.0 | pydantic |
| anyio | 4.15.1 | httpx, httpx2, openai |
| babeldoc | 0.6.3 | 根套件 |
| backports-zstd | 1.8.0 | pyzstd |
| bitarray | 3.12.3 | bitstring |
| bitstring | 4.4.0 | babeldoc |
| certifi | 2026.10.19 | httpcore, httpx, requests |
| cffi | 2.1.1 | cryptography |
| chardet | 7.7.0 | babeldoc |
| charset-normalizer | 3.5.2 | babeldoc, requests |
| click | 8.5.0 | huggingface-hub, xsdata |
| cloudpickle | 3.1.2 | joblib |
| colorama | 0.4.6 | tqdm |
| configargparse | 1.8.0 | babeldoc |
| cryptography | 50.0.2 | babeldoc |
| filelock | 4.2.4 | huggingface-hub |
| flatbuffers | 25.12.19 | onnxruntime |
| freetype-py | 2.5.1 | babeldoc |
| fsspec | 2026.9.0 | huggingface-hub |
| h11 | 0.16.0 | httpcore, httpcore2 |
| hf-xet | 1.7.0 | huggingface-hub |
| httpcore | 1.0.9 | httpx |
| httpcore2 | 2.16.0 | httpx2 |
| httpx | 0.28.1 | babeldoc |
| httpx2 | 2.16.0 | huggingface-hub, openai |
| huggingface-hub | 2.2.0 | babeldoc |
| hyperscan | 0.9.1 | babeldoc |
| idna | 3.20 | anyio, httpx, httpx2, requests |
| imageio | 2.38.1 | scikit-image |
| jinja2 | 3.1.6 | xsdata |
| jiter | 0.17.0 | openai |
| joblib | 1.6.0 | scikit-learn |
| lazy-loader | 0.6 | scikit-image |
| levenshtein | 0.27.5 | babeldoc |
| lxml | 6.1.3 | xsdata |
| markdown-it-py | 4.2.0 | rich |
| markupsafe | 3.0.4 | jinja2 |
| mdurl | 0.1.2 | markdown-it-py |
| ml-dtypes | 0.6.0 | onnx |
| msgpack | 1.2.3 | babeldoc |
| narwhals | 2.27.1 | scikit-learn |
| networkx | 3.7 | scikit-image |
| numpy | 2.5.4 | babeldoc, imageio, ml-dtypes, onnx, onnxruntime, opencv-python-headless, scikit-image, scikit-learn, scipy, tifffile |
| onnx | 1.23.2 | babeldoc |
| onnxruntime | 1.31.0 | babeldoc |
| openai | 3.36.1 | babeldoc |
| opencv-python-headless | 5.0.0.93 | babeldoc |
| orjson | 3.13.0 | babeldoc |
| packaging | 26.3 | huggingface-hub, lazy-loader, onnxruntime, scikit-image |
| peewee | 4.5.3 | babeldoc |
| pillow | 12.3.0 | imageio, scikit-image |
| protobuf | 7.36.2 | onnx, onnxruntime |
| psutil | 7.2.2 | babeldoc |
| pycparser | 3.11 | cffi |
| pydantic | 2.11.10 | babeldoc, openai |
| pydantic-core | 2.33.2 | pydantic |
| pygments | 2.21.0 | rich |
| pymupdf | 1.26.7 | babeldoc |
| pyyaml | 6.0.3 | huggingface-hub |
| pyzstd | 0.20.0 | babeldoc |
| rapidfuzz | 3.14.6 | levenshtein |
| regex | 2026.9.29 | tiktoken |
| requests | 2.34.2 | tiktoken, xsdata |
| rich | 15.0.0 | babeldoc |
| rtree | 1.4.1 | babeldoc |
| ruff | 0.17.1 | xsdata |
| scikit-image | 0.26.0 | babeldoc |
| scikit-learn | 1.9.1 | babeldoc |
| scipy | 1.18.1 | babeldoc, scikit-image, scikit-learn |
| sniffio | 1.3.1 | openai |
| socksio | 1.0.0 | httpx |
| tenacity | 9.2.1 | babeldoc |
| threadpoolctl | 3.7.0 | scikit-learn |
| tibs | 0.5.7 | bitstring |
| tifffile | 2026.10.15 | scikit-image |
| tiktoken | 0.14.0 | babeldoc |
| toml | 0.10.2 | babeldoc |
| toposort | 1.10 | xsdata |
| tqdm | 4.70.1 | babeldoc, huggingface-hub |
| truststore | 0.10.5 | httpcore2, httpx2 |
| typing-extensions | 4.16.0 | anyio, huggingface-hub, onnx, openai, pydantic, pydantic-core, typing-inspection, xsdata |
| typing-inspection | 0.4.4 | pydantic |
| uharfbuzz | 0.56.3 | babeldoc |
| urllib3 | 2.8.0 | requests |
| xsdata | 26.10 | babeldoc |

## 舊應用閉包的差集（49）

以下不在 BabelDOC 必要宣告閉包。差集不是 uninstall 指令；本工作沒有移除既有環境套件。GUI、設定、Preview、下載、ZIP 匯出與每個翻譯服務仍要在 Rust 上逐項驗收。

| Distribution | 現有版本 |
|---|---|
| aiohappyeyeballs | 2.7.1 |
| aiohttp | 3.14.5 |
| aiosignal | 1.4.0 |
| annotated-doc | 0.0.5 |
| attrs | 26.1.0 |
| audioop-lts | 0.2.2 |
| azure-ai-translation-text | 1.0.1 |
| azure-core | 1.41.0 |
| brotli | 1.2.1 |
| deepl | 1.32.0 |
| fastapi | 0.161.1 |
| fonttools | 4.67.0 |
| frozenlist | 1.8.0 |
| gradio | 6.30.0 |
| gradio-client | 2.7.2 |
| gradio-i18n | 0.3.4 |
| groovy | 0.1.2 |
| hf-gradio | 0.4.1 |
| isodate | 0.7.2 |
| langcodes | 3.4.1 |
| language-data | 1.4.0 |
| legacy-cgi | 2.6.4 |
| marisa-trie | 1.4.1 |
| multidict | 7.1.0 |
| ollama | 0.6.3 |
| opentelemetry-api | 1.45.1 |
| pandas | 2.3.3 |
| pdf2zh-next | 2.9.0 |
| propcache | 0.5.4 |
| pydantic-settings | 2.16.1 |
| pydub | 0.25.1 |
| python-dateutil | 3.0.0 |
| python-dotenv | 1.2.4 |
| python-multipart | 0.0.32 |
| pytz | 2026.5 |
| safehttpx | 0.1.7 |
| semantic-version | 2.10.0 |
| shellingham | 1.5.4 |
| six | 1.17.0 |
| sse-starlette | 3.5.0 |
| starlette | 1.9.0 |
| tencentcloud-sdk-python-common | 3.1.192 |
| tencentcloud-sdk-python-tmt | 3.1.121 |
| tomlkit | 0.13.3 |
| typer | 0.27.3 |
| tzdata | 2025.3 |
| uvicorn | 0.54.0 |
| xinference-client | 4.0.0 |
| yarl | 1.25.1 |

## Rust 依賴選擇

工作空間採 `core`、`babeldoc-bridge`、`cli`、`app` 四個 crate；下表是分階段需求，不把未實作模組列成完成。精確 Rust 版本與 transitive closure 應以實際 `Cargo.lock` 為準，本文件不猜測 crates.io 最新版本。

| 外部 crate／標準庫 | 使用目的 | 範圍控制 |
|---|---|---|
| `serde`、`serde_json` | 強型別設定、IPC、API JSON | 共享 message／config model；明確 protocol version |
| `tokio` | process／stdio／非同步 HTTP 協調、deadline／取消 | 只開所需 runtime、process、io、sync、time features |
| `reqwest` | 各HTTP provider共用transport與cookie | 明確TLS／timeout／redirect policy；停用內建retry，provider控制各自SDK層 |
| `clap` | CLI flags／help／互斥條件 | 不把 credentials 寫入 help、log 或 worker |
| `rusqlite` | Rust 持久翻譯 cache | schema、transaction、busy timeout、成功結果規則；Windows SQLite 配置明確鎖定 |
| `hmac`、`sha2` | Tencent TC3簽名；後續cache／檔案身分 | 已用獨立Python stdlib oracle對照TC3；不把hash當作憑證遮蔽 |
| `httpdate` | SDK Retry-After日期 | 可注入wall clock，與數值／ms優先順序分開驗證 |
| `regex`、`html-escape`、`unicode-general-category` | 原prompt／Bing解析、Google HTML5 entity、Python whitespace語意 | HTML5複合entity補足93組多scalar映射；來源為Python標準庫HTML5資料，發行盤點需保留相關來源聲明 |
| `eframe` | app 原生 GUI | 後續階段；其 native 系統／圖形依賴需在新機驗收 |
| Rust `std` | path、同步基本元件、錯誤／程序資訊 | 不為可由標準庫處理的小需求任意增加 crate |

不要把「Rust取代SDK」解釋成所有服務只需同一份OpenAI JSON。Azure、DeepL、Tencent、Ollama、Xinference等認證、端點與結果格式不同；第5階段已分別移植16種service並以mock／真loopback／受控CLI驗證。網路重試、速率與取消政策集中在core；Python adapter不實作第二套。精確剩餘差異見PROVIDER_TEST_MATRIX，不以離線測試取代真API與發行驗收。

## 引擎資源與本機只讀查核

BabelDOC 不只有 Python wheels。`babeldoc/assets/embedding_assets_metadata.py` 列出 ONNX 模型（行 3）、tokenizer cache（行 7）、字型（行 39 起）與 CMap（行 536 起）。`assets/assets.py:465–498` 形成資產 manifest，`verify_file` 用 SHA3-256；`generate_offline_assets_package`／`restore_offline_assets_package` 有官方 ZIP 與 manifest tag 流程（同檔 501–599）。這些函式可能讀寫 cache 或下載，不能把呼叫 `init()` 當離線資產驗證。

2026-10-26 直接讀取本機 `~/.cache/babeldoc`，未下載／改動，逐一按安裝 metadata 的 SHA3-256 核對：

| 資源 | manifest 應有 | 本機有 | 已有檔案 bytes | hash 不符 |
|---|---:|---:|---:|---:|
| fonts | 38 | 34 | 265,807,784 | 0 |
| cmap | 146 | 146 | 7,705,262 | 0 |
| tiktoken | 1 | 1 | 3,613,922 | 0 |
| models | 1 | 1 | 75,324,598 | 0 |
| 合計 | 186 | 182 | 352,451,566 | 0 |

缺少 `LXGWWenKai-Regular.1.520.ttf`、`LXGWWenKaiGB-Regular.ttf`、`LXGWWenKaiMonoTC-Regular.ttf`、`LXGWWenKaiTC-Regular.ttf`。指定四頁 PDF 可用現有字型完成，不代表完整離線資產包已備妥，更不代表其他字型選項在離線新機可用。不能將開發者的整份 cache 當作乾淨交付。

`babeldoc/const.py:11` 將 cache 根固定為 `Path.home() / ".cache" / "babeldoc"`；`42–44` 在 import 時建立 tokenizer 目錄並覆寫 `TIKTOKEN_CACHE_DIR`。僅設定 `XDG_CACHE_HOME` 不能改變此路徑。在不改上游的前提下，Windows 選擇正常的 per-user cache 或明確、僅對 worker 子程序生效的 profile 環境；後者須在匯入前設定並另驗證 Path.home 與資產還原行為，不能全域改系統環境。

`TranslationConfig(..., doc_layout_model=None)` 初始化時就載入模型（`format/pdf/translation_config.py:308–312`）。啟動期間也應回報「初始化引擎」狀態並允許有界取消，而不是直到第一個 translation callback 才開始計時。資產下載是上游引擎責任，不與 Rust 翻譯 API 限流混算。

## Windows CPU／DirectML 的實際邊界

本機 `.venv` 安裝 `onnxruntime==1.31.0`，**沒有 `onnxruntime-directml` distribution**。舊 `script/build_windows.ps1:43–69` 的 DirectML extra 會保留 CPU 與 DML 兩份 metadata，且因 wheel 共用 `onnxruntime/`，特別將鎖定的 DirectML wheel 最後 staging 並檢查 provider。這是舊發行 profile，不能算作此 Worker PoC 的 DML 驗收。

更關鍵的是，已安裝 BabelDOC 0.6.3 的 `docvision/doclayout.py:51–77` 在非 macOS 分支只選 CPU provider，原始碼註解明確排除 DML／CUDA。**DmlExecutionProvider 可被列出，不等於此引擎使用它執行推論。** 本次保留 CPU 基準；不修改安裝包來打開加速，也不把有 GPU／DML wheel 寫成性能改善。

## 發行需保留的非 Python 內容

| 內容 | 必要理由／處置 |
|---|---|
| 相符架構與版本的 embedded CPython runtime、標準庫與 DLL | Rust GUI／CLI 啟動 worker 仍需 Python；不依賴使用者系統 Python／PATH |
| native wheels 的 `.pyd`、DLL 與套件資料 | PyMuPDF、ONNX Runtime、NumPy／SciPy、OpenCV、freetype／HarfBuzz、Hyperscan 等不只是 `.py`；依 wheel 資源完整 staging |
| dist-info METADATA、RECORD、license／notices | 版本盤點與再散布 provenance；不能為精簡而刪光授權 metadata |
| BabelDOC 字型／模型／CMap／tokenizer 資產 | 可選官方離線 ZIP 或明確首次下載流程；前者需完整 hash，後者不能標「完全離線」 |
| Rust exe、Python worker、有限設定範本 | Worker 不含 API key；範本不得夾帶真實 config、cache、PDF 或帳戶資料 |
| manifest 與 BUILD_INFO | 記錄 source commit、Python、BabelDOC、鎖檔 hash、每檔 hash、CPU／DirectML profile；保留可重現基準 |

舊 `script/package_windows.py:19–39` staging 會排除 tests、bytecode、virtualenv hooks 與 direct_url，並保留其他套件資源；`42–89` 拒絕典型使用者狀態資料夾並產生第三方清單／逐檔 SHA256。可沿用這些原則，但新依賴來源必須是獨立 Worker 環境。舊 `script/_pystand_static.int:11–15,41` 直接載入舊 app，因此不可原封不動當作新 launcher。

這次沒有產出或宣稱通過新的乾淨 Windows 安裝包；新機沒有開發 `.venv`、沒有既有 cache、路徑含中文與空白、無 console 視窗、EOF／取消／crash 回收等條件仍要獨立驗收。移除 Gradio 可省去其 web runtime，但不會讓 BabelDOC 的 native／字型資源需求消失。

## Pillow 與既有安全限制

本機 Pillow 仍為 **12.3.0**。它除了舊 Gradio 以外，也在 `babeldoc → scikit-image → pillow` 與 `babeldoc → scikit-image → imageio → pillow` 的必要宣告閉包。BabelDOC 掃描偵測引用 `skimage.metrics`（`format/pdf/document_il/midend/detect_scanned_file.py:7`），其 pdfminer image 模組另有 PIL 路徑（`pdfminer/image.py:150–151,172,240–241`）；這些靜態存在不等於已證明每份 PDF 可達每項漏洞，但更不能宣稱 Rust GUI 使 Pillow 問題全部消失。

既有 [第四輪 Pillow 報告](../../reports/round4/02_pillow_security.md) 與 [Windows 發行驗證](../../reports/round4/06_windows_release.md) 記錄當時 Windows 安裝版本符合 41 項 Pillow 公告，另六項 filelock 公告屬鎖檔的 Python 3.10 分支。本次盤點沒有重新上網更新 advisory，也沒有修補或升級 Pillow，因此保留原未解限制。移除 Gradio 可以解除其 `<13` 限制，但是否升級 Pillow 是另一個需獨立驗證上游相容性與支援 Python 版本的變更，不在本次固定版本移植中偷渡。

## 證據範圍

版本數與父依賴表均由當前 installed metadata 計算；沒有把 `uv.lock` 的所有平台／Python 分支數當作此 Windows runtime 數量。BabelDOC 的 13 個關鍵引擎／translator／進度／資產來源檔已與 installed RECORD hash 比對，全部相符。Codebase-memory 已確認 `.venv` 為刻意排除範圍，因此此部分使用直接來源與 metadata，不虛構外部套件圖譜覆蓋。舊打包腳本與報告來源已有 coverage 查核；`_pystand_static.int` 的 partial 解析範圍已直接讀取。
