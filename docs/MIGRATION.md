# Rust 移植與清理摘要

2026-10-31。清理前回復點為 `codex/baseline-final-integration-20261031`（`22401df`），原 Python 穩定 main 為 `d18165e`。Git 歷史與 maintenance 分支保留，沒有重新初始化、刪分支、Push 或建立 GitHub Release。

先重新執行舊 Python 全套 285 項，以及獨立 retry／transport 39 項；確認 Release GUI 翻譯、取消、另存與 PDF 基準後，才依明確清單刪除舊 App。關鍵 oracle 移至 `tests/baseline/` 及 core 的 `legacy_retry_contract_v1.json`，不再依賴舊 App 的 import。

清理清單包含 541 個已追蹤檔：59 個舊應用檔、27 個舊測試、135 個重複歷史報告、245 個舊版多語 Gradio 文件、19 個舊 GitHub 流程／metadata、41 個文件翻譯中介檔與 15 個舊建置／根目錄檔。另整合／移除 36 份過時的移植快照；維護中的 IPC、cache、retry、服務矩陣、Python 盤點與 GUI 證據移至 `docs/`，兩份固定輸入移至 `python-worker/` 與 `resources/`。

已移除 Python GUI／Gradio、CLI、Config Manager、translator／SDK clients、cache、tasks、retry 等自行維護模組；舊 Docker／PyStand／Gradio 打包與發布腳本也已移除。Python 現在只有最小 Worker、引擎必要依賴和開發測試／組包工具。85 個 runtime wheel 身分與既有 v3 完全相同；根 uv 鎖檔只管理 pytest／Ruff，開發 venv 已依新鎖檔縮減。

刪除只作用於經審閱的明確追蹤檔及可再生 bytecode，沒有對整個專案遞迴刪除。私人設定、Key、模型、字型、DLL、Git 與三份未追蹤 PDF 都受到保護。精確清單與執行紀錄留於本機 `.test-output/final-validation/`；歷史詳情由 Git 保存，避免在正式文件中維持互相矛盾的多份報告。

最終功能、個人執行版本、測試限制與 Git 狀態以 [FINAL_STATUS](FINAL_STATUS.md) 為準。
