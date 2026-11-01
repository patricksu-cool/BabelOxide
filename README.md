# PDFMathTranslate-next：Rust + BabelDOC

Windows 個人使用的 PDF 翻譯工具，保留公式、圖表與版面，輸出單語與雙語 PDF。Rust 負責原生 GUI、CLI、翻譯 API、設定、重試、快取及預覽；Python 只保留 BabelDOC 引擎與最小 Worker，無 Gradio。

## 直接使用

完整自用版本位於 `dist/gui-ux-20261101/`，舊 `dist/final/` 與個人資料保留。執行 **`pdf2zh-gui.exe`**，選擇翻譯服務，設定 Base URL／模型／Key 與語言，加入 PDF，按「開始翻譯」。可切換原文、譯文與雙語預覽，翻頁、縮放、取消或另存結果，並匯出整個批次的 PDF ZIP。

請搬移完整資料夾，包含 runtime、babeldoc、resources 與 native；不要只搬 EXE。不需另外安裝 Python、Rust、uv 或 BabelDOC。ONNX Runtime 部分模組仍需要系統的 Microsoft Visual C++ x64 Runtime，未宣稱零系統依賴或乾淨 VM 驗收。

- 設定：`config/config.v3.toml`，按「保存設定」才保存本次編輯，輸入 PDF 清單不作為預設。
- Rust 翻譯快取：`cache/translations.v2.db`。
- BabelDOC 可變資料：`cache/babeldoc-home/`；模型／字型種子保留在 `resources/`。
- 程式資料夾不可寫時，畫面／CLI 會顯示使用者資料備援位置，不覆寫已有設定。
- 執行包的 `config/config.example.toml` 是空 Key 範本，原始碼範本在 `resources/config.example.toml`；不會自動覆蓋設定。

CLI 範例（執行版本目錄內）：

```powershell
.\pdf2zh.exe --help
.\pdf2zh.exe 'input.pdf' --openai --openai-model 'your-model' --lang-in en --lang-out zh-TW --output 'output'
```

先在 GUI 保存 Key，或使用對應的 `PDF2ZH_*` 環境變數。避免將 Key 直接放在命令列或日誌。開始翻譯會執行主／術語服務健康檢查，可能計入 API 用量；只開啟 GUI、預覽與保存設定不呼叫 API。

## 維護

```text
crates/          Rust workspace，7 個 crates
python-worker/   BabelDOC 邊界、測試、固定執行期依賴
resources/       資產 lock、設定範本
script/          組包、依賴查核與本機回歸工具
tests/baseline/ 原 Python 的關鍵行為與 PDF 基準資料
docs/           架構、建置、故障排解與最終結果
```

詳見 [架構](docs/ARCHITECTURE.md)、[開發與建置](docs/DEVELOPMENT.md)、[服務相容性](docs/PROVIDERS.md)、[Python 依賴](docs/PYTHON_DEPENDENCIES.md)、[疑難排解](docs/TROUBLESHOOTING.md) 與 [最終狀態](docs/FINAL_STATUS.md)。原始碼使用 AGPL-3.0-only；執行包保留第三方授權與 notices。本專案不包含 GitHub Release 或自動 Push 流程。
