# 第二輪第四階段：實際 PDF、GUI 與 Windows EXE 驗證（2026-10-23）

## 測試條件與範圍

- Windows、Python 3.13.16、隔離的 `.test-output/round2-env`，BabelDOC 0.6.3／PyMuPDF 1.26.7；對照使用修改前 `.venv` 的 BabelDOC 0.6.2／PyMuPDF 1.25.2。沒有讀取或使用真實 API Key。
- 指定四頁 PDF：`C:\Users\user\Downloads\High_speed_buffer_latch_ISCAS03.pdf`。Debug 使用 GoogleTranslator 的離線 identity stub；非 Debug 使用只在 `127.0.0.1` 監聽、相容 OpenAI Chat Completions 的本機伺服器，合成金鑰，關閉翻譯快取。伺服器可回傳一般段落及 BabelDOC 所需的批次 JSON。這只驗證翻譯管線與回傳格式，不驗證真實中譯品質。
- 首次非 Debug 測試替身把提示詞誤作譯文，造成極小字及大量文字重複；修正輸入片段擷取並停用已污染的快取後重新測試。首次產物保留在忽略的 `.test-output/`，以下結果只採用修正後執行。測試替身與原始 PDF 均未提交。

## PDF 結果

| 項目 | 修改前 0.6.2 | 修改後 0.6.3 | 結論 |
|---|---:|---:|---|
| 非 Debug 本機 API 請求 | 107（批次 36） | 107（批次 36） | 相同 |
| 單語／雙語頁數 | 4／4 | 4／4 | 相同 |
| 單語／雙語可擷取字元 | 54,544／150,543 | 54,544／150,543 | 相同 |
| 單語／雙語公式符號計數 | 48／98 | 48／98 | 相同 |
| 單語／雙語嵌入圖片計數 | 23,998／47,996 | 23,998／47,996 | 相同 |
| 逐頁擷取文字 SHA-256 | 全部相同 | 全部相同 | 無文字差異 |
| 50 DPI 逐頁渲染 | 兩種輸出各 4 頁 | 兩種輸出各 4 頁 | 8 頁逐像素相同 |

Poppler 可開啟與渲染全部輸出；150 DPI 目視檢查第 2 頁的電路圖、公式和文字。圖像未見缺漏，文字大致可讀。公式附近以及少數正文仍可見局部重疊或斷字；以相同本機回應重跑舊版後，舊版第 2 頁與新版渲染一致，確認這不是本次 BabelDOC 升級造成的回歸。Identity 回應不是實際中文翻譯，不能據此保證真實翻譯後的排版品質。

修改後 Debug 模式亦成功產生 4 頁單語／雙語 PDF；公式符號 48／98、圖片 23,998／47,996，與第二階段相同。與第二階段 Debug 渲染相比，50 DPI 單語各頁約 0.36%–0.66%、雙語約 0.31%–0.58% 像素有變化；Debug 標記與版面資料不完全穩定，不能用逐位元組相等作為門檻。詳細逐頁指標及差異見同目錄 `04_*metrics.json` 和 `04_*render.json`。

## GUI

- Gradio 在 `127.0.0.1` 啟動，主頁 HTTP 200。透過真實 Gradio Client 事件完成 PDF 上傳、儲存設定、載入設定、翻譯和檔案下載；不是只呼叫未綁定的內部函式。事件檢查顯示上傳、翻譯、儲存與頁面載入端點存在。
- 使用一頁測試 PDF、相同本機 API。GUI 產生並下載可由 PyMuPDF 開啟的單語、雙語 PDF（各 1 頁），以及包含兩份 PDF 的 ZIP。儲存後另以全新 Python 行程重新載入隔離設定，確認服務為 OpenAI、模型名稱及 loopback URL 一致；未輸出或檢視金鑰欄位。
- GUI 停止時出現一次 Uvicorn 連線關閉後的 `LocalProtocolError`；HTTP／上傳／翻譯／下載均已完成。Identity 批次回應與輸入相同時，BabelDOC 依原邏輯記錄 fallback 警告。這兩項留作後續觀察，沒有因此修改翻譯或重試邏輯。

## Windows EXE

- 依專案 PyStand 1.1.4＋embedded Python＋wheel `site-packages` 流程更新本機打包目錄，未在此機器執行 GitHub Actions 發行工作流程。從與 EXE 不同的工作目錄執行 `--version`，exit 0、顯示 2.9.0；工作目錄未建立 `config/`，EXE 旁有 `config/default/2.9.0.toml`。
- 以合成 `--openai-api-key` 執行 EXE Debug 啟動，輸出不含合成金鑰。另發現 PyStand 最外層例外原以 `traceback.print_exc()` 直接輸出，繞過應用程式日誌遮罩；已以獨立 commit `45dfaba` 改為遮罩後輸出，兩種 CLI 參數型式的早期例外測試均通過。
- EXE GUI 以隱藏背景行程短暫啟動，`http://127.0.0.1:17865/` 回傳 HTTP 200 與約 654 KB HTML，隨即停止；未對 EXE GUI 重複完整上傳翻譯操作。

## 自動檢查與未執行項目

- 完整測試：**153 passed、68 warnings、0 failed**。`ruff check .`、新增測試格式檢查及 `git diff --check` 通過。
- 真實翻譯供應商與付費 API：**未執行**；無使用者授權，也沒有測試憑證。完整 CI 發行包、其他 Windows 電腦及其他作業系統：**未執行**。
- EXE GUI 的完整翻譯／下載：**未執行**；Python GUI 已完成同一路徑的 HTTP 事件與下載驗證，EXE 已完成 GUI 啟動與設定路徑驗證。後續階段應再檢查更新依賴後的 EXE。
- 本階段未更動產品 Retry／Backoff 算法；另保留已確認的既有公式重疊風險，避免把「PDF 可開啟」誤寫成「排版完全正常」。
