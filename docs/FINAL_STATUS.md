# 最終狀態

更新：2026-10-31（Asia/Taipei）。**主要個人使用流程已驗收，Windows 包已發布至 `dist/final/`。** 未執行與部分驗證範圍列於下方，不計為通過。

## 架構與完成項目

Rust 的 7 個 crates 負責原生 GUI、CLI、24 個服務選項／16 種 provider、HTTP、Retry／Backoff、QPS／Concurrency、usage、設定、SQLite cache、tasks、IPC／程序清理、PDFium 預覽與另存。Python 只保留 BabelDOC 0.6.3、85 個必要 runtime distributions 與最小 Worker；沒有 Gradio 或舊 Python App。Worker 的原生初始化包含 Windows 超長路徑的 OpenCV 相容處理，不含翻譯服務或重試邏輯。

設定預設為 EXE 旁 `config/config.v3.toml`，Rust cache 為 `cache/translations.v2.db`，BabelDOC 可變資料為 `cache/babeldoc-home/`。模型／字型種子與可變 cache 分開；唯讀位置會顯示使用者資料備援路徑，保護原設定與已有備援設定。Retry／Backoff 沿用原行為，獨立 Python oracle 與本機契約測試繼續保留。

## 清理與主要結構

依審閱清單移除 541 個舊 App／測試／建置／文件／報告檔，另整合 36 份過時移植快照。已移除 Python GUI／Gradio、CLI、設定管理、translator／SDK clients、cache、tasks、retry 與舊打包流程；私人設定、Key、模型、Git 與未追蹤 PDF 保留。詳見 [清理摘要](MIGRATION.md) 與 [Python 盤點](PYTHON_DEPENDENCIES.md)。

主要目錄為 `crates/`、`python-worker/`、`resources/`、`script/`、`tests/baseline/`、`docs/`；根 uv 專案只管理 pytest／Ruff，執行期依賴由 Worker profile 鎖定。必要 Python、PDFium、Rust、模型／字型 notices 已附；七個 egui 元件採同 workspace 的共享授權文字，其來源版本限制保留在 provenance，未宣稱完整商業再散布審查。

## 驗證結果

| 項目 | 結果與範圍 |
| --- | --- |
| Rust Workspace | 245 passed、0 failed、1 預設 ignored；fmt、Clippy `-D warnings`、Release build 通過 |
| PDFium 原生測試 | 明確執行上述 ignored 測試：1 passed；真 PDF 幾何／RGBA／錯誤與 handle 釋放通過 |
| 舊 Python 基準 | 清理前 285 passed；獨立 retry／transport 39 passed |
| 縮減後 Worker | 35 passed；開發 venv 不含舊 App／Gradio／BabelDOC |
| Builder | 31 passed；離線組包成功；85 個 wheel 身分與 v3 相同，182 個引擎資產驗證通過 |
| 服務／Retry | 33 項 compatibility 與 44 項 transport contracts 通過；未使用付費或外部 API，SDK 邊界限制見下方 |
| Release GUI | v3 先完成服務／API／語言、保存重開、翻譯、預覽縮放、另存、失敗、再執行、取消與處理中關閉；最終 EXE 再完成原生拖放／選檔、保存重開、四頁翻譯、單語／雙語第二頁、另存、HTTP 400、取消及處理中關閉 |
| 最終 GUI 清理 | 兩次 exit 0；Worker PID 16308、GUI PID 40112 均退出，無殘留；log 無合成 Key |
| PDF 基準比對 | 最終 GUI 與長路徑 CLI 各產生 mono／dual 四頁；文字、逐字元座標、圖片一致，各八頁 Poppler 96 DPI RGB 像素差 0 |
| 中文字型 | 本機固定繁體中文回應，單語／雙語輸出成功；Source Han Serif TW 嵌入與可見字形確認。此項是字型 smoke，不代表翻譯品質或中文排版全面驗收 |
| 搬移與工作目錄 | 中文＋空格資料夾、非原始碼 CWD、包內 Python／Worker／模型／字型、config／cache 與預覽通過 |
| Windows 長路徑 | 264 字元 CLI 路徑完整四頁翻譯通過；268 字元 GUI 路徑已自動開出視窗。該長路徑視窗完整操作因桌面工具授權逾時未完成；同一雜湊 EXE 在較短中文空格路徑完成上述 GUI 回歸 |
| 其他磁碟代號 | 暫時 Z: 別名啟動一頁翻譯通過，映射已移除；仍是 C: 實體磁碟，不計為跨實體磁碟驗證 |
| 唯讀備援 | 注入權限失敗、設定保留與備援不覆寫測試通過；未變更真實目錄 ACL |
| 最終包 | 13,067 個 manifest 項目、1,248,094,450 bytes（不含 manifest 自身）；全檔 hash／size／集合相符，無額外私人設定、Key、PDF、venv 或可變 cache |
| Git main | 待執行 fast-forward 與合併後回歸 |

量化證據見 [最終驗證資料](validation/final-verification.json)，桌面操作範圍見 [GUI 實測](validation/GUI.md)。PDF SHA256 為 `d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f`。本輪修正 Worker 啟動目錄、OpenCV 平行後端與 GUI COM／OLE 的長路徑問題；另修正測試 fixture 的非阻塞 socket 與 PID 檔發布競態，沒有重設計重試或 BabelDOC 排版。

## 使用與重建

完整專案：`C:\Users\user\Documents\PDFMathTranslate-next-2.9.0`。自用版本：`C:\Users\user\Documents\PDFMathTranslate-next-2.9.0\dist\final`，執行 `pdf2zh-gui.exe`；CLI 為 `pdf2zh.exe`。搬移時複製完整資料夾，不需另外安裝 Python／Rust／uv。

在 GUI 選服務、Base URL／模型／Key 與語言，再按保存；設定在執行包 `config/config.v3.toml`，空 Key 範本為 `config/config.example.toml`。cache 路徑見上方；唯讀備援會明確顯示。開始翻譯的健康檢查可能計入自己的 API 用量。

重新編譯與離線組包依 [DEVELOPMENT](DEVELOPMENT.md)，排解見 [TROUBLESHOOTING](TROUBLESHOOTING.md)。系統仍需官方 [VC++ v14 x64 Runtime](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170)，未從 System32 複製 DLL。包的程式碼提交為 `5db9e6a`；`BUILD_INFO.release_ready=false` 保留 builder 的公開再散布／乾淨機 gate，個人使用驗收以本報告及獨立證據為準。

## 剩餘限制與未執行項目

Tencent profile／CVM／TKE 動態憑證鏈未移植，支援靜態及完整環境 ID／Key；HTTPX write／pool timers、proxy／CA 等 SDK 邊界未宣稱完全等價，見 [服務相容性](PROVIDERS.md)。不承諾遠端服務絕不重複計費。

各服務真 API、乾淨 Windows VM、不同實體磁碟、真實唯讀 ACL 及全部進階欄位未逐一驗證。超長 GUI 路徑需要可用的 Windows 短檔名；沒有時會提示縮短資料夾，外部啟動器也可能拒絕長路徑。超長 runtime 的 OpenCV 使用單執行緒相容模式，ONNX 與翻譯 API 並行數不變；正常使用路徑不受此模式影響。
