# 移植測試報告

更新：2026-10-27，Asia/Taipei。Python 穩定基準 `d18165e`，Rust 開發分支 `feature/rust-babeldoc-hybrid`。本報告目前驗收核心 PoC；不宣稱全面功能等價。

## 本次實測

| 檢查 | 結果 |
| --- | --- |
| 整合後 main Python 基準 | 253 passed；詳 GIT_INTEGRATION |
| 新 Worker 加原 Python 完整 pytest | **269 passed，20.39 秒，0 failed／skipped** |
| Rust unit tests | core 10、bridge framing 3，全部通過 |
| Rust process integration tests | 8 passed；含真正 Windows child／grandchild handle 退出驗證 |
| Rust Clippy／format、Python Ruff | 通過；Clippy 使用 all-targets 與 -D warnings |
| Rust CLI `--version` | `pdf2zh 2.9.0`，exit 0，不啟 Python |
| 指定 PDF debug 一頁 PoC | exit 0；31 loopback API requests、11 batch；mono／dual 皆產生 |
| 指定 PDF 全四頁 PoC | exit 0；105 loopback API requests、36 batch；mono／dual 皆產生 |
| 完整 PDF 與 main 比對 | 兩種輸出皆 4 頁；文字、字元座標、圖片相同；8 頁渲染像素差 0 |

Rust 指令：`cargo test --workspace --locked`。Python 指令：`.venv/Scripts/python.exe .test-output/rust-migration/run_isolated.py pytest -q`。Windows asyncio／loopback 使用允許本機通訊的執行環境，HOME／USERPROFILE／TEMP／設定目錄隔離；未使用付費或使用者憑證。

原始證據：`.test-output/rust-migration/rust-poc-tests.log`、`poc-python-full.xml`、`poc-private-stdin/`、`poc-full-verified/`。[PDF 比對數據](POC_PDF_COMPARISON.json)使用 Poppler 96 DPI RGB，以及 PyMuPDF 的文字、逐字元 bbox 與圖片 digest／bbox。人工檢視第二頁；既有公式附近的重疊仍與基準一致，未修改 BabelDOC 排版以掩蓋差異。

原 main fixture 共 107 次請求，Rust PoC 為 105 次；原 translator 工廠有忽略 cache 的 `translate("Hello")` health check（`translator/utils.py:73`），PoC 尚無完整初始化健康檢查。請求數差異不作 retry 等價證明。

## 已涵蓋的邊界

- UTF-8、多個 frame、分段 frame、select 中斷保留資料、超大／不完整／錯版本 JSON。
- 同步 callback 的逆序回覆、取消解除等待、Rust 錯誤阻止假成功、重複／未知 ID。
- Python／CRT／Win32 stdout 診斷隔離；標準 stdin EOF 與私有協定讀取分離；私有 fd 不可繼承。
- HTTP timeout、取消、QPS、空／無效回應、具／不具 Content-Length 的大小界限、錯誤不回顯合成 key。
- startup crash、exit0 無 terminal、合法 buffered finish、缺少要求輸出、錯 session／task、忽略取消的程序與持 pipe 孫程序。

## 失敗紀錄與修正

早期 Rust GNU linker 產物在 main 前發生存取違規，經最小 probe／LLDB 縮小後改用 Rust bundled `rust-lld`；之後 21 項 Rust 測試通過。早期 Python frame 參數測試產生過長 pytest ID，已用明確短 ID 修正。

第一次完整 PDF 與第一次一頁 PoC 都逾時，沒有列為通過。debug 堆疊顯示 NumPy 原生 import 期間停滯，標準 stdin 同時由協定 reader 阻塞讀取；隔離私有 stdin 後重新執行一頁、完整四頁及完整測試通過。舊失敗證據保留於測試目錄。

## 尚未驗收

原 34 項 Python retry tests 仍包含於 269 項，**尚未有對應完整 Rust 34 項等價驗收**。SDK retry／jitter／Retry-After、所有 provider、設定遷移、cache、GUI、預覽、真實免費 API、長時間／大型／並行 PDF、乾淨 Windows、正式打包、安全與再散布授權均需後續驗證。沒有將舊輪次通過數字當成上述新功能的測試結果。
