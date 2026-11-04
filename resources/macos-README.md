# BabelOxide v1.0.0 — macOS ARM64

需要 Apple Silicon 與 macOS 14 或以上。請保留完整 `BabelOxide.app`，在 Finder 開啟；CLI 位於 `BabelOxide.app/Contents/MacOS/`。不需要另裝 Python、Rust 或 BabelDOC。

本包未簽章、未公證，Gatekeeper 可能阻擋啟動。請先驗證來源；公開散布前由擁有者完成適當簽章與公證。本工具不移除 quarantine、不停用系統保護，也不使用簽章金鑰。

已內附固定 Python、BabelDOC 0.6.4、PDFium、ONNX Runtime 與必要字型／模型；以 CPU 為保底，沒有承諾 CoreML 加速或 Intel 支援。`BUILD_INFO.json` 與 `NATIVE_DEPENDENCIES.json` 記錄實際檢查；版本 smoke 不代表已完成桌面互動驗收。

設定與快取使用與 CLI 共用的位置；應用程式資料夾不可寫時，改用使用者 Library/Application Support 的獨立位置。個人 Key 不隨包提供。授權與第三方文件在 `Contents/MacOS/licenses/`。
