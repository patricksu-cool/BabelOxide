# BabelOxide v1.0.0 — Windows x64

開啟 `BabelOxide-gui.exe`，加入 PDF、選擇語言與常用組合，在「設定 → 服務」保存網址／模型／Key，按「開始翻譯」。可預覽原文／單語／雙語、取消、重試未完成文件、開啟資料夾、另存或匯出批次 ZIP。CLI 為 `BabelOxide.exe --help`。

請保留完整資料夾，包含 runtime、babeldoc、resources、native、licenses 及 provenance。不需要另外安裝 Python、Rust 或 uv；ONNX Runtime 仍需要官方 Microsoft Visual C++ x64 Runtime。不要從 System32 複製 DLL。

設定為 `config/config.v3.toml`，按保存才寫入；快取為 `cache/`。舊格式及必要的 `PDF2ZH_*` 變數可沿用，新變數使用 `BABELOXIDE_*`。資料夾不可寫時程式會顯示備援位置。執行包沒有私人 Key、PDF 或翻譯成果。

BabelDOC 引擎鎖定 0.6.4。`BUILD_INFO.json` 記錄來源提交與建置驗證範圍。平台與服務限制見原始碼的 `docs/LIMITATIONS.md`。

本程式為 AGPL-3.0-only 自由軟體，不提供擔保。請查看 `licenses/project/LICENSE`、`licenses/project/THIRD_PARTY_NOTICES.md`、其他 `licenses/` 及 `THIRD_PARTY_COMPONENTS.json`。GUI 的「關於 BabelOxide」保留來源與授權入口。
