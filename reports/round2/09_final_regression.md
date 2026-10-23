# 第二輪第九階段：最終回歸驗證（2026-10-23）

產品基準：`8ac31f3`，分支 `maintenance-round2`。已完成安全修補及逐模組重構後才執行本階段。所有翻譯請求使用離線 Mock 或 `127.0.0.1` 的 OpenAI 相容 identity server；沒有讀取真實 API Key 或呼叫付費翻譯服務。

## 1. 環境與自動化測試

專案預設 `.venv` 已用 `uv sync --locked` 同步到正式鎖檔，取代本輪開始時的 BabelDOC 0.6.2／PyMuPDF 1.25.2／GitPython 3.1.62。現在為 BabelDOC **0.6.3**、PyMuPDF **1.26.7**、GitPython **3.2.1**。Windows Python 3.13.16、uv 0.13.1；`uv pip check` 檢查 **181 個已安裝套件**，相容性通過。

測試使用工作區內隔離的 USERPROFILE／TEMP／app 路徑，執行：

```powershell
.venv/Scripts/python.exe -m pytest -q --junitxml=.test-output/round2/stage9-pytest.xml -o faulthandler_timeout=30
```

**212 passed、67 warnings、0 failed、0 skipped／xfail，20.04 秒。** JUnit 與完整日誌保存在忽略的 `.test-output/round2/`。

| 測試群組（依 JUnit 檔分類） | 通過數 | 重點 |
|---|---:|---|
| Config | 69 | 建立、讀寫、修改、重載、遷移不覆寫、路徑、TOML 格式、寫入失敗與並行原子替換 |
| GUI | 52 | 原有服務欄位、上傳／清除狀態、回傳順序、取消、設定保存模式、Rate 選項及套件資源 |
| Retry／Backoff | 34 | 服務別策略、429／5xx、Retry-After、等待、逾時、例外、QPS、取消與資源釋放 |
| 翻譯整合 adapters | 18 | 設定映射、主／術語翻譯器共用、表格模型、glossary、Token 完成事件及關閉 pipe／queue |
| CLI 退出碼 | 7 | 成功、部分失敗、完全失敗、既有 GUI 分支與例外路徑 |
| 日誌與 PyStand 遮罩 | 13 | 合成 API Key、敏感欄位、Exception、早期 launcher 失敗 |
| 依賴與服務功能 | 19 | 安全版本、bitstring／Tencent 相容性、DeepSeek 既有行為 |

警告包含 Pydantic instance `model_fields`、websockets／套件 API 棄用，維持重構前 67 項；沒有隱藏為失敗或刪除測試。

## 2. 指定四頁 PDF

輸入：`C:\Users\user\Downloads\High_speed_buffer_latch_ISCAS03.pdf`。統一目標語言 `zh-CN`、無浮水印、不自動擷取 glossary、忽略翻譯快取，使用本機 identity API。

| 執行 | Exit | 本機請求／批次 | 單語／雙語頁數 | 日誌含合成金鑰 |
|---|---:|---:|---:|---|
| Python 非 Debug | 0 | 107／36 | 4／4 | 否 |
| Python Debug | 0 | 107／36 | 4／4 | 否 |
| 新 Windows EXE 非 Debug | 0 | 107／36 | 4／4 | 否 |

非 Debug 的 Python／EXE 輸出逐頁文字 SHA-256、字元／公式符號／嵌入圖片指標，與第四階段且已在第七階段確認的重構前基準相同。單語文字 **54,544** 字元、公式符號 **48**、圖片記錄 **23,998**；雙語 **150,543／98／47,996**。圖片數為 Poppler 的記錄數，包含重複嵌入，不代表該論文有這麼多獨立圖表。

用 Poppler 50 DPI 渲染兩種輸出各四頁，Python＋EXE 共 **16／16 頁逐像素相同**；檔案雜湊仍可能因 PDF metadata 不同而不同。比對資料見 `09_pdf_metrics.json`、`09_pdf_comparison.json` 及四份 `09_*_render.json`。

Debug 單語／雙語擷取字元 **507,667／2,108,005**，公式符號 **48／98**、圖片 **23,998／47,996**，均可開啟與渲染。相較第一輪／第二輪早期的 Google identity stub，這次使用本機 OpenAI API，且 Debug 含不穩定標記；沒有將 Debug 字元或渲染當成逐位元組相同。另產生的 decompressed PDF 是 Debug 產物，未混入兩份正式輸出的計數。

150 DPI 目視檢查第 2 頁：電路圖和公式仍在，正文可讀；公式附近、局部斷字與重疊和原基準相同。這是已存在的 BabelDOC／identity 輸出版面問題，**未修復，不能宣稱排版完全正確**。真實中文譯文品質未驗證。

## 3. Python GUI 與 EXE GUI

兩者均經真實 Gradio HTTP 事件完成：上傳一頁測試 PDF → 保存設定 → 載入 191 個控制值 → 翻譯 → 下載單語／雙語各一頁及含兩份 PDF 的 ZIP。每次 **3 個本機請求、1 個批次**；PDF 可用 PyMuPDF 開啟，ZIP 成員數為 2。

- Python GUI 首頁 HTTP 200，646,920 bytes。
- EXE GUI 首頁 HTTP 200，646,969 bytes；瀏覽器顯示上傳、語言、翻譯、取消和預覽區，沒有空白頁。
- EXE GUI 的重新載入另外核對 OpenAI、測試模型及 loopback URL 值，沒有輸出金鑰。第一次測試直接把 Gradio `update` 字典當字串比較而失敗；修正測試取出 `value` 後通過，產品程式未改動。
- 以 embedded runtime 加入打包的 `site-packages`，在測試頁預載 PDF；瀏覽器可見正文、頁碼、清除與翻頁控制，兩個測試頁的 error console 均為空。首次直接執行 embedded Python 未加入 PyStand 的套件路徑，得到 `ModuleNotFoundError: gradio`；明確套用同一套件路徑後通過。此預覽頁是打包元件測試，EXE 主 GUI 本身另有上述完整 HTTP 流程。
- 已停止本輪 GUI／預覽行程，關閉測試分頁，確認 17864／17866／17867 沒有測試 listener。EXE 測試包中的合成設定已移除並重新產生預設設定；未留下合成憑證。

## 4. CLI 與 EXE 路徑

| 實際子行程情境 | Exit | 產生 PDF |
|---|---:|---:|
| Python 全部成功 | 0 | 2 |
| Python 成功＋損壞 PDF | 1 | 2 |
| Python 全部失敗 | 1 | 0 |
| EXE 損壞 PDF | 1 | 0 |

三個 Python 情境共 12 次本機請求，日誌均不含合成金鑰。新 EXE 從其他工作目錄啟動 `--version` 回傳 0／2.9.0，設定建立在 **EXE 旁 `config/default/2.9.0.toml`**，工作目錄沒有 config，也未遷移舊使用者設定。清除合成測試設定後再次 `--version` 仍通過。

EXE 由全新、非 editable、no-dev 的 136 套件環境重新組成；沿用官方 PyStand 1.1.4／embedded Python 3.13.3。本機建置環境是 Python 3.13.16。新 wheel 資源逐位元組驗證、鎖定安裝及 PowerShell／YAML 檢查見 `08g_locked_windows_build.md`。

## 5. Retry、靜態與安全檢查

- 最終完整測試中的 Retry 子集 **34 passed**。以 Git 比較 `63074ab`，`pdf2zh_next/translator/` 及 `utils/asynchronize.py` **零差異**；跨行程、Timeout、取消及例外類別的 AST 也未被本次拆分改寫。
- `ruff check .`、18 個新／拆分模組與相關測試的 formatter、`git diff --check` 通過。沒有刪除測試或放寬 lint 設定。
- Python 範圍 `ruff format --check pdf2zh_next tests script` **失敗**：7 個既有檔案仍需格式化；相較第七階段少了已拆分的 GUI 問題。包含 high_level 的既有空行、兩個 CLI translator 的日誌換行、比較腳本及三個既有測試。全根目錄格式檢查另納入文件 Python 區塊，共 30 檔未符合。這不是已通過的 gate。
- 最終完整鎖檔稽核：214 套件、98 原始記錄、**68 個去重公告**；runtime：164 套件、同樣 98／68。兩次 `uv audit` 均 **exit 1**，無查詢失敗狀態。Gradio 7、Pillow 56、Starlette 5；BabelDOC／GitPython 無本次公告。詳見 `09_dependency_audit.json` 及第六階段的可達性／相容性分析。
- `config/`、`.venv/`、`.test-output/` 沒有被 Git 追蹤；原 AGPL 授權檔與上一輪基準相同。

## 6. 未執行與既有失敗

1. 真實翻譯 API／真實憑證／付費服務、所有供應商 SDK 的內建 Retry 與網路配額：未執行。
2. 完整 GitHub Actions 發行、重新取得 embedded runtime、離線資源包產生與刪除後下載、其他 Windows 電腦及其他作業系統：未執行。不能把本機 EXE 重組寫成完整 CI 發行成功。
3. 真實 30 分鐘 Timeout、網路壓力、Windows 翻譯行程中途硬取消的長時間測試：未執行；本專案錯誤／取消與資源釋放規則以可控制 Mock 驗證。
4. 文件一般建置在第六階段修改前後都因 `partials/languages/zh_TW.html` 缺失失敗，strict 另有斷鏈。文件實作未更動，本階段未重複建置，也沒有當成通過。
5. 68 項版本命中公告、既有公式重疊、上述格式與棄用警告仍存在。未發現本輪重構引入的必要功能測試失敗。

相較第一輪 **139 passed**、第二輪重構前 **160 passed**，最終為 **212 passed**。本輪實際執行範圍內的啟動、Config、GUI、CLI、指定 PDF、EXE、日誌與 Retry 驗證通過；安全稽核及未執行範圍維持上述明確限制。
