# v2.9.0 修改前測試基準（2026-10-22，Windows）

## 來源與環境

- 未修改原始碼快照：`277d1f1`。授權檔 `LICENSE` 為 AGPL-3.0，未變更。
- 程式入口：`pdf2zh_next.main:cli`；GUI 由 `--gui` 啟動；主要 PDF 流程在 `high_level.py`，翻譯器在 `translator/translator_impl/`，設定在 `config/`。Windows EXE 工作流程使用 PyStand 與 `script/_pystand_static.int`。
- 專案由 `pyproject.toml` 管理，以 `uv sync` 安裝、`uv run pytest .` 測試。本機使用 `uv 0.13.1`、Python 3.13.16 與專案內 `.venv`。測試時將 `USERPROFILE` 指向 `.baseline-home`，避免讀取個人 API 設定；未使用付費服務。
- 原始依賴解析會安裝 `bitstring 5.0.0`，導致 `babeldoc 0.6.2` 匯入 `BitStream` 時，測試收集失敗。為取得可執行基準，只在 `.venv` 內改裝 `bitstring 4.4.0`（及其 `tibs 0.5.7`、`bitarray 3.12.3`）；此時尚未修改原始碼或 `pyproject.toml`。未鎖定依賴是後續須處理的重現性問題。

## 自動化測試與啟動

- 初次 `pytest -q .`：因上述 `BitStream` 匯入錯誤，在收集階段停止，未算通過。
- 安裝相容 `bitstring` 後，在非沙盒環境執行 `python -m pytest -q . --basetemp=.test-output/pytest-baseline`：**99 passed、73 warnings**。沙盒中的 Windows `asyncio` 本機 socket 受限，會讓 GUI 匯入與 CLI 啟動停住，因此完整結果採非沙盒執行。多數警告是 SWIG、Gradio、Pydantic 與巢狀 argparse 群組的棄用警告。
- `pdf2zh_next.exe --version`：成功，輸出 `pdf2zh-next version: 2.9.0`。
- 測試檔案涵蓋設定、快取、DeepSeek 與 GUI 特定情境；缺少多數翻譯服務的 Retry/Backoff 行為測試，後續另建 characterization tests。

## 指定 PDF 基準

- 輸入：`C:\Users\user\Downloads\High_speed_buffer_latch_ISCAS03.pdf`，4 頁、307,450 bytes，未加密；Poppler 能擷取文字。
- 以 CLI `--google --debug --no-auto-extract-glossary --qps 200 --pool-max-workers 16 --watermark-output-mode no_watermark` 執行完整 PDF 流程。測試腳本在記憶體中將 Google 翻譯呼叫替換成「原文回傳」，沒有網路翻譯請求。`--debug` 使流程在主行程執行，讓 Mock 生效；輸出含原專案的除錯框線。
- 成功處理 1,062 個段落並產生單語與雙語 PDF，流程報告耗時 **276.68 秒**。翻譯層共呼叫 233 次，其中 232 次命中先前未完成基準嘗試留下的翻譯快取，實際 Mock 呼叫 1 次；這限制了本次對翻譯請求次數的驗證。
- `pdfinfo`：單語與雙語輸出各 4 頁、未加密，大小分別為 4,118,716 與 8,119,938 bytes。Poppler 成功渲染兩份 PDF 的所有頁面；已檢視單語第 1 與第 4 頁，文字區、公式與電路／圖表仍可見。渲染時 Poppler 報告缺少部分原始字型及 Type 3 字型警告，因此視覺品質須依後續前後對比判斷。
- 頁面文字雜湊、公式符號數與嵌入圖片數記錄於 `reports/baseline_pdf_metrics.json`。PDF 的文字抽取量會受輸出字型／版面編碼影響，該數值只作同流程前後比較，不作譯文品質分數。

## 既知限制

- 無付費 API 呼叫，也未驗證真實服務的翻譯品質、配額、429 或網路失敗。
- 此基準的完整 PDF 輸出為 debug 模式；正常模式的無框線輸出尚待回歸測試。
- 原專案在使用者舊路徑 `~/.config/pdf2zh` 有 `config.v3.toml`；僅確認檔案存在，沒有讀取或輸出其中的憑證內容。專案根目錄目前沒有 `config/`。
