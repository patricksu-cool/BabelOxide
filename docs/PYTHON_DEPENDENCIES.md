# Python 保留範圍與依賴

更新：2026-10-31，Windows x64、CPython 3.13.16、BabelDOC 0.6.3。原 Python App、GUI、CLI、設定／translator／API／cache／tasks／retry 模組已移除。Worker 只做 BabelDOC 初始化、引擎選項轉換、翻譯回呼 IPC、進度、取消與資源清理；沒有自己的 provider HTTP 或重試政策。框架、路徑與容量檢查屬於必要的跨語言邊界。

執行期保留 85 個確切 wheel，與已驗證 v3 profile 的名稱、版本、URL、大小與 SHA256 相同，包括 Pillow 13.0.0。`python-worker/baseline-wheels.json`、`wheel-overrides.json` 與 `requirements-win64.lock.txt` 是固定來源；metadata 父相依位於 [依賴資料](../python-worker/dependencies-win64.json)。根目錄 uv 專案只描述 pytest／Ruff 開發工具，不能代替執行期 profile。

BabelDOC 的套件宣告閉包包含 OpenAI、Peewee、Tenacity、Pillow、ONNX Runtime、xsdata extras 及 Ruff 等。名稱像應用程式或開發工具，不代表可從該閉包刪除。以下列出每個保留項目的直接相依來源，並不表示每次 PDF 都會 import 所有套件。模型、字型、CMap、tokenizer、Python 標準函式庫與 native DLL 另外保留。

本機 builder 以已驗證的離線輸入組裝；缺 wheel／資產或 hash 不符會停止。Worker 32 項邊界測試、隔離 installed metadata 閉包及真 PDF 回歸分別驗證不同風險。開發腳本與 fake worker 是測試工具，不隨執行包發布。歷史原碼可從 Git 基準回復，無需保留第二套正式 App。
| Distribution | Effective version | Required by (metadata closure) |
|---|---|---|
| annotated-types | 0.8.0 | pydantic |
| anyio | 4.15.1 | httpx, httpx2, openai |
| babeldoc | 0.6.3 | Worker root |
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
| pillow | 13.0.0 | imageio, scikit-image |
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
