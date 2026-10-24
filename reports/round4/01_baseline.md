# 第四輪基準

日期：2026-10-24（Asia/Taipei）。起點 `0147d19`，新分支 `maintenance-round4`；原工作目錄乾淨，保留全部歷史。已閱讀第三輪全部 Markdown 報告及測試、安全、套件與 PDF 證據。

| 項目 | 本輪實測 |
| --- | --- |
| 完整 Pytest | **237 passed，0 failed，12.74 秒** |
| Retry / Backoff | **34 passed，3.19 秒** |
| Config／GUI 網路／PDF 預覽／依賴定向測試 | 82 passed，5.67 秒（與完整測試重疊，不相加） |
| `uv pip check` | 174 個已安裝套件相容 |
| CLI／第三輪穩定 EXE `--version` | 2.9.0，exit 0；使用隔離設定 |
| GUI 啟動、HTTP config、正常 close | 200、337 元件、127.0.0.1、share=False、無互動圖片輸入 |
| 指定 PDF 第一頁 CLI | exit 0、33 次本機請求（11 批）、mono／dual 皆產生，合成 Key 不在日誌 |
| 鎖檔安全掃描 | **未通過**：208 套件、47 筆公告，Pillow 41、filelock 舊 Python 分支 6 |

版本與相依限制見 `01_environment.json`：Python 3.13.16、Gradio 6.30.0、Pillow 12.3.0、BabelDOC 0.6.3、PyMuPDF 1.26.7、gradio-i18n 0.3.4。未沿用第三輪測試數字作為本輪結果。

原始日誌與 JUnit 在忽略追蹤的 `.test-output/round4/`。受限執行在 Windows asyncio 初始化停滯，停止該測試後以相同程式、隔離 USERPROFILE／HOME／TEMP／PDF2ZH_APP_DIR 在允許本機 socket 的執行環境重跑。GUI harness 使用 Gradio 的 debug=False／prevent_thread_lock=True，以便在 finally 呼叫 close；正式產品啟動參數未改動。瀏覽器互動、完整 PDF 三版本比較與重新組包留在後續階段，不以這份基準宣稱全部通過。

只有 loopback 固定回應服務，無真實 provider 請求。憑證盤點只記錄欄位是否非空，非空不代表有效或獲授權；未輸出、修改或提交憑證值。未覆寫第三輪交付包或使用者設定。
