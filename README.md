# BabelOxide v1.0.0

原生 PDF 翻譯工具，保留公式、圖表與版面，支援單語及雙語輸出。Rust 負責 egui GUI、CLI、服務、設定、快取、重試與任務。Python Worker 橋接 **BabelDOC 0.6.4**。

支援 Windows x64 與 macOS 14+ Apple Silicon。兩平台的 GitHub Actions 完整包已由使用者確認可翻譯及預覽 PDF。從原始碼啟動時，請依下方步驟準備 Runtime。

## 使用

Windows 開啟完整包中的 `BabelOxide-gui.exe`，macOS 開啟 `BabelOxide.app`。需要搬移時請保留完整資料夾。原始碼由 Git 維護，需要審查包時在 Windows 執行 `打包原始碼.cmd`，macOS 執行 `打包原始碼.command`，輸出上一層的 `BabelOxide-source-review.zip`。

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

從原始碼執行須先安裝 Rust／Cargo 與 uv。`cargo build --release --locked` 只編譯 Rust；在 Windows 雙擊根目錄 `啟動開發版.cmd`，macOS ARM64（14+）雙擊 `啟動開發版.command`。首次啟動會沿用正式 Builder，下載並驗證固定的 Python、BabelDOC 0.6.4、PDFium、模型與字型，再啟動原始碼 GUI；不需要先下載完整 Artifact，後續重用本機 Runtime／快取。

```powershell
.\啟動開發版.cmd --build --prepare  # 編譯、準備並檢查，不開啟 GUI
.\啟動開發版.cmd --check            # 檢查既有 Runtime，不呼叫翻譯 API
.\啟動開發版.cmd                    # 啟動 GUI（macOS 改用 ./啟動開發版.command）
.\啟動開發版.cmd --cli -- --version # 使用相同 Runtime 啟動 Release CLI
```

開發環境與獨立發行包的差異、指定既有 Runtime 及檢查日誌見 [BUILD](docs/BUILD.md#開發驗證)。準備／檢查不修改私人設定，也不呼叫翻譯服務；實際按下翻譯仍使用你選擇的服務。

```text
crates/          Rust workspace
python-worker/   最小 Worker、測試、兩平台固定依賴
resources/       圖示、空範本、資產與下載 pins、授權
script/          完整包建置、CI、完整性及本機回歸
tests/baseline/  上游行為與 PDF 基準資料
docs/            建置、架構與限制
```

Actions 只接受手動觸發，建置包與 CI 日誌保留 7 天。參閱 [建置](docs/BUILD.md)、[架構](docs/ARCHITECTURE.md)與[已知限制](docs/LIMITATIONS.md)。

## 授權

**AGPL-3.0-only**。這是 PDFMathTranslate-next 與 BabelDOC 的修改／重新實作衍生作品，保留原作者與第三方權利。完整條款見 [LICENSE](LICENSE)，來源與修改告知見 [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES.md)；GUI「關於 BabelOxide」亦可查看。
