# 第四階段：全專案掃描與重構計畫（2026-10-22）

## 範圍與方法

- 掃描 `pdf2zh_next/`、`tests/`、`script/`、建置設定與文件。結構索引涵蓋 35 個主要程式模組；已確認本文引用的主要程式檔在索引中且資料未過期，並以原始碼核對關鍵結論。靜態圖中的呼叫邊不能證明實際執行，也不能排除所有動態循環依賴。
- `ruff check . --output-format json`：0 項問題；這代表現行規則通過，不代表所有邏輯正確。現有套件測試為 103 passed、73 warnings。測試集中於設定、快取、DeepSeek 與部分 GUI 情境，多數翻譯服務及跨行程處理缺少直接測試。
- `uv audit --locked --output-format json` 稽核 220 個鎖定套件，回傳 89 筆記錄；依套件與公告 ID 去重後為 **58 筆公告**，詳見 `04_dependency_audit.json`。這是版本比對，不等於已證明每項漏洞可從本程式觸發。原始結果保留在未追蹤的 `.test-output/uv-audit.json`。
- 本階段只分析與記錄，未修改產品程式碼、重試演算法或檔案架構。

## 問題清單（依風險與可驗證性排序）

| 優先 | 問題與證據 | 影響／建議 |
|---|---|---|
| P0 | `uv.lock` 鎖定 BabelDOC 0.6.2。其維護者 [GHSA-m8gf-v64p-gfmg](https://github.com/funstory-ai/BabelDOC/security/advisories/GHSA-m8gf-v64p-gfmg) 指出 `<=0.6.2` 在處理特製 PDF 的 CMap 資料時可載入不受信任 pickle，0.6.3 修正。此專案確實以 BabelDOC 處理輸入 PDF。 | 對不受信任 PDF 有高度風險；建議在獨立依賴相容性分支／階段驗證 BabelDOC 0.6.3、bitstring、PDF 輸出及 EXE 建置後再決定升級。本階段不升級、不執行惡意 PoC。 |
| P0 | `uv.lock` 對 Python 3.11+ 選 `bitstring 5.0.0`，BabelDOC 0.6.2 匯入 `BitStream` 失敗；原始 `uv sync` 後無法收集測試。基準環境只在 `.venv` 手動降至 4.4.0 才能執行。 | 新環境無法可靠啟動。後續應在獨立、經完整回歸的相依套件修正中鎖定相容版本；不可將目前手動環境修正誤認為鎖檔已修復。 |
| P1 | `main.py:82` 在 `--debug` 時記錄完整 `SettingsModel`，API Key 是 `translate_engine_model.py` 中的一般 `str`；`config/main.py:321,325,409,569` 也含原始設定的 debug 日誌（該模組目前設 INFO）。 | 除錯輸出可能暴露憑證。未讀取或輸出任何真實 Key；建議建立合成憑證測試後只記錄遮罩值。 |
| P1 | `main.py:102-103` 用 `ignore_error=True` 呼叫 `do_translate_file_async` 後一律回傳 0；`high_level.py:707-786` 明確累計並回傳錯誤數。 | CLI 翻譯失敗可能仍報成功退出，會誤導自動化。先建立退出碼測試；修正將改變外部行為，需獨立提交並確認相容需求。 |
| P1 | 多個服務在 `translator_impl/` 用 Tenacity；OpenAI 類服務對 429 最多 100 次、Google/Bing 等對所有 `Exception` 最多 5 次。`base_translator.py:87,108` 在進入服務前限流；`qps_rate_limiter.py:22-38` 的 `sleep` 位於全域鎖內。 | 錯誤分類、累計等待及並行吞吐量具有服務差異。第五階段先建立 characterization tests；不擅自統一重試邏輯。 |
| P1 | `gradio 5.35.0`、`starlette 0.52.1`、`pillow 11.3.0` 分別命中 7、5、45 項去重公告。專案約束 `gradio<5.36`，多個 Gradio 修正版為 6.x。 | 需按實際 GUI 入口、Windows 版本及輸入型態逐項確認可達性與升級成本；目前只報版本風險。詳見公告清單與 [Gradio 公告](https://github.com/gradio-app/gradio/security/advisories/GHSA-jmh7-g254-2cq9)、[Starlette 公告](https://github.com/Kludex/starlette/security/advisories/GHSA-82w8-qh3p-5jfq)。 |
| P2 | `config/main.py:216-263` 直接以 `open("w")` 重寫 TOML；鎖僅涵蓋單次讀或寫，讀取、比較、更新不是跨程序交易。 | 多個 GUI/CLI 行程同時儲存可能丟失更新，異常中止可能留下截斷檔；沒有重現證據。保留現行行為，先以可控制並行測試確認後再考慮原子寫入。 |
| P2 | `high_level.py:338-516` 同時管理 async callback、兩條 pipe、兩個 thread、子行程與多層取消／清理；目前無直接涵蓋其逾時和資源清理的測試。 | 高耦合且難以安全改動。先補取消、崩潰、逾時及輸出檔測試，暫緩搬移模組。 |
| P2 | `gui.py` 約 4,471 行，結合 UI 組裝、限流參數、下載及壓縮輸出；設定欄位也集中在 `config/translate_engine_model.py`。 | 實際維護成本高；但大規模拆檔可能破壞 GUI 事件綁定，應只處理可獨立測試的純函式。 |
| P2 | `google.py`、`bing.py` 的 `requests.Session.get()` 沒有明確 timeout；其外層重試與其他服務不同。 | 網路請求可能長時間卡住；先在第五階段記錄原始行為，暫不修改。 |
| P3 | 除英文、繁中外的進階文件仍指向 `~/.config/pdf2zh`；`config/main.py:251-261` 建構 `toml_content` 後實際輸出的是 `content`。 | 文件誤導與無效中間物件。可在最後以低風險、獨立提交修正，並檢查輸出格式一致。 |

## 架構與重構順序

主要路徑是 `main.py`/`gui.py` → `ConfigManager` → `high_level.py` → BabelDOC 與 `translator/`；服務實作各自持有 HTTP/SDK 與重試策略，共用 `BaseTranslator`、快取與 QPS limiter。靜態呼叫圖未顯示明確循環呼叫；匯入時副作用與動態導入仍須用實際啟動驗證。

1. 第五階段先盤點所有 Retry、Rate Limit、Timeout 與錯誤分支，透過 Mock、虛擬等待與可控隨機數固定原始行為。沒有可證明安全性的服務不改動。
2. 第六階段重跑完整測試、CLI/GUI、設定讀寫、指定 PDF 前後對比、失敗情境與 Windows 打包。任何功能退化先修復或回復。
3. 第七階段只選能獨立測試的小型改動：文件同步、設定序列化的無效物件清理、局部純函式抽取與必要的敏感資訊遮罩。每模組測試、比對、獨立提交。
4. CLI 退出碼、原子設定寫入、GUI 拆分、跨行程重構、相依套件升級列為獨立待辦；先建立專項測試與相容性證據，再決定是否修改。安全性高風險以本報告優先揭露，不在本階段大量升級。

## 掃描限制

- 依賴稽核只比較鎖檔版本，不能證明所有公告入口可由本專案使用；其中部分與音訊、OAuth、FileExplorer 或特定影像格式有關，需逐項評估。
- 本階段沒有呼叫真實翻譯 API、付費服務，也沒有執行漏洞 PoC；未發現憑證曾實際外洩的證據。
- 前述 `bitstring` 不相容與 CLI 退出碼是已由原始執行／原始碼確認的問題；設定並行、逾時卡住與高耦合的實際故障仍屬待測風險。
