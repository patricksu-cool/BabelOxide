# BabelOxide 1.0.0 驗證狀態

更新：2026-11-01（Asia/Taipei）。開發分支 `feature/babeloxide-v1`。Windows 已有本機驗證的完整包；macOS 原生驗收與授權審查仍未完成，未合併 main、未建立正式 `v1.0.0` tag、未 Push／發布 Release。

## 保留的本機產物

`dist/BabelOxide-v1.0.0-windows-x64/` 是唯一保留的完整執行包，GUI 為 `BabelOxide-gui.exe`。13,127 項 manifest 與檔案雜湊再次核對通過；程式碼建置提交為 `77a5c38f3b4086fdf491a0bca59522e2fbf54dfb`，`BUILD_INFO.release_ready=false` 不變。需要系統官方 VC++ v14 x64 Runtime，未驗證乾淨 VM。

重複的發行 ZIP、舊 Source ZIP、過時包與建置暫存已移除；需要封裝時再生成。`build-inputs/windows-x64/` 保留原生 runtime、PDFium、182 項離線資產及目前 85 個固定套件的快取；其中舊 BabelDOC 0.6.3 實體快取已換成鎖定的 0.6.4，整套依賴成功離線安裝。開發 `.venv/`、個人 config、Git 與程式碼工具狀態保留。

## 已完成的驗證

先前同一程式碼的完整 Windows 建置驗證：Rust **301 passed、0 failed、1 預設 ignored**，fmt／嚴格 Clippy／Release、明確執行的原生 PDFium 測試通過。BabelDOC 0.6.3／0.6.4 的指定文件 mono／dual 結構與八頁 Poppler RGB 一致。實際 ZIP 曾搬到中文空格路徑驗證；深層 ONNX 路徑須使用支援長路徑的解壓工具。

GUI 已實測多選、預覽、自訂組合保存／重新命名／重開、逐份佇列、取消與清理、原設定重試、HTTP 400 後重試、單語／雙語、翻頁／縮放、開啟檔案／資料夾及中文另存；重試後既有成果雜湊不變。387 次請求全部使用本機 fixture，兩次 GUI 退出皆 0，日誌沒有合成 Key。Identity 回應只能驗證排版與串接，不能衡量真實翻譯品質。

本輪在清除舊狀態後重新執行：Worker **35**、Windows Builder **35**、Portable Builder **7**、受控引擎回歸 **4**、來源封裝 **22** 項通過；Ruff、Rustfmt、Cargo workspace metadata、Actions 靜態契約、CLI／GUI 版號、原生依賴與 PDFium 載入通過。功能程式碼未變更，沒有重新執行會重建大量 target 的 Rust 編譯／Clippy／測試。

## 目錄與測試資料

專案內 891 份 PDF 均確認為歷史測試資料並已清除；自動測試可自行生成合成 PDF。`tests/baseline/`、core 契約／壓縮 fixtures、兩平台鎖檔及授權原文保留。`.test-output/`、`.baseline-home/`、專案內 `.uv-cache/`、舊 target 與舊 Python App 輸出已清除；本輪測試產物亦已再次清理，專案約 2.05 GiB。測試與輸出目錄由程式建立，詳見 [BUILD](BUILD.md)。

七個 Cargo crate 的 target／模組／測試引用及 Python 維護入口已查核，沒有足夠證據可刪除其正式實作；Retry／Backoff、QPS、Concurrency、Cache、IPC 與 UI 行為不變。重複授權文字因各元件的署名與來源責任分別保留。開發歷史由 Git 保存，不長期保留逐輪測試 Log、截圖與報告。未執行全域 Cargo／uv 快取清理或 Git 歷史重寫。

## 尚未通過的發行條件

- macOS arm64、14+ 的原生 build、dylib 匯入、完整 .app、桌面互動仍未驗證；設定、來源、84 個固定 wheel 清單與 CI 保留。Windows 靜態／合成測試不代表 Mac 通過。
- GitHub Actions 的兩個遠端 job 未觸發／驗證；目前只有 Node24 Actions、唯讀權限、matrix、完整 ZIP 與 7 天保留的靜態契約通過。
- `dispatch 0.2.0` 的授權原文仍缺（manifest 宣告 MIT）；objc2 的 Apple SDK 說明及 Mac 原生庫再散布條件仍需審查。完整 [LICENSE](../LICENSE) 與 [THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md) 保留，沒有正式法務認證。
- 真實 OS 拖放、高 DPI 125%／150%／200%、所有服務、唯讀 ACL、不同實體磁碟及乾淨 Windows VM 未逐項驗收；詳見 [LIMITATIONS](LIMITATIONS.md)。
