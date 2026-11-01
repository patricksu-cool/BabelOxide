# BabelOxide 1.0.0 驗證狀態

更新：2026-11-01（Asia/Taipei）。工作分支 `feature/babeloxide-v1`；Windows 可用包已完成本機驗證。**macOS 原生驗收仍未完成，因此未建立正式 `v1.0.0` tag、未合併 main、未 Push／發布 Release。**

| 平台 | 實際結果 |
| --- | --- |
| Windows x64 | Rust 1.99.1 GNU；完整 runtime、CLI／GUI、搬移與 PDF 回歸通過。仍需系統官方 VC++ v14 x64 Runtime；未測乾淨 VM |
| macOS arm64、14+ | 原生來源／完整 .app builder／84 個固定 wheel／Mach-O 檢查與 CI 已實作。此機是 Windows，未跑 Mac build、import、GUI，沒有已合格的 Mac 成品 |
| GitHub Actions | Node24 官方 Actions、唯讀權限、雙 runner、完整 ZIP、7 天 Artifacts 靜態契約通過；兩個遠端 job 均未觸發／驗證 |

## 啟動與產物

- GUI：`dist/BabelOxide-v1.0.0-windows-x64/BabelOxide-gui.exe`；CLI：同目錄 `BabelOxide.exe --help`。
- 完整包：`dist/BabelOxide-v1.0.0-windows-x64.zip`，**698,080,955 bytes**；另有原始碼 `dist/BabelOxide-v1.0.0-source.zip`。
- 請保留整包；建議 7-Zip 解壓到較短路徑。深層 ONNX 檔案曾使一般 Python 解壓遇到 WinError 206；既有 7-Zip 完成解壓後，全檔查核通過，沒有修改系統政策。
- 包內 **13,127 項 manifest、1,249,541,691 bytes**（不含 manifest 本身）。程式碼建置提交 `77a5c38f3b4086fdf491a0bca59522e2fbf54dfb`；其後只有交付文件更新。`BUILD_INFO.release_ready=false` 保留。

Windows ZIP SHA256：

```text
fa225056d2e2f74280c5eb9a4acfdf29d7f1248036653f998bf72eb087994358
```

## 檢查結果

| 項目 | 結果與範圍 |
| --- | --- |
| Rust | **301 passed、0 failed、1 預設 ignored**；fmt、Clippy `-D warnings`、Release workspace build 通過 |
| Python／封裝 | Worker **35**、Windows Builder **35**、Portable Builder **7**、Windows 來源封裝 **22**；Ruff 通過 |
| 真引擎／原生庫 | BabelDOC **0.6.4**、Python **3.13.16**、Pillow **13.0.0**；Windows 85 個鎖定 wheel、182 項模型／字型／CMap／tokenizer 資產；4 個受控引擎案例及明確執行的原生 PDFium **1** 項通過 |
| 完整包 CI | 同一 `script/ci.py --check --build` 完成 native imports、資產還原、CLI／GUI 版號、本機 Mock mono+dual、PDFium、YAML、manifest 與 ZIP CRC |
| 可攜性 | 將實際 ZIP 解壓到原始碼外的中文空格路徑，全檔集合／size／SHA256 相符；包內 runtime、可攜 config／cache、非來源 CWD 的 CLI 四頁翻譯 exit 0；不依賴開發 .venv |
| 指定論文 | 相對 0.6.4 基準，mono／dual 各四頁的文字、逐字座標、圖片、幾何與嵌入字型相同；八頁 Poppler 96 DPI RGB 像素差 **0** |

升級前亦已比較 0.6.3／0.6.4，相同固定 identity 回應下上述結構與像素一致；另外覆蓋 Latin／CJK 行距及異常大頁框的像素預算。單次暖機測得 31.44→26.76 秒、1688.41→1652.28 MiB，僅為該次觀察，並非統計效能結論。

論文實際位置為 `Downloads/論文/High_speed_buffer_latch_ISCAS03.pdf`，SHA256 `d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f`。PDF 本身、個人設定／Key 與翻譯結果均不在成品或原始碼包。

## Release GUI 實測

實際 ZIP 的 GUI 已操作：Ctrl+O 多選含中文空格檔名、原文預覽、內建組合、自訂保存／重新命名／重開；兩份 PDF 的逐份佇列與串列完成；翻譯中取消→清理→原設定重試；HTTP 400→重試成功。重複提交後，先前四份成果的 SHA256 保持不變。

單語／雙語切換、翻頁、縮放、開啟成果資料夾、SumatraPDF 開啟雙語、中文空格另存及位元組相同已驗證。約 790px 寬的工作區／設定／預覽頁籤可操作，網址／Key 遮罩；Ctrl+S 保存後重開，模型／合成 Key／QPS 與已重新命名組合保留，input_files 清空。About 的版號、可再散布權利、無擔保、來源署名及完整 AGPL 可見。

最終 GUI 兩次退出皆 **0**，387 次請求全部送往本機 fixture（含一次受控 HTTP 400），日誌無合成 Key。未呼叫付費翻譯服務；identity 測試不代表真實翻譯品質。真實 OS 拖放、高 DPI 125%／150%／200%、全部服務與真實唯讀 ACL 未逐項操作；拖放及 fallback 有受控測試，不能外推為手動驗收。

## 清理、相容與授權

移除 **18 份**過時／重複追蹤文件，契約整併至 BUILD／ARCHITECTURE／LIMITATIONS／DESIGN；保留 frozen retry oracle、PDF 行為基準、必要 fixtures、Worker／locks 與授權原文。另清除本次 **10 項**過渡包／builder 暫存及一份隔離測試副本，保留驗證紀錄、最終成品、既有舊包與使用者資料。

新 `BABELOXIDE_*` 優先，舊變數與 config.v3.toml 可讀；舊 fallback 的 config／cache／WAL 原地使用，無效遷移不發布。Presets 排除 Key／網址／執行限制；UI／更名不改 provider Retry／Backoff、Timeout、QPS、Concurrency、cache identity 或 IPC 政策。SDK 與平台界線見 [LIMITATIONS](LIMITATIONS.md)。

完整 [LICENSE](../LICENSE) 與 PDFMathTranslate-next v2.9.0 原文一致；[THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md)、GUI 及引擎 LICENSE 保留上游來源、修改日期與 BabelDOC 的 `Copyright (C) 2024 <funstory.ai limited>`。Rust 全目標 439 項另補 33 個元件的固定來源 notices；**dispatch 0.2.0 授權原文仍缺**（manifest 宣告 MIT），objc2 的 Apple SDK 說明及 Mac 原生庫散布條件亦保留待審。沒有正式法務認證或已公開對應原始碼的宣稱。

原始 main `71e80c2` 保留於 `codex/babeloxide-before-v1`，引擎穩定 tag 為 `codex/engine-0.6.4-20261101`。未切換／調用附件指定的輔助模型，也未宣稱完成該模型分工。精確本機證據位於忽略版控的 `.test-output/babeloxide/` 與 `.test-output/ci/windows-x64/`；Git 保存開發歷史。
