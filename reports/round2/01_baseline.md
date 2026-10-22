# 第二輪第一階段：修改前基準（2026-10-23）

## Git 與環境

- 從上一輪的 `9236822` 建立 `maintenance-round2`，未重設原有歷史；建立分支前工作樹乾淨。
- Windows、Python 3.13.16、uv 0.13.1；專案 `.venv` 內已安裝 185 個相容套件，`uv pip check` 通過。
- 基準套件：BabelDOC 0.6.2、bitstring 4.4.0、Gradio 5.35.0、Pillow 11.3.0、Starlette 0.52.1、GitPython 3.1.62、Tencent TMT SDK 3.1.121。
- 現有 `uv.lock` 被 `.gitignore` 排除，未受 Git 追蹤。安全更新階段須將確定可重現的鎖檔正式納入版本控制。
- 專案既有 PyStand 1.1.4 打包目錄位於忽略的 `.test-output/exe-build-stable/`；本階段尚未重建 EXE。

## 自動測試與入口

- 在隔離的 USERPROFILE、TEMP、TMP、uv 快取目錄下，完整測試：**139 passed、73 warnings、0 failed**，約 7.18 秒。Retry／Backoff 子集：**34 passed、5 warnings**。這與上一輪 139 passed 一致。
- 首次在受限沙盒內直接執行時，BabelDOC SQLite 快取於使用者目錄開啟失敗；改用工作區隔離目錄並在允許 Windows 測試程序的環境重跑後通過。首次結果是環境阻礙，並非測試通過。
- 原始碼 CLI `--version` 回傳 2.9.0；Gradio GUI 在 `127.0.0.1:17861` 啟動，首頁 HTTP 200、654138 bytes。尚未在此階段逐一操作 GUI 事件與檔案上傳。
- 上一輪本機 PyStand EXE 從不同工作目錄以 `--version` 啟動並回傳 0。該次啟動意外觸發舊使用者設定的自動複製；複製檔未開啟，已立即刪除，未加入 Git。後續 EXE 測試強制使用隔離 USERPROFILE。
- Config 建立、讀取、遷移及 EXE 路徑測試均包含於完整測試；正式 GUI 的手動設定流程留待第四階段。

## 指定 PDF 離線處理

- 輸入：`C:\Users\user\Downloads\High_speed_buffer_latch_ISCAS03.pdf`，307450 bytes、4 頁。
- 使用原有 `offline_pdf.py` 將 GoogleTranslator 翻譯函式換成 identity Mock，執行真正 CLI/BabelDOC PDF 管線；不使用真實憑證或付費 API。以 `--debug --no-auto-extract-glossary --qps 200 --pool-max-workers 16 --watermark-output-mode no_watermark` 執行。
- 結果：exit 0；翻譯入口 233 次，離線 Mock 呼叫 157 次，快取 76 次。相較上一輪，新的隔離 HOME 使用冷快取，故 Mock 呼叫次數不同。
- 單語與雙語均輸出可開啟、可擷取文字、可渲染的 4 頁 PDF。單語 505751 字元、公式符號 48、圖片 23998；雙語 2060433 字元、公式符號 98、圖片 47996。詳細逐頁結果與 SHA-256 見 `01_baseline_metrics.json`。
- 與上一輪最終渲染以 50 DPI 比較，單語逐頁變動像素 0.39%–0.68%，雙語 0.34%–0.61%；與上一輪未改產品程式碼的兩次輸出差異量級相近。詳見兩份 `01_prior_to_baseline_*_render.json`。檔案或文字雜湊並不相同，不視為位元組等價。
- Poppler 對原 PDF 的缺少 Symbol、ArialUnicode 顯示字型及 Type 3 字形提出警告，但所有頁面完成渲染。此階段尚未測非 Debug、GUI 上傳或真實翻譯 API。

## Retry／Backoff 基準

`tests/test_retry_characterization.py` 的 34 項 Mock 測試通過，涵蓋 16 個服務、22 個 Tenacity 裝飾器以及重試次數、等待、429/5xx、逾時、例外、取消與資源清理。產品 Retry／Backoff 尚未修改；供應商 SDK 自身及真實網路行為仍未驗證。
