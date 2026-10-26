# Git 整合驗證

日期：2026-10-26，Asia/Taipei。Rust 移植起點是 `d18165e5f6c9ff7c5b4aa91bcc06253b592f1b9f`。

## 盤點與整合

起始分支 `maintenance-round4`，工作區與 index 乾淨，沒有未追蹤檔案、remote、remote tracking branch、其他 worktree、merge 或 rebase。

| 分支 | 原始 HEAD | 與上一列的獨有提交 |
| --- | --- | --- |
| main | 923682228fc16f0f39505e98ea2d071058d3b717 | 基準 |
| maintenance-round2 | 1345fc2b04f5a7cb1b3c592cb18cfd1e400a3c7d | 17 |
| maintenance-round3 | 0147d19a473d258485104ee7eee2198d13316610 | 19 |
| maintenance-round4 | d18165e5f6c9ff7c5b4aa91bcc06253b592f1b9f | 14 |

三個分支構成完整祖先鏈，沒有分歧或無關歷史。Astra Max（`gpt-6-astra`、`max`）獨立唯讀審查後，將此鏈視為一個整合單位，以 `git merge --ff-only maintenance-round4` 納入 main。完整保留 50 個提交及全部舊分支，未 cherry-pick、reset、刪除或 push。

合併前建立 annotated tag `pre-rust-main-20261026`，解參照為原始 main `9236822…`。第一次快轉遭短暫 `index.lock` 阻擋；鎖檔自行消失後，確認工作區乾淨再成功重試，沒有手動刪除鎖檔。

## 本次實測

測試使用專案 `.venv`、隔離 HOME／USERPROFILE／TEMP／PDF2ZH_APP_DIR，沒有讀取使用者翻譯憑證。Windows asyncio／loopback 測試需在允許本機通訊的執行環境運作；受限程序停滯後已中止。

| 檢查 | 結果 |
| --- | --- |
| 合併前完整 Pytest | 253 passed，16.50 秒 |
| 合併前 Retry characterization | 34 passed，2.88 秒 |
| 合併後 main 完整 Pytest | 253 passed，16.74 秒 |
| Ruff lint／format | 通過；338 files already formatted |
| `uv pip check` | 174 packages 相依一致 |
| main GUI 啟動／config／關閉 | HTTP 200，337 元件，127.0.0.1，share=False，正常關閉 |
| 指定 PDF 完整四頁 CLI | exit 0；107 次本機固定回應；mono／dual 均產生；日誌無合成 Key |
| 各 maintenance 分支包含於 main | 三個 `merge-base --is-ancestor` 均 exit 0 |

完整收集包含 `test/test_cache.py` 的 8 項與 `tests/` 的 245 項。最初只執行 `tests/` 得到 245 passed，發現差異後重新執行完整 253 項。第一次 merge 失敗後曾誤啟動未合併 main 的測試（119 passed、20 failed，缺少舊 gradio_pdf）；保留診斷，沒有將其當成合併後結果。

原始 JUnit 與 GUI 記錄位於忽略追蹤的 `.test-output/rust-migration/`；固定回應 PDF 與摘要位於 `.test-output/round4/rust-main-baseline/`。PDF 來源為使用者指定的 `High_speed_buffer_latch_ISCAS03.pdf`，服務僅 loopback identity fixture，並非真實翻譯品質驗收。

## 開發基準

所有上述整合門檻完成後，在 main 建立 `rust-python-baseline-20261026` 標籤，再建立 `feature/rust-babeldoc-hybrid`。後續修改僅在該分支進行。

標準回歸通過不代表既有安全與發行限制已消失。第四輪嚴格警告模式、Pillow 公告、獨立乾淨 Windows 與第三方再散布驗證仍須另外處理；本階段未將舊報告當作本次通過證據。
