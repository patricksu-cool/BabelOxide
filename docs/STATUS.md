# BabelOxide 1.0.0 驗證狀態

更新：2026-11-01（Asia/Taipei）。開發分支 `feature/babeloxide-v1`。Windows 已有本機驗證的完整包；macOS 原生驗收與授權審查仍未完成，未合併 main、未建立正式 `v1.0.0` tag、未 Push／發布 Release。

## 保留的本機產物

`dist/BabelOxide-v1.0.0-windows-x64/` 是唯一保留的完整執行包，GUI 為 `BabelOxide-gui.exe`。13,127 項 manifest 與檔案雜湊再次核對通過；程式碼建置提交為 `77a5c38f3b4086fdf491a0bca59522e2fbf54dfb`，`BUILD_INFO.release_ready=false` 不變。需要系統官方 VC++ v14 x64 Runtime，未驗證乾淨 VM。

重複的發行 ZIP、舊 Source ZIP、過時包與建置暫存已移除；需要封裝時再生成。`build-inputs/windows-x64/` 保留原生 runtime、PDFium、182 項離線資產及目前 85 個固定套件的快取；其中舊 BabelDOC 0.6.3 實體快取已換成鎖定的 0.6.4，整套依賴成功離線安裝。開發 `.venv/`、個人 config、Git 與程式碼工具狀態保留。

本輪重新建立 `target/release/BabelOxide.exe` 與 `BabelOxide-gui.exe`，沒有覆寫上述完整包。新版 CLI 已以原包的固定 runtime／資產、隔離設定與合成 PDF 驗證；完整發行包需由本輪 builder 重新生成，才能包含新增的集中授權文件。來源審查 ZIP 共 181 檔，不包含執行資源、個人設定或 Git。

## 已完成的驗證

先前同一程式碼的完整 Windows 建置驗證：Rust **301 passed、0 failed、1 預設 ignored**，fmt／嚴格 Clippy／Release、明確執行的原生 PDFium 測試通過。BabelDOC 0.6.3／0.6.4 的指定文件 mono／dual 結構與八頁 Poppler RGB 一致。實際 ZIP 曾搬到中文空格路徑驗證；深層 ONNX 路徑須使用支援長路徑的解壓工具。

GUI 已實測多選、預覽、自訂組合保存／重新命名／重開、逐份佇列、取消與清理、原設定重試、HTTP 400 後重試、單語／雙語、翻頁／縮放、開啟檔案／資料夾及中文另存；重試後既有成果雜湊不變。387 次請求全部使用本機 fixture，兩次 GUI 退出皆 0，日誌沒有合成 Key。Identity 回應只能驗證排版與串接，不能衡量真實翻譯品質。

本輪發布前自動化驗證使用既有 rustc／cargo **1.99.1**、Python **3.13.16**：

- Rust fmt、workspace／all-targets 嚴格 Clippy、workspace test 及 `cargo build --release --locked` 全部通過。測試 **301 passed、0 failed、1 ignored**；該 PDFium native smoke 另以完整包及生成 PDF 明確執行，**1 passed**。CLI／GUI Release 執行檔存在。
- Worker **35**、Windows Builder **36**、Portable Builder **7**、受控 BabelDOC 引擎回歸 **4**、Windows 原始碼封裝 **22** 項通過；Ruff 通過。Mac 原始碼打包的 Bash 語法與 Git Bash 整合測試 **24 passed、2 skipped**，兩項為本機無法建立的真 symlink，仍需 Mac 實跑。
- 新 Release CLI 使用合成 PDF／本機 Mock API，成功載入設定、啟動 Worker、完成 3 次請求、正常退出並產生 mono／dual；Poppler 分別解析為一頁 A4／A3。HTTP 401 回報 error、exit 1；實際 Ctrl-C 回報 cancelled、exit 130，觀察到的三個 Worker／後代 PID 均已退出，沒有 PDF。未呼叫真實或付費 API。
- 已按 SHA256 重新收集 439 個 Rust 套件（432 個 registry、7 個 workspace）與 85 個 Windows Python 套件，合計 917 份授權／來源文件，缺文清單為空。dispatch 0.2.0 以已驗證發布 manifest、明確標示的 OSI MIT 標準範本及來源告知補齊；另保留 BabelDOC 原文、OFL 1.1／NAVER 字型條款、ONNX 第三方 notices。這是文件盤點結果，不是跨平台散布法務認證。
- 現有完整包全 manifest 雜湊與原生 PDFium 通過；Workflow 靜態契約確認僅手動觸發、兩平台、唯讀權限、完整 ZIP 及 7 天保留。沒有觸發遠端 Actions。

本輪沒有 GUI 桌面操作。聲明更新前的新 GUI 曾回報 1.0.0；更新後增量 Release 建置成功，最終版號擷取因自動審查判定可能開窗／掛起而未重做，不列為本輪 GUI 互動通過。使用者仍需親自驗收。

## 目錄與測試資料

前輪已清除專案內 891 份歷史測試 PDF及舊產物，當時約 2.05 GiB。`tests/baseline/`、core 契約／壓縮 fixtures、兩平台鎖檔及授權原文仍保留。本輪允許保留重新產生的 target／Python／Cargo 快取，以提高後續編譯速度；僅適度清除本輪合成 PDF與大型隔離 Worker 中間資料，沒有再次大規模清理或觸碰私人設定／翻譯 cache。測試與輸出目錄由程式建立，詳見 [BUILD](BUILD.md)。

七個 Cargo crate 的 target／模組／測試引用及 Python 維護入口已查核，沒有足夠證據可刪除其正式實作；Retry／Backoff、QPS、Concurrency、Cache、IPC 與 UI 行為不變。重複授權文字因各元件的署名與來源責任分別保留。開發歷史由 Git 保存，不長期保留逐輪測試 Log、截圖與報告。未執行全域 Cargo／uv 快取清理或 Git 歷史重寫。

## 尚未通過的發行條件

- macOS arm64、14+ 的原生 build、dylib 匯入、完整 .app、桌面互動仍未驗證；設定、來源、84 個固定 wheel 清單與 CI 保留。Windows 靜態／合成測試不代表 Mac 通過。
- GitHub Actions 的兩個遠端 job 未觸發／驗證。僅手動版 Workflow 必須先由使用者加入 GitHub 實際預設分支，才能顯示 Run workflow；本機 main 沒有該檔案，且沒有 remote 可確認 GitHub 設定，詳見 [BUILD](BUILD.md)。
- macOS `.command` 尚未在原生 Bash 3.2／zip／unzip／系統 Perl或 Finder 雙擊執行；已接入 Mac Actions 原生封裝測試，Windows 結果不能代替。
- objc2 的 Apple SDK 說明及 Mac standalone CPython／原生 wheel 的成品散布條件仍需審查；公開散布時須提供實際提交的相應來源／必要依賴來源。完整 [LICENSE](../LICENSE) 與 [THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md) 保留，沒有正式法務認證。
- 真實 OS 拖放、高 DPI 125%／150%／200%、所有服務、唯讀 ACL、不同實體磁碟及乾淨 Windows VM 未逐項驗收；詳見 [LIMITATIONS](LIMITATIONS.md)。
