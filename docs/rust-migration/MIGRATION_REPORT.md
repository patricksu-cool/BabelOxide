# Rust 移植進度

更新：2026-10-27，Asia/Taipei。分支 `feature/rust-babeldoc-hybrid`。**目前完成核心 PoC，尚未完成全面移植或正式發行。**

## 已完成

1. main 安全整合三個 maintenance 分支的 50 個提交，保留歷史與基準標籤；見 [Git 整合](GIT_INTEGRATION.md)。
2. 分析原 Python／BabelDOC 邊界、34 項 retry characterization 與 SDK 額外重試；見 [架構](ARCHITECTURE.md)、[依賴](DEPENDENCIES.md)、[行為契約](RETRY_COMPATIBILITY.md)。
3. 建立 Rust workspace：`core`、`babeldoc-bridge`、`cli`。`app` crate 尚未建立。
4. Rust CLI → 私有 stdio Worker → Rust OpenAI-compatible HTTP → BabelDOC → PDF 的真實流程已走通。Worker 不 import `pdf2zh_next`，不持有 API key、HTTP 翻譯器、應用快取或 retry。
5. 指定四頁 PDF、固定 identity 回應、同 BabelDOC／字型／引擎選項下，mono／dual 共八個頁面與整合後 main 的文字、字元座標、圖片及 96 DPI RGB 渲染皆一致；像素差為 0。這是受控排版驗證，不是真實中文翻譯品質驗收。
6. Windows suspended spawn／Job Object／kill-on-close、並行請求配對、取消、崩潰與異常退出的初步自動化驗證通過；見 [測試報告](TEST_REPORT.md)。

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
| 翻譯服務 | 僅 OpenAI-compatible PoC；其餘服務尚未接管 |
| Retry／Backoff／SDK 行為 | 已完成來源對照，尚未移植等價實作 |
| Config／舊格式遷移 | 尚未實作 |
| 持久翻譯 cache／token 統計 | 尚未實作 |
| 全部 CLI 參數／多檔批次 | 尚未實作完整相容 |
| GUI／內嵌 PDF 預覽 | 尚未實作 |
| 不同術語服務／glossary／分割策略 | 尚未完整接線 |
| 長時間、多 Worker、大型 PDF | 尚未完成驗收 |
| 最小 Python runtime／Windows 發行包 | 尚未建立新版本 |
| 獨立乾淨 Windows／安全與授權驗收 | 尚未完成 |

未刪除任何舊 Python 應用模組或 Gradio。原實作仍是功能與 retry 比對基準；不能因 PoC 成功便刪除。

目前 Python 測試／盤點腳本是開發工具，不屬於規劃中的產品 runtime。最終 runtime 依賴盤點仍需對實際新發行包驗證；[目前 metadata 盤點](PYTHON_DEPENDENCY_INVENTORY.json)不是已完成打包的證明。
