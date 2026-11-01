# BabelOxide 實作狀態

更新：2026-11-01（Asia/Taipei）。工作分支 `feature/babeloxide-v1`；原始 main 基準 `71e80c2` 保留於 `codex/babeloxide-before-v1`。尚未完成 v1.0.0 驗收，不建立正式版本標籤。

## 引擎升級

BabelDOC 已鎖定 **0.6.4**，官方 wheel SHA256 `e7dcdd5b8213f657af1df68e329f92d0534c9b94c53fcd82e9e04c52060cb7d0`。已讀取官方 0.6.3→0.6.4 source diff；依賴宣告及 embedding assets metadata 不變，沿用經驗證的 182 項資產與 Pillow 13.0.0 安全更新。舊 profile 從 Git 基準回復，不作為新版打包候選。

| 檢查 | 本機結果 |
| --- | --- |
| Rust workspace | 292 passed、0 failed、1 預設 ignored；fmt、Clippy `-D warnings`、Release build 通過 |
| Worker／Windows builder | 35／35 passed |
| 受控引擎案例 | 4 passed：版號、混排行距、旋轉與普通頁面、超大頁框低像素預算及座標換算 |
| 指定四頁論文 | 0.6.3／0.6.4 固定 identity 回應；mono／dual 文字、逐字座標、圖片、頁面幾何及嵌入字型一致；八頁 Poppler 96 DPI RGB 像素差 0 |
| 耗時／峰值記憶體 | 單次暖機條件下 31.44→26.76 秒、1688.41→1652.28 MiB；不視為統計效能結論 |
| Release GUI | 原生選檔、四頁 PDFium 預覽、翻譯真實進度至完成、雙語預覽及關閉已操作；取消／錯誤／復原另由既有 IPC／任務測試覆蓋 |
| 完整 Windows 候選 | `dist/babeloxide-engine-064/`，13,071 項 manifest、1,248,878,152 bytes；隔離 runtime、資產還原、原生 import、CLI／GUI 版本 smoke 通過 |

論文位於指定檔名的 `Downloads/論文/` 子目錄，SHA256 `d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f`。使用本機固定回應，未呼叫付費服務。詳證保留於忽略版控的 `.test-output/babeloxide/`，PDF 不納入原始碼或成品。

## UI/UX

已實作工作區／設定分頁、常用組合與白名單保存、逐份真實狀態、原設定重試、既有成果保護、開啟資料夾、固定開始按鈕、窄視窗預覽頁籤及快捷鍵。Rust workspace **296 passed、0 failed、1 預設 ignored**；GUI 68 項、fmt、嚴格 Clippy、Release build 通過。

Release GUI 已實際操作：組合切換／自訂保存／重開；Key、模型、QPS 保留；Ctrl+O 多選中文空格檔名；HTTP 400 → 原設定重試；Worker 載入中取消 → 再次重試 → 兩份 PDF 完成；保留失敗／取消歷史與逐份狀態；窄至約 790px 視窗的工作區與設定分頁。重新命名、原生 drop、輸出保護另有自動測試。125%／150%／200% 真實 Windows DPI、真實唯讀 ACL 與全部服務未逐項操作，仍待最終驗證。

## 品牌與設定相容

產品／七個 owned crates、CLI／GUI binary、環境變數主名稱與 builder 已統一為 BabelOxide **1.0.0**。舊 `PDF2ZH_*` 環境變數、`config.v3.toml`、home 遷移及既有 fallback 的 config／cache／WAL 保留；無效舊設定在發布新檔前拒絕。Rust **301 passed、0 failed、1 預設 ignored**；fmt、嚴格 Clippy、Release、Worker／Builder 契約檢查通過。Windows EXE ProductName／Version／OriginalFilename、CLI 版號、原創視窗圖示與 About 三個授權頁籤已實際檢查，GUI 關閉 exit 0、未發出 API 請求。

原始碼 CMD 封裝器已更名、納入來源告知／鎖檔／原創圖示，並固定二進位圖示雜湊；22 項隔離整合案例通過，包含中文與特殊路徑、內容排除、憑證拒絕、圖示竄改與原子替換。

## 建置管線

已建立雙 runner 的 `.github/workflows/build.yml`、公開下載與 hash pins、Mac 84 個固定 wheel、Mac 原生開檔／結果開啟／設定 fallback／PDFium arm64 loader，以及完整 `.app` builder。Node24 Actions、唯讀權限、不發布、7 天 Artifacts 的 YAML 契約檢查通過。

本機執行同一 `script/ci.py` 入口：Rust **298 passed**（另 1 預設 ignored）、Worker 35、Windows Builder 35、Portable Builder 7、引擎案例 4、明確原生 PDFium 1、fmt／Clippy／Ruff、完整包 relocation／native import／資產還原／本機 Mock mono+dual／manifest／ZIP CRC 全部通過。過渡驗證 ZIP 為 **697,876,167 bytes**；當時 workspace 仍是 2.9.0，並非最終 v1.0.0 成品。

**Mac blocker：此機為 Windows，未執行原生 Mac build／dylib import／GUI；未 Push 或觸發遠端 Actions。** 已驗證官方 Mac PDFium 的 arm64 Mach-O 與系統依賴、Python archive SHA256，以及 macOS 14+ 的整套 wheel 解析；這些不等於 Mac 原生驗收。Mac job 有缺失時明確失敗，沒有 placeholder 或 continue-on-error。

文件精簡、完整依賴授權審查與最終整合驗收仍在後續階段。Windows ONNX Runtime 仍需官方 VC++ x64 Runtime。沒有 Push 或發布 Release。
