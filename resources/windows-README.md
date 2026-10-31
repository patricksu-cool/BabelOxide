# PDFMathTranslate：Windows 個人使用版

直接執行 `pdf2zh-gui.exe`。加入 PDF，選擇翻譯服務，輸入自己的 Base URL、模型與 API Key，設定原文／目標語言後開始翻譯。可預覽原文、單語及雙語，翻頁、縮放、取消或另存結果。

按「保存設定」後保存在 `config/config.v3.toml`；`config/config.example.toml` 是空 Key 範本，不會自動覆蓋設定。Rust cache 位於 `cache/translations.v2.db`，BabelDOC 可變資料位於 `cache/babeldoc-home/`。安裝目錄不可寫時會顯示使用者資料備援位置。

CLI 為 `pdf2zh.exe`，使用 `--help` 查看選項；不帶參數會開啟 GUI。已有 GUI 保存設定時，可執行 `pdf2zh.exe input.pdf --output output`。Key 也可透過對應的 `PDF2ZH_*` 環境變數提供，避免把 Key 放進命令列或分享設定檔。

請搬移完整資料夾；不需安裝 Python、Rust、uv 或 BabelDOC，也不需指定 Worker。包內模型與字型是必要資源，首次翻譯會初始化可寫 cache。程式開始翻譯會執行服務健康檢查，可能計入 API 用量；取消與 timeout 無法保證遠端服務沒有計費。

系統須具備 Microsoft Visual C++ v14 x64 Runtime（ONNX Runtime 使用），需要時參考 [官方說明](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170)。本包沒有從 System32 複製 DLL，也未宣稱無系統依賴或乾淨 VM 驗收。只供個人使用；保留 `licenses/` 與第三方 notices。

可移植性與測試詳情見原始碼專案的 `docs/FINAL_STATUS.md`。遇到 API／字型／DLL／預覽問題，參考 `docs/TROUBLESHOOTING.md`。執行包沒有私人 Key、個人設定或測試 PDF。
