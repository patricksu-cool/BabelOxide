# 第二輪第七階段：重構前穩定基準（2026-10-23）

基準為 `bf96246`，產品版本 2.9.0、BabelDOC 0.6.3、PyMuPDF 1.26.7、Gradio 5.35.0。測試使用隔離設定與合成憑證，沒有呼叫付費翻譯 API。

| 檢查 | 結果 |
|---|---|
| 完整自動化測試 | **160 passed、67 warnings、0 failed** |
| Config／Retry／CLI／日誌遮罩重點測試 | **119 passed、1 warning**；涵蓋建立、讀寫、重載、遷移與路徑 |
| Retry／Backoff Characterization Tests | **34 passed**；本輪核心算法未更動 |
| CLI 真實子行程＋本機 API | 成功／部分失敗／完全失敗退出碼 **0／1／1**，輸出 PDF **2／2／0**，12 次本機請求，合成金鑰未進日誌 |
| Python GUI | 第六階段同一產品程式碼已完成上傳、設定儲存／重載、翻譯及下載；單語、雙語與 ZIP 均有效 |
| 靜態檢查 | `ruff check .`、`git diff --check` 通過 |
| 安全檢查 | 執行依賴 164 套件、98 筆原始記錄、**68 項去重公告**；維持第六階段已揭露的 Gradio／Pillow／Starlette 風險，稽核 exit 1 |

## Windows EXE 與指定 PDF

從正式 lockfile 用 `uv sync --locked --no-dev` 建立全新環境，136 個實際安裝套件通過 `uv pip check`。專案先建成 wheel 再非 editable 安裝，確認沒有 `_editable_impl_pdf2zh_next.pth`，以既有 PyStand 1.1.4／embedded Python 重新組成本機 EXE 目錄。這是本機打包流程，沒有執行完整 GitHub Actions 發行或重新下載離線資源包。

- 從不同工作目錄執行 `--version`：exit 0、2.9.0；`config/default/2.9.0.toml` 在 EXE 旁，工作目錄沒有 `config/`。
- 指定四頁 PDF 使用本機 OpenAI 相容 identity API：EXE exit 0、107 請求（批次 36），單語／雙語各四頁；合成金鑰未出現在日誌。明確指定與 Python 基準相同的 `--lang-out zh-CN` 後，逐頁文字、公式符號及圖片指標 **8/8 頁一致**，50 DPI 的八頁渲染**逐像素一致**。詳細資料見 `07_exe_*json`。第一次使用全新設定的預設 `zh`，與 Python 保存設定 `zh-CN` 不同，故未用該次結果作相等比較。
- 損壞 PDF：EXE exit 1，沒有輸出 PDF。
- EXE GUI 首頁 HTTP 200；瀏覽器實際顯示上傳、語言、翻譯與預覽區。上傳一頁測試 PDF 後，預覽成功顯示正文、頁碼與翻頁控制。GUI 測試行程均已停止。

首次 EXE 路徑檢查因 PowerShell 使用唯讀系統變數名稱造成隔離目錄設定失效，程式意外將舊設定副本遷移到忽略的測試目錄。未開啟或輸出副本內容，已立即刪除，原設定未修改；之後改用明確命名的隔離目錄重跑，確認沒有遷移副本。測試資料與設定不提交 Git。

## 失敗、限制與門檻判斷

- `ruff format --check`：**失敗**，8 個既有檔案需格式化，包含 GUI、high_level、兩個 CLI translator、PDF 比較腳本及三個既有測試。工作樹產品程式碼未修改，故為既有格式差異；後續只在涉及模組時整理。
- 文件建置：第六階段修改前後都因缺少 `partials/languages/zh_TW.html` 失敗；嚴格模式另有失效文件連結。沒有當成通過。
- 瀏覽器有既有 `Could not parse custom js method`／`Unexpected token ';'`，來源是頁面載入的自訂 JavaScript；主要 GUI、PDF 預覽與後端流程實測可用。後續可用獨立測試和最小修正處理。
- 指定 PDF 的既有公式附近重疊仍存在，與第四階段基準一致；identity Mock 不代表真實中文譯文品質。
- 真實翻譯供應商、完整 CI 發行、其他作業系統與其他 Windows 電腦：**未執行**。EXE GUI 沒有重跑完整翻譯下載；EXE CLI 已處理指定 PDF，Python GUI 已完成完整事件流程。

必要的產品啟動、Config、翻譯輸出、退出碼、Retry 與 EXE 驗證已通過，未發現本輪修補造成的重大功能退化。保留已揭露的依賴與文件問題，將此版本保存為第八階段重構前基準。
