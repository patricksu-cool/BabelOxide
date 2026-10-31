# 最終狀態

更新：2026-10-31（Asia/Taipei）。**整合驗證進行中；最終包與 main 尚未完成驗收。**

## 架構與完成項目

Rust 的 7 個 crates 負責 GUI、CLI、24 個服務選項／16 種 provider、HTTP、Retry／Backoff、QPS／Concurrency、usage、設定、SQLite cache、tasks、IPC／程序清理、PDFium 預覽與另存。Python 只保留 BabelDOC 0.6.3 引擎、85 個必要 runtime distributions 及最小 Worker；沒有 Gradio 或舊 Python App。

設定預設為 EXE 旁 `config/config.v3.toml`，Rust cache 為 `cache/translations.v2.db`，BabelDOC 可變資料為 `cache/babeldoc-home/`。包內模型／字型種子與可變 cache 分開；唯讀位置會顯示獨立的使用者資料備援路徑，保護原設定與已有備援設定。

## 清理

依審閱清單移除 541 個舊 App／測試／建置／文件／報告檔，另整合 36 份過時移植快照。關鍵 Python oracle 與 PDF 指標保留於 `tests/baseline/` 和 core fixture；必要第三方文字、字型 notices 與固定資產保留。私人設定、Key、模型、Git 與未追蹤 PDF 未刪除。詳見 [清理摘要](MIGRATION.md) 與 [Python 盤點](PYTHON_DEPENDENCIES.md)。

## 驗證紀錄

| 項目 | 狀態 |
| --- | --- |
| Rust Workspace | 244 passed、0 failed、1 預設 ignored；fmt、Clippy、Release build 通過 |
| 舊 Python 基準 | 清理前 285 passed；retry／transport 39 passed |
| 縮減後 Worker | 32 passed，開發 venv 不含舊 App／Gradio |
| Builder | 29 passed；85 個 wheel 精確身分保持；182 個資產完整驗證 |
| v3 Release GUI | 實際完成選檔、服務／API／語言修改、保存重開、翻譯、兩種預覽／第二頁／縮放、另存、失敗、再執行、取消與處理中關閉 |
| v3 PDF | mono／dual 各四頁，文字／座標／圖片相同；八頁 96 DPI RGB 像素差 0 |
| 最終包／搬移／Native smoke | 待驗證 |
| Git main 整合 | 待最後回歸通過 |

## 使用與重建

完整專案為 `C:\Users\user\Documents\PDFMathTranslate-next-2.9.0`；目標個人版為 `dist/final/`，GUI 入口 `pdf2zh-gui.exe`，CLI 為 `pdf2zh.exe`。在 GUI 選擇服務、輸入自己的 URL／模型／Key 後保存；執行版本不需額外 Python／Rust／uv。系統仍需官方 VC++ v14 x64 Runtime。

重建與本機 mock 回歸見 [DEVELOPMENT](DEVELOPMENT.md)，一般問題見 [TROUBLESHOOTING](TROUBLESHOOTING.md)。

## 實際限制

Tencent 的 profile／CVM／TKE 動態憑證鏈未移植，支援靜態及完整環境 ID／Key；HTTPX write／pool timers、proxy／CA 等 SDK 邊界未宣稱完全等價。各服務真 API、乾淨 VM、不同實體磁碟及全部進階選項未逐一驗證；唯讀備援測試使用注入的權限失敗。原生拖放尚未在本輪桌面實測，既有 egui drop 測試保留。未宣稱商業再散布審查或絕對不會遠端重複計費。這些限制不隱藏在 Worker 中，詳見 [服務相容性](PROVIDERS.md)。
