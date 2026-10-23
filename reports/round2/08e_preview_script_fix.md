# 預覽 JavaScript 解析修復

新增 Node.js 檢查先重現 `Unexpected token ';'`（1 failed）：Gradio 要求可呼叫的函式表達式，原腳本是帶分號的立即執行語句。改成回呼函式後，真實瀏覽器又證實原本未執行的 worker URL 改寫會將 PDF.js 導向無法載入的 GitHub raw 模組，導致空白 PDF。因此移除該失效 workaround，保留 Gradio PDF 原有 worker 與 fetch，縮放邏輯仍在。

最終 Node 測試同時編譯並在受控 DOM 替身下呼叫函式，確認保留原始 `window.fetch`。資源測試 **3 passed、2 warnings**；ruff 與 `git diff --check` 通過。無 Node 的環境會明確 skip 這項可選語法測試，本機 Node 24.20.0 已實際執行。

瀏覽器預載同一頁測試 PDF，使用相同 CSS、PDF 元件和頁面腳本，成功顯示正文、頁碼與控制項，錯誤記錄為空。先前用瀏覽器檔案選擇器上傳的工具等待異常長（約 771 秒），不能據此宣稱產品上傳效能已通過；最後採預載 harness 隔離驗證腳本與 worker。完整 GUI 的 HTTP 上傳／翻譯測試仍於最終回歸重跑。

CSS／JS 抽出時保留的原有尾端空白造成上一個 Commit 的 diff 檢查警告，本次一併移除，沒有改寫 CSS 規則。
