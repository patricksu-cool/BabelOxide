# BabelOxide v1.0.0

原生 PDF 翻譯工具，保留公式、圖表與版面，支援單語及雙語輸出。Rust 負責 egui GUI、CLI、服務、設定、快取、重試與任務；最小 Python Worker 僅橋接 **BabelDOC 0.6.4**，沒有 Gradio。

Windows x64 完整包已通過本機建置、中文空格路徑搬移、CLI／GUI 與 PDF 回歸驗證。macOS arm64 原生驗證尚未完成，1.0.0 保留為工作分支版本，尚未建立正式版本標籤。實際結果與待審項目見 [STATUS](docs/STATUS.md)。

## 使用

Windows 開啟 `dist/BabelOxide-v1.0.0-windows-x64/BabelOxide-gui.exe`。本機只保留已驗證的完整資料夾；需要搬移時可用 7-Zip 壓縮整個資料夾。原始碼由 Git 維護，需要審查包時執行 `打包原始碼.cmd`。Mac 原生驗收完成後使用完整 `BabelOxide.app`，目前沒有已驗證的 Mac 成品。

1. 加入或拖放 PDF。
2. 選語言、服務與模型，在「設定 → 服務」保存網址與 Key。
3. 沿用常用設定，或選「論文雙語」「快速單語」／自訂組合，按「開始翻譯」。
4. 在佇列選擇文件，預覽原文／譯文／雙語，開啟、另存、開啟資料夾或匯出批次 ZIP。

PDF 依序處理；失敗／取消後可用原任務設定重試未完成文件，保留已完成成果。快捷鍵：Ctrl／⌘+O 加入、Ctrl／⌘+S 保存、Ctrl／⌘+Enter 開始。

請搬移完整資料夾，保留 runtime、babeldoc、native、resources 與 licenses，不能只搬 EXE。Windows 需要官方 VC++ x64 Runtime；Mac 需要 Apple Silicon、macOS 14+，未簽章／未公證，可能受 Gatekeeper 阻擋。沒有 Intel 包或保證 GPU 加速。

Windows ZIP 建議用已支援長路徑的 7-Zip 解壓到較短位置；完整引擎有深層 ONNX 檔案，一般 Python 解壓在較長目錄可能遇到 WinError 206。不要改系統政策或只解壓部分檔案。

```powershell
.\BabelOxide.exe --version
.\BabelOxide.exe --help
.\BabelOxide.exe 'input.pdf' --openai --openai-model 'your-model' --lang-in en --lang-out zh-TW --output 'output'
```

先在 GUI 保存 Key，或設定對應 `BABELOXIDE_*` 環境變數。翻譯會執行服務健康檢查，可能計入 API 用量；開啟 GUI、預覽與保存設定不呼叫翻譯 API。

## 設定與升級

- `config/config.v3.toml` 沿用既有格式及未知欄位，按保存才寫入；輸入 PDF 清單不保存。組合不包含 Key、網址、QPS／Concurrency／Timeout／Retry。
- 快取為 `cache/translations.v2.db` 與 `cache/babeldoc-home/`；模型／字型種子在 `resources/`。資料夾不可寫時顯示備援位置。
- 舊 `PDF2ZH_*` 變數可讀，新 `BABELOXIDE_*` 優先；舊 config.v3.toml 可直接沿用。已有舊備援資料夾會原地使用，避免搬動活躍 SQLite WAL 或重置設定。
- 新包只帶空 Key 範本，不攜入本機私人設定。升級既有可攜資料夾時保留自己的 config、cache 與成果。舊 CLI 檔名已改為 BabelOxide，外部腳本須更新。

## 建置與維護

```text
crates/          Rust workspace
python-worker/   最小 Worker、測試、兩平台固定依賴
resources/       圖示、空範本、資產與下載 pins、授權
script/          完整包建置、CI、完整性及本機回歸
tests/baseline/  上游行為與 PDF 基準資料
docs/           建置、架構、設計、限制與驗證狀態
```

參閱 [BUILD](docs/BUILD.md)、[ARCHITECTURE](docs/ARCHITECTURE.md)、[DESIGN](docs/DESIGN.md)、[LIMITATIONS](docs/LIMITATIONS.md) 與 [STATUS](docs/STATUS.md)。Actions 只 Build／Test／上傳完整 Artifacts，保留 7 天，不自動發布 Release。

## 授權

**AGPL-3.0-only**。這是 PDFMathTranslate-next 與 BabelDOC 的修改／重新實作衍生作品，保留原作者與第三方權利。完整條款見 [LICENSE](LICENSE)，來源與修改告知見 [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES.md)；GUI「關於 BabelOxide」亦可查看。
