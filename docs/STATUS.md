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

## 尚待完成

UI/UX、跨平台 workflow／macOS 組包、BabelOxide 品牌、清理與法律入口、最終整合驗收仍在後續階段。macOS 未在此 Windows 主機上驗證；Windows ONNX Runtime 仍需官方 VC++ x64 Runtime。沒有 Push 或發布 Release。
