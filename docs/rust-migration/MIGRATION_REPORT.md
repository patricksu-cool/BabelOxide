# Rust 移植進度

更新：2026-10-30，Asia/Taipei。分支 `feature/rust-babeldoc-hybrid`。**已完成核心 PoC、Rust 翻譯服務及設定／快取／CLI／任務管理的離線整合；原生 GUI、預覽及正式發行仍在實作。**

## 已完成

1. main 安全整合三個 maintenance 分支的 50 個提交，保留歷史與基準標籤；見 [Git 整合](GIT_INTEGRATION.md)。
2. 分析原 Python／BabelDOC 邊界、34 項 retry characterization 與 SDK 額外重試；見 [架構](ARCHITECTURE.md)、[依賴](DEPENDENCIES.md)、[行為契約](RETRY_COMPATIBILITY.md)。
3. 已驗收 Rust workspace：`core`、`babeldoc-bridge`、`cli`、`config`、`tasks`。`app` 與 `pdf-preview` 正在實作，尚未列為通過。
4. Rust CLI → 私有 stdio Worker → Rust OpenAI-compatible HTTP → BabelDOC → PDF 的真實流程已走通。Worker 不 import `pdf2zh_next`，不持有 API key、HTTP 翻譯器、應用快取或 retry。
5. 指定四頁 PDF、固定 identity 回應、同 BabelDOC／字型／引擎選項下，mono／dual 共八個頁面與整合後 main 的文字、字元座標、圖片及 96 DPI RGB 渲染皆一致；像素差為 0。這是受控排版驗證，不是真實中文翻譯品質驗收。
6. Windows suspended spawn／Job Object／kill-on-close、並行請求配對、取消、崩潰與異常退出的初步自動化驗證通過；見 [測試報告](TEST_REPORT.md)。
7. 16 種服務的 HTTP／CLI 執行、typed 錯誤、分層 retry、可注入時間與亂數、QPS 與 usage 統計已移到 Rust；原 Python 仍保留。各案例及未證等價的細節見 [測試矩陣](PROVIDER_TEST_MATRIX.md) 與 [Astra 審查](PROVIDER_REVIEW.md)。設定與 CLI 現已接線 24 個服務選項（含別名）。
8. Bridge支援獨立main／term service、動態outstanding容量、有界進度佇列；已驗證200個同時在途請求、慢進度消費者與真正CLI取消後程序樹退出。Windows清理等待Job ActiveProcesses歸零，避免只發終止命令就回傳。
9. 第5階段再次以相同SHA-256原檔完成四頁翻譯；與main基準的mono／dual共八頁文字、字元座標、圖片與96 DPI RGB渲染一致，見 [本輪PDF數據](PHASE5_PDF_COMPARISON.json)。原檔已移至下載目錄的「論文」子目錄，未改用已翻譯的PDF作輸入。
10. 第 6 階段完成 235 項設定描述、舊 TOML 相容與不覆寫遷移、程式目錄原子保存、SQLite 持久快取、clap 多檔 CLI 與共用 TaskManager。167 項 Rust 測試、fmt、Clippy 及 26 項 Worker 測試通過；詳見 [設定](CONFIG_MIGRATION.md)、[快取](CACHE_COMPATIBILITY.md)、[任務審查](TASKS_REVIEW.md)。舊快取缺少完整來源身分，只有明確 provenance binding 才重用，不自動混用不同端點結果。
11. 新 CLI 再次完成四頁 PDF：105 個固定回應請求、36 個 batch、68 個進度與單一 finish。mono／dual 共八頁文字、座標、圖片及渲染像素差皆為 0，見 [第 6 階段比對](PHASE6_PDF_COMPARISON.json)。這仍是受控 identity 回應驗證。

## 已修正的整合問題

- 使用 Rust bundled `rust-lld` 與 self-contained GNU runtime，避免本機預設 linker 所產生的混合 DLL import table 啟動存取違規；不安裝新工具、不改全域 PATH、不降版依賴。
- 保留跨 select 的部分 frame，避免 HTTP task 完成時取消 reader future 而遺失資料。
- 觀察 child／writer 退出，避免孫程序持 stdout 時一直等待 EOF。
- 明確送出舊 app 的 `remove_non_formula_lines=True`，保留引擎不同的預設行為。
- stdin/stdout 協定皆使用私有、不可繼承的 duplicate；標準 stdin 指向 NUL，stdout 指向 stderr。原本真實 Worker 在 NumPy 原生模組載入時停滯；只改為私有 stdin 後，一頁及完整四頁均成功。Python 堆疊不能單獨證明原生鎖的細節，因此不將推測寫成已證實的 NumPy 缺陷。
- debug 模式可保留最多 16 MiB Worker stderr 與堆疊；預設不保存文件診斷內容。

## 尚未完成

| 功能 | Rust 現況 |
| --- | --- |
| 翻譯服務 | 16 服務核心及 24 個設定／CLI 選項已有離線測試；GUI 接線與真實服務驗收仍待完成 |
| Retry／Backoff／SDK 行為 | 已實作並驗證主要分層政策；write／pool與部分SDK細節的界限仍列於測試矩陣 |
| Config／舊格式遷移 | 已實作並測試；Tencent 自動憑證鏈等缺口見 CONFIG_MIGRATION |
| 持久翻譯 cache／token 統計 | 已實作；舊庫受保護，取消等待 blocking writer 清理；證據界限見 CACHE_COMPATIBILITY |
| 全部 CLI 參數／多檔批次 | 235 項設定與批次已接線；warmup／離線資產操作尚未接通 |
| GUI／內嵌 PDF 預覽 | 正在實作 eframe／PDFium；尚未編譯及互動驗收 |
| 不同術語服務／glossary／分割策略 | Bridge／worker已有接線與邊界測試，完整CLI／GUI及真PDF各選項驗收尚未完成 |
| 長時間、多 Worker、大型 PDF | 尚未完成驗收 |
| 最小 Python runtime／Windows 發行包 | 尚未建立新版本 |
| 獨立乾淨 Windows／安全與授權驗收 | 尚未完成 |

未刪除任何舊 Python 應用模組或 Gradio。原實作仍是功能與 retry 比對基準；不能因 PoC 成功便刪除。

目前 Python 測試／盤點腳本是開發工具，不屬於規劃中的產品 runtime。最終 runtime 依賴盤點仍需對實際新發行包驗證；[目前 metadata 盤點](PYTHON_DEPENDENCY_INVENTORY.json)不是已完成打包的證明。
