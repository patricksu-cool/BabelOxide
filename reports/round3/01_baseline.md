# 第三輪基準

日期：2026-10-23（Asia/Taipei）。分支 maintenance-round3，起點 1345fc2，原工作目錄乾淨，保留第二輪全部歷史。

已閱讀 reports/round2/10_final_report.md、09_final_regression.md、06_dependency_security.md、08g_locked_windows_build.md。

- 完整 Pytest：211 passed、1 failed、67 warnings，12.25 秒。失敗：test_independent_writers_use_distinct_temporary_files，Windows 並行 os.replace 出現 WinError 5；不能沿用第二輪 212 passed 的結論。
- Retry／Backoff：34 passed，3.45 秒。
- 指定 PDF：CLI exit 0，本機 identity 請求 107 次、36 批次，產生單語／雙語輸出，合成金鑰未出現在日誌。見 01_baseline_pdf.json。
- 原始碼、文件、資源清單與體積：01_baseline_inventory.json（Git 追蹤檔案，排除 venv／編譯快取／個人設定）。
- 最初受限制環境在 Windows asyncio socketpair 停滯，改在隔離 USERPROFILE／TEMP／PDF2ZH_APP_DIR 下執行；未改產品程式，沒有呼叫付費 API。
- 文件、格式與安全稽核紀錄在後續報告補入。

所有程式修改均在本基準建立之後開始。
