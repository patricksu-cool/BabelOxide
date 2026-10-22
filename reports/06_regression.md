# 第六階段：重構前完整回歸（2026-10-22）

## 測試環境與範圍

- Windows、Python 3.13.16、專案 `.venv`。先修正兩項原始依賴不相容：`bitstring==4.4.0` 與 `tencentcloud-sdk-python-tmt==3.1.121`；該修正已有獨立 commit。未為安全公告升級 BabelDOC、Gradio、Pillow 或 Starlette。
- 指定的四頁 `High_speed_buffer_latch_ISCAS03.pdf`；使用原始 CLI／BabelDOC 翻譯流程與 Google 翻譯函式的固定離線 Mock。未呼叫真實或付費 API。保留與修改前相同的 `--debug --google --no-auto-extract-glossary --qps 200 --pool-max-workers 16 --watermark-output-mode no_watermark` 選項。
- 原始基準見 `02_baseline.md`；設定修改後結果見 `03_config_validation.md`。本階段的單語／雙語 PDF 保存在被 Git 忽略的 `.test-output/stage6-stable-pdf/`，其摘要及逐頁比對為 `06_pdf_metrics.json`、`06_mono_render_comparison.json`、`06_dual_render_comparison.json`。

## 通過

| 項目 | 結果 |
|---|---|
| 完整自動測試 | **138 passed、73 warnings、0 failed、0 xfailed**。涵蓋設定建立／遷移／讀寫／重載、相依套件介面、服務重試與失敗分類、逾時、取消和資源清理。原先 Tencent SDK 缺少 `TextTranslate` 類別的 xfail 在相容版本修正後改為通過。 |
| 啟動與 GUI | 原始碼 CLI `--version` 成功；GUI 啟動並從 `127.0.0.1:17861` 回傳 HTTP 200 的首頁。設定檔實際寫於指定程式目錄的 `config/`，改變工作目錄不影響讀寫。 |
| 相依套件檢查 | `uv pip check` 通過，專案 `.venv` 中 185 個已安裝套件無依賴衝突。`BitStream` 與 Tencent `TextTranslate` 介面有直接測試。 |
| 指定 PDF 全流程 | 離線處理成功，翻譯入口呼叫 233 次，其中快取 232 次、Mock 實際呼叫 1 次；輸出單語、雙語 PDF 均可由 Poppler 開啟與渲染，每份均為 4 頁。 |
| PDF 內容比對 | 相較第三階段，單語公式符號 **48**、嵌入圖片 **23,998**；雙語公式符號 **98**、嵌入圖片 **47,996**，全數維持一致。可擷取字元數由單語 505,225 至 505,786、雙語 2,059,307 至 2,060,677。文字雜湊不同，不能宣稱位元組相同；差異以版面渲染與內容計數進一步評估。 |
| PDF 視覺比對 | 對第三階段 PDF 以 50 DPI 渲染四頁，單語各頁變動像素比例 **0.39%–0.68%**、雙語 **0.33%–0.61%**；最高平均色彩通道差異 **0.067/255**。檢視含公式、電路圖及波形的頁面，未見明顯破版或圖表遺失。Poppler 發出原 PDF 字型缺失／Type 3 警告，流程仍完成。 |
| Retry／錯誤情境 | Characterization Tests 以 Mock 與虛擬等待涵蓋次數、等待、429／5xx、`Retry-After`、例外傳遞、超時、取消及服務差異；產品重試演算法未修改。 |
| Windows EXE 路徑 | 依現有 PyStand 啟動方式，用 PyStand 1.1.4 與 Python 3.13.3 embedded runtime 重建本機 EXE。從其他工作目錄執行 `--version` 成功；預設設定建立在 EXE 所在的 `.test-output/exe-build-stable/config/default/2.9.0.toml`，未在工作目錄建立 `config/`。 |
| 靜態檢查 | `ruff check .` 與 `git diff --check` 通過。 |

## 失敗／未通過

- `ruff format --check pdf2zh_next/config/main.py tests/config/test_location.py` 未通過：兩檔只有長行排版差異，沒有語意差異。安排在第七階段的設定模組小型整理中修正，並獨立驗證。
- `uv audit --locked` 仍回報 **89 筆記錄、58 項去重公告**。這是已知依賴版本風險，不是測試故障，也尚未修補；詳見 `04_scan_plan.md` 與 `04_dependency_audit.json`。其中 BabelDOC 0.6.2 的不受信任 PDF CMap pickle 問題尤其需要獨立的相容性升級驗證。

## 未執行與限制

- 未使用個人 API Key、真實翻譯服務或付費服務，因此不能保證外部服務的實際網路行為。HTTP／SDK 失敗路徑由 Mock 測試。
- EXE 是遵循專案 PyStand 啟動結構的本機重建，已驗證 CLI 啟動與設定路徑；未執行完整 CI 發行流程、DirectML 資產打包、EXE GUI 互動或安裝程式驗證。
- PDF 測試以 `--debug` 保證 Mock 在主行程生效，因此輸出含原有除錯框線；未測非 debug 模式的同一 PDF 翻譯流程。未以人工逐字核對每一段文字或公式語義。
- 沒有以惡意 PDF 驗證依賴安全公告的可利用性；依賴稽核只證明版本命中公告。

## 進入第七階段的結論

可執行的必要功能測試均通過，未發現本次設定路徑或依賴相容性修正造成重大功能退化。保留重試與跨行程核心原始實作；第七階段只做已規劃、能逐項測試的小型整理。安全依賴升級與高風險架構搬移另列後續工作。
