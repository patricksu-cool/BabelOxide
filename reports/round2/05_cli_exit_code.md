# 第二輪第五階段：CLI 翻譯失敗退出碼（2026-10-23）

## 原因與重現

`do_translate_file_async(settings, ignore_error=True)` 已逐檔統計失敗次數：遇到 `error` 事件或例外會加一，仍繼續處理其他檔案。但 `main.main()` 原本丟棄回傳值，無條件回傳 0，導致部分及全部翻譯失敗時 CLI 仍顯示成功。

先新增成功、部分失敗、全部失敗的測試；修改前執行結果 **4 passed、2 failed**，兩個失敗案例都實際收到 `SystemExit(0)`，預期是非零。高階翻譯流程的三種逐檔計數測試在修改前已通過，確認錯誤來源是 CLI 入口的回傳路徑。

## 修復及相容性

`pdf2zh_next/main.py` 保留 `ignore_error=True` 的逐檔繼續處理方式，接收既有失敗計數：零失敗回傳 0，只要有失敗就回傳 1。未修改翻譯引擎、Timeout、Retry／Backoff、限流或並行設定。GUI 分支仍在 CLI 翻譯呼叫之前直接回傳 0。

這是預期的 CLI 對外行為變更：舊版即使部分輸出成功仍可能回傳 0；新版只要任一檔案失敗就回傳 1。依賴舊退出碼的批次腳本應檢查產物及失敗日誌，並將 1 視為需要處理的部分／全部失敗。失敗計數不直接當退出碼，以免超過 Windows／POSIX 退出碼範圍。

## 驗證

- `tests/test_cli_exit_code.py` **7 passed**：高階流程逐檔計數 3 種、CLI 成功／部分／全部失敗 3 種、GUI 分支 1 種。
- 真正 CLI 子行程連本機 OpenAI 相容 Mock，使用一份有效及一份故意損壞的測試 PDF：成功 exit 0 並產生 2 份 PDF；混合輸入 exit 1，仍產生有效檔的 2 份 PDF；僅損壞檔 exit 1，產生 0 份 PDF。三種情況日誌均未含合成金鑰；未呼叫真實服務。
- 重新打包本機 PyStand EXE；從不同工作目錄執行 `--version` 回傳 0，損壞 PDF 回傳 1，錯誤已記錄。
- 全部自動測試 **160 passed、67 warnings、0 failed**；`ruff check .`、相關格式檢查、`git diff --check` 通過。GUI 的完整上傳與下載流程已於第四階段測過；本階段 GUI 分支測試確認該入口不執行 CLI 翻譯。
