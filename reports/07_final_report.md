# PDFMathTranslate-next v2.9.0 最終報告（2026-10-23）

## 1. 修改摘要

- 以現有原始碼建立全新 Git 歷史。第一個 commit 保存完全未修改的原始碼；AGPL-3.0 授權及原有版權資料保留。`.gitignore` 排除 `config/`、虛擬環境、快取、輸出及常見密鑰檔。`git ls-files config` 為空。
- 將預設設定移至程式所在目錄的 `config/`。原始碼、一般凍結 EXE 與 PyStand 啟動都不依賴目前工作目錄；新檔不存在時，舊 `~/.config/pdf2zh/config.v3.toml` 僅複製一次，既有新檔不被覆寫。格式仍為原 TOML。
- 修正原始依賴不相容：鎖定 `bitstring==4.4.0` 與 `tencentcloud-sdk-python-tmt==3.1.121`，使全新專案環境能匯入 BabelDOC，Tencent 服務類別也可載入。沒有為安全公告升級其他套件。
- 在重構前建立主要翻譯服務 Retry／Backoff 的 Characterization Tests。第七階段只移除設定寫檔函式中從未用於輸出的 TOML 中間建構，另新增巢狀設定與 `None` 序列化的位元組格式測試，並更新九種語言的進階使用文件。

## 2. 架構變更與選擇

| 項目 | 原始 v2.9.0 | 最終版本 | 原因 |
|---|---|---|---|
| 設定儲存位置 | 使用者目錄 `~/.config/pdf2zh/` | 程式旁 `config/`；安全的一次性舊檔複製 | 符合可攜式 Windows EXE 與原始碼執行需求 |
| 路徑解析 | 以使用者家目錄為中心 | `const.py` 決定應用程式目錄；PyStand 啟動腳本先傳入 EXE 目錄 | 讓 GUI、CLI、子行程共用相同設定位置 |
| TOML 寫入 | 建構未使用的 `toml_content`，最後仍 `tomlkit.dump(content, f)` | 直接保留原有 `tomlkit.dump(content, f)` 路徑 | 刪除無效工作，避免讀者誤認那份文件會寫出 |
| 測試與紀錄 | 多數服務重試及 PDF 前後比對缺乏固定基準 | 新增設定、依賴、Retry 測試與分階段報告、PDF 指標腳本 | 讓每階段可驗證、可比較、可獨立回復 |
| 來源目錄與服務模組 | GUI、跨行程處理、各翻譯服務位於原模組 | **未搬移**；原有服務重試與 HTTP 呼叫保持原樣 | 大規模拆檔／共用重試抽象的相容性證據不足，沒有足夠利益支持風險 |

英文、繁中及另外九種語言的進階 Markdown 文件已描述新路徑。`docs/Contribute/*.po` 是舊翻譯字串資料，未重新產生；若日後用它重新生成文件，需先同步翻譯目錄，避免覆蓋已更新的 Markdown。

## 3. 測試報告

### 通過

| 測試 | 結果 |
|---|---|
| 初始基準 → Config 修改 → 重構前 → 最終完整測試 | **99 → 103 → 138 → 139 passed**。最終為 **139 passed、73 warnings、0 failed**；新增的第 139 項測試先在原始寫檔實作通過，重構後仍通過。 |
| 設定 | 建立、遷移不覆寫、讀取、修改與重新載入，以及來源／EXE 路徑獨立於工作目錄的測試通過；`config/` 被 Git 忽略。 |
| CLI／GUI | CLI 啟動及版本查詢通過；最終 GUI 在 loopback 回傳 HTTP 200、654,137 bytes 首頁。 |
| 指定 PDF | 最終離線 Mock 流程完成；原始四頁 PDF 產出可由 Poppler 開啟／渲染的單語與雙語四頁 PDF。翻譯入口 233 次、快取 232 次、離線 Mock 真正呼叫 1 次。 |
| PDF 內容 | 重構前後單語公式符號 **48**、圖片 **23,998**；雙語公式符號 **98**、圖片 **47,996**，均相同。最終可擷取字元單語 **505,242**、雙語 **2,060,933**，重構前分別 **505,786**、**2,060,677**。檔案及文字雜湊不同，因此不宣稱逐位元組相同。 |
| PDF 視覺 | 與重構前輸出以 50 DPI 逐頁比較，單語變動像素比例 **0.38%–0.67%**、雙語 **0.33%–0.60%**；最大平均色彩通道差異 **0.066/255**。這與先前未重構兩次處理的差異量級相近。檢視公式、電路圖及波形頁面，未見明顯破版或圖表遺失。詳見 `07_final_pdf_metrics.json` 及兩份渲染比對 JSON。 |
| Retry／失敗情境 | 全套測試包含 429、5xx、`Retry-After`、逾時、最終例外、取消與並行限制等 Characterization Tests；第七階段後仍全數通過。 |
| Windows EXE | 本機 PyStand 1.1.4／Python 3.13.3 embedded runtime 打包結構於第六階段建成；最終更新打包目錄的設定模組，原始檔與打包檔 SHA-256 相同。從其他工作目錄執行 `pdf2zh.exe --version` 回傳 2.9.0，設定位於 EXE 旁 `config/default/2.9.0.toml`，工作目錄沒有 `config/`。 |
| 靜態／相依套件 | 最終 `ruff check .`、設定實作與路徑測試兩檔的 `ruff format --check`、`git diff --check` 均通過；`uv pip check` 檢查 185 個已安裝套件，無依賴衝突。 |

### 失敗或仍有警示

- `uv audit --locked` 因現有依賴安全公告以非零狀態結束：218 個受檢套件、**101 筆原始記錄**，依套件及顯示公告 ID 去重為 **70 項**。第六階段為 89 筆／58 項；後續資料庫新增 12 個顯示 ID（GitPython 1、Pillow 11），相依套件版本未因重構改變。詳見 `07_dependency_audit.json`；記錄數不代表 70 項均可由本程式觸發。
- Poppler 渲染期間仍發出原 PDF 字型缺失／Type 3 字形警告；所有頁面仍成功渲染。自動測試有 73 項棄用警告，沒有失敗。

### 未執行

- 未使用真實翻譯 API、個人 API Key 或付費服務；外部服務實際網路行為沒有被驗證。HTTP／SDK 錯誤路徑使用 Mock。
- PDF 使用 `--debug` 讓離線 Mock 在主行程生效，輸出保留原有除錯框線；沒有以同一 PDF 驗證非 debug 模式或人工逐字校對全部文字、公式語義。
- Windows EXE 僅驗證本機 PyStand 打包結構、CLI 啟動與設定路徑；沒有執行完整 CI 發行流程、DirectML 資產整合、EXE GUI 手動操作或安裝程式測試。
- 未執行惡意 PDF／安全漏洞 PoC，亦未升級有公告的相依套件。`ruff format --check .` 全專案格式檢查未作為門檻；已修改的設定路徑檔案通過格式檢查。

## 4. 安全報告

- **已核實版本風險，高優先：** 實際 PDF 管線使用 BabelDOC 0.6.2，維護者的 [GHSA-m8gf-v64p-gfmg](https://github.com/funstory-ai/BabelDOC/security/advisories/GHSA-m8gf-v64p-gfmg) 指出該版本處理特製 PDF 的 CMap 時可能反序列化不受信任 pickle，修正版為 0.6.3。建議在獨立工作中升級並重跑 PDF、快取、Windows 打包及服務回歸；升級前只處理可信來源 PDF。
- **已核實程式碼風險：** `main.py` 的 debug 設定日誌會包含一般字串型 API Key，設定模組也有原始設定 debug 訊息。未發現真實憑證洩漏證據；建議以合成密鑰測試後遮罩日誌，避免將除錯紀錄公開。
- **版本命中但可達性待查：** Gradio 5.35.0、Pillow 11.3.0、Starlette 0.52.1、GitPython 3.1.62 均有公告。GitPython 新列的 [GHSA-83vg-56qc-22m7](https://github.com/advisories/GHSA-83vg-56qc-22m7) 與不受信任 submodule 路徑相關；專案原始碼未找到直接 GitPython 使用，但仍需確認傳遞依賴與發行內容。其它 GUI／圖片公告也需按實際入口逐項評估，不能以版本比對直接宣稱可利用。
- **待測風險：** 同時寫設定可能丟失更新或留下不完整檔；Google/Bing 請求未設明確 timeout；CLI 對部分翻譯失敗仍回傳 0。這些不在本次低風險重構中改變外部行為，需建立專項測試並獨立修復。

## 5. Retry／Backoff

產品程式碼的 Retry、Backoff、QPS 限制、Timeout、HTTP 429／5xx 處理與異步取消 **沒有修改**。第五階段盤點 16 個服務與 22 個 Tenacity 裝飾器，以 Mock、虛擬等待及可控錯誤固定最大嘗試次數、等待、`Retry-After` 未被本地裝飾器採用、最終例外及服務間差異。原有 Bing／DeepL 對廣泛 `Exception` 重試、SiliconFlowFree 多層重試、Claude timeout 轉為不可重試例外等行為均保留，詳見 `05_retry_characterization.md`。未在真實 API 條件下驗證供應商側限流。

## 6. Git 歷史

| 階段 | Commit | 內容 |
|---|---|---|
| 原始快照 | `277d1f1` | 完全未修改的 v2.9.0 原始碼 |
| 第一階段 | `4d15fd7` | 忽略個人設定、密鑰、快取及測試輸出 |
| 第二階段 | `ee58c90` | 修改前測試及指定 PDF 基準 |
| 第三階段 | `62cb11a` | 程式旁設定路徑與安全遷移 |
| 第四階段 | `7fce1b6` | 全專案掃描、風險排序及安全報告 |
| 第五階段 | `9e7ab45` | Retry／Backoff Characterization Tests |
| 第六階段前置相容性 | `bd56642` | Bitstring 與 Tencent SDK 相容性釘選 |
| 第六階段穩定門檻 | `b9d8b48` | 重構前完整回歸紀錄 |
| 第七階段設定模組 | `d08950c` | 無效 TOML 建構清理與格式保護測試 |
| 第七階段文件 | `0d945a1` | 九種語言設定路徑同步 |

各主要修改可由獨立 commit 回復。最終報告、最終 PDF 比對與當日安全稽核摘要另存於本報告所在 commit。

## 7. 剩餘問題與建議順序

1. **高優先安全工作：** 在隔離修改中驗證 BabelDOC 0.6.3 或更新版，及 Gradio／Pillow／Starlette 的可達公告；先確立功能與 EXE 回歸，再升級。GitPython 新公告亦需按使用路徑確認。
2. **可觀察的功能缺陷：** CLI 某些失敗回傳成功退出碼，會誤導自動化；建立退出碼測試後以獨立 commit 修正。
3. **需先重現的穩定性風險：** 設定寫入原子性、無 timeout 網路請求、跨行程取消／清理。這些屬高風險路徑，不以沒有故障證據的重構更動。
4. **驗證缺口：** 真實 API、非 debug PDF、完整 Windows 發行包及 EXE GUI；也需處理 `docs/Contribute/*.po` 的舊翻譯字串。現有測試不能替代這些驗證。

結論：已完成可執行的分階段功能驗證與兩項低風險整理，沒有觀察到新增的重大功能退化；未能證明安全的核心架構與重試行為保留原狀。
