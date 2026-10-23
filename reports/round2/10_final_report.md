# PDFMathTranslate-next v2.9.0 第二輪最終報告

日期：2026-10-23。專案：`C:\Users\user\Documents\PDFMathTranslate-next-2.9.0`。分支：`maintenance-round2`。本輪沿用第一輪 `9236822` 歷史，沒有重新初始化 Git。

## 1. 完成摘要

依序完成第二輪基準、安全與功能修補、重構前驗證、實際模組／資源拆分及最終回歸。自動化測試由第一輪 **139**、重構前 **160**，增加至最終 **212 passed、67 warnings、0 failed**。

- **安全**：BabelDOC 0.6.2 → **0.6.3**，配合最低相容 PyMuPDF **1.26.7**；文件 dev 依賴 GitPython 3.1.62 → **3.2.1**；正式追蹤 `uv.lock`。統一日誌憑證遮罩，包含 Debug、設定解析、Exception、兩個 CLI translator 與 PyStand 最外層例外。
- **功能修正**：任一 PDF 失敗時 CLI 回傳 **1**，全部成功維持 **0**；GUI 不改用這個退出碼。修復可重現的 Config 共用暫存檔競爭／寫壞舊檔，以及 PDF 預覽 JavaScript 語法錯誤；啟用腳本後發現的 worker fetch 攔截也已移除。
- **可攜式 Config**：保留第一輪的程式旁 `config/`、不依賴 cwd、原 TOML 格式及不覆寫的舊設定遷移；本輪增加唯一暫存檔、fsync／原子替換與失敗清理。
- **架構**：拆出 GUI 檔案回呼、Session、欄位規則、設定轉換、語言／CSS／JavaScript／YAML 資源，以及 BabelDOC 設定與 Token 統計；根模組保留相容入口。Windows 打包改用鎖定、非 editable 環境。
- **交付狀態**：預設 `.venv` 已同步修補後鎖檔；本機新 EXE 位於 `.test-output/round2/exe-stage9/pdf2zh.exe`。測試 GUI 行程已停止，該 EXE 測試包已重設為沒有合成憑證的預設設定。沒有使用真實或付費翻譯 API。

**仍有 68 項依賴公告、既有公式附近重疊、文件建置與格式問題；本報告不宣稱安全稽核全部通過或所有排版問題已修復。**

## 2. 安全修復與剩餘風險

| 項目 | 變更與驗證 | 狀態 |
|---|---|---|
| BabelDOC `GHSA-m8gf-v64p-gfmg` | 選最低維護者修正版 0.6.3，確認 verified CMap loader；保留 bitstring 相容版。完整 PDF、GUI、EXE 驗證，最終 audit 無該公告 | 已修補 |
| GitPython `GHSA-83vg-56qc-22m7` | 移除未使用且阻擋修補的 Material recommended／RSS extra，保留實際 minify，dev 指定 3.2.1；runtime 依賴不因此改動 | 已修補；本專案未呼叫受影響 submodule API |
| API Key／Token／Password 日誌 | `security_logging.py` 共用遮罩、合成憑證與例外測試；實際 Python／EXE Debug、非 Debug、失敗日誌均未含測試金鑰 | 已修補已知輸出點 |
| Gradio 5.35，7 項 | Windows Python 3.13+ 檔案路由條件相關，預設 GUI 綁定 `0.0.0.0`，列高優先待實證；其他公告有各自功能前提 | 保留風險 |
| Pillow 11.3，56 項 | 版本命中，實際影像解碼路徑依公告而異；不能把每項視為已可由 PDF 觸發 | 保留、待確認可達性 |
| Starlette 0.52.1，5 項 | Windows StaticFiles 等條件相關；Gradio 底層使用仍需個別核對 | 保留、待確認可達性 |

最終完整鎖檔 **214 套件／98 原始記錄／68 去重公告**，runtime **164／98／68**；兩次 audit 均 exit 1。第一輪報告為 70 個去重公告，本輪移除 BabelDOC 與 GitPython 兩項；公告數也會隨資料庫更新，不能當成可利用漏洞數。

已實際試裝 Gradio 6.16、6.30 及新版 PDF 元件／Pillow／Starlette：單元測試與後端 HTTP 雖成功，瀏覽器因 PDF 元件缺少 Svelte 資源而空白，故回復候選升級。沒有強行違反 Pillow／Starlette 版本限制。後續應先修復或替換預覽元件並補瀏覽器測試，再升 GUI 依賴；目前 GUI 應限可信任使用範圍，不宜直接公開。

詳細公告 ID、版本、修補障礙及來源見 [第六階段安全評估](06_dependency_security.md) 與 [最終稽核資料](09_dependency_audit.json)。未執行攻擊性 PDF 或漏洞 PoC；沒有以未驗證推測聲稱已可利用。

## 3. 實際架構變更

以重構前穩定版 `63074ab` 比較，行數包含註解／空行：

| 模組 | 重構前 | 最終 | 責任調整 |
|---|---:|---:|---|
| `gui.py` | 4,474 行 | **2,803 行** | 保留 Gradio 介面、事件綁定與任務控制；可獨立的轉換／回呼／資源移出 |
| `high_level.py` | 826 行 | **646 行** | 保留公開入口、例外與行程生命週期；移出設定組裝及完成資料統計 |
| `config/main.py` | 692 行 | **659 行** | 保留設定管理／驗證與相容方法；編解碼及原子儲存交給 storage |

### 主要檔案結構

```text
重構前                         重構後
pdf2zh_next/                   pdf2zh_next/
  gui.py                        gui.py                 # 介面／控制入口
  gui_translation.yaml          ui/
  high_level.py                   files.py             # 四個檔案事件回呼
  config/main.py                  session.py           # Session factory
  translator/                     fields.py            # 六個欄位 helper
  utils/asynchronize.py           settings.py          # 設定／位置參數轉換
                                  rate_options.py      # 原 Rate 算式
                                  languages.py
                                  resources/
                                    __init__.py
                                    gui.css
                                    preview.js
                                    translation.yaml   # 實際搬移原 YAML
                                high_level.py
                                translation/
                                  configuration.py
                                  usage.py
                                config/
                                  main.py
                                  storage.py
                                security_logging.py
                                translator/            # 服務策略保留
                                utils/asynchronize.py  # IPC 規則保留
```

### 拆分與減少重複

- **檔案與狀態**：`update_preview`、`on_file_upload`、`on_file_clear`、`on_file_input_change` 搬到 `ui/files.py`；`new_session_state` 集中四處空狀態建立，保留容器獨立性及原取消時機。
- **設定與欄位**：`SaveMode`、`build_translate_settings`、`build_ui_inputs` 拆至 `ui/settings.py`，以新增 `SettingsContext` 接收 ConfigManager／對照表／保存回呼；六個 metadata helper 與兩個 Rate helper 分離，後者 AST 不變。GUI 的原函式簽章、保存後全域狀態更新及 Gradio 事件端點保留。
- **資源**：CSS／JavaScript 從 Python 字串移到套件資源；原翻譯 YAML 實際搬到 `ui/resources/translation.yaml`，用 `__file__` 解析。wheel 與 EXE 已驗證；自訂翻譯檔的人應使用新位置。
- **翻譯 adapters**：`load_glossaries`、`build_babeldoc_config` 接收明確 factory／loader／logger；主與術語翻譯器的共用條件、建議 QPS／pool 副作用保持原樣。`collect_token_usage` 集中完成事件，四種 counter 讀取不再複製兩次。
- **Config 儲存**：TOML encode／decode／唯一暫存檔原子寫入放入 `config/storage.py`；先以兩項失敗重現 shared-temp 競爭及中斷破壞，再以四項並行／失敗測試驗證。這不提供跨行程 read-modify-write 交易，完整寫入仍採最後完成者覆蓋。
- **建置**：移除整份工作目錄複製與 activate，明確使用 `uv sync --locked --no-dev --no-editable`，避免發行目錄依賴來源 checkout；快取包含 lockfile，失敗退出碼不被後續步驟遮蓋。

這是職責與檔案的實際拆分；沒有把格式整理作為全面重構的替代。逐項原行為測試、搬移與驗證證據見 [第八階段計畫](08_refactor_plan.md) 及 `08a` 至 `08g` 報告。

### 保留的高風險邊界

GUI 的大型介面宣告、閉包事件、翻譯任務／進度／下載整合仍在根模組；尚有全域設定和元件耦合。已建立可測的轉換與回呼邊界，但未把所有 session／Gradio 綁定改成新框架。

`high_level.py` 保留跨行程控制與公開例外類別，維持 pickle module path；`_translate_in_subprocess` 及接收／取消／清理邏輯未改。`translator/utils.py` 已有共用 factory，其既有健康檢查請求與各服務 API client／快取／重試未合併。這些部分需要服務別、真實行程及更多 session 整合證據，不能只靠資料轉換測試證明安全。

## 4. 最終測試結果

完整證據見 [第九階段回歸報告](09_final_regression.md)。

| 項目 | 結果 |
|---|---|
| 自動化測試 | **212 passed、67 warnings；0 failed／skipped／xfail** |
| Config | 69 項測試；建立、格式、重載、遷移不覆寫、原子寫入與並行通過 |
| 指定 PDF | Python Debug／非 Debug、新 EXE：各 exit 0，單語／雙語各四頁；每次 107 個本機請求 |
| PDF 比較 | Python＋EXE 非 Debug 共 **16 頁逐像素相同**；逐頁文字、公式與圖片指標一致 |
| GUI | Python 與 EXE 各完成上傳、保存、重載、翻譯、兩份 PDF 與 ZIP 下載；瀏覽器主介面／預覽正常，沒有記錄到前端 error |
| CLI | 全成功／部分失敗／全失敗 **0／1／1**，輸出 **2／2／0**；EXE 損壞 PDF 為 1／0 |
| Windows EXE | fresh no-dev／non-editable 環境 136 套件通過相容性檢查；本機 PyStand＋embedded 3.13.3 重新組包、GUI／PDF／其他 cwd 的 config 路徑通過 |
| 日誌 | 13 項自動測試及實際流程的合成金鑰檢查通過 |
| 靜態 | Ruff lint、拆分模組 formatter、diff 檢查通過；完整格式檢查仍失敗，詳見下節 |
| 安全稽核 | **未通過：68 項公告、exit 1**；沒有新增本輪已知公告 |

Debug 的文字與框線受測試服務／隨機標記影響，不宣稱與早期產物逐位元組相同。非 Debug 使用相同本機回應的比較一致。既有公式重疊仍在，Mock 無法驗證真實中文品質。

## 5. Retry／Backoff 保護結果

**沒有修改演算法或執行行為。** 重構前 `63074ab` 到最終，整個 `translator/` 及 `utils/asynchronize.py` 零差異。第二輪更早在兩個 CLI translator 的變更只有日誌遮罩／摘要。

34 項 Characterization Tests 覆蓋 16 個服務、22 個 Tenacity 裝飾器：嘗試含首次的計數、指數等待、無 jitter 的原策略、Timeout、429／5xx／Retry-After、可重試例外、最終例外、共享限流、取消及 Mock 資源清理均維持原行為。GUI Rate 算式只原樣搬移，未統一服務策略。

既有問題仍保留：部分 HTTP 呼叫缺少明確 timeout；本層不按 Retry-After 調整；部分服務可重試到 100 次；內部重試不重新取得 QPS 令牌；少數 predicate 連中斷都可能再試。SDK 內部重試沒有完整驗證。第一輪早期 Retry 報告的 Tencent xfail 已由第一輪後續相容版修正，目前 **34 passed、無 xfail**。

## 6. Git 歷史與回復

| 階段 | Commit | 內容 |
|---|---|---|
| 1 | `b6eaf41` | 第二輪基準，保留前輪歷史 |
| 2 | `930ee24` | BabelDOC／PyMuPDF 修補、正式鎖檔 |
| 3 | `3e81cd6` | 應用程式日誌遮罩 |
| 4 | `45dfaba`、`13bba5c` | PyStand 例外遮罩、PDF／GUI／EXE 實測 |
| 5 | `ac0bf84` | CLI 失敗退出碼 |
| 6 | `bf96246` | GitPython 開發依賴修補及相容性試驗 |
| 7 | `63074ab` | 重構前穩定基準 |
| 8a | `a3df70c` | Config 原子並行寫入 |
| 8b | `b07e527` | GUI 檔案與 Session 拆分 |
| 8c | `8a4c016` | GUI 設定與欄位分層 |
| 8d | `51ac1d3` | GUI 資源與語言檔搬移 |
| 8e | `aca28e6` | 預覽 JS 與 worker fetch 修正 |
| 8f | `35172f2` | 翻譯設定與 Token 用量 adapters |
| 8g | `8ac31f3` | Windows 鎖定非 editable 建置 |
| 9 | `f5bd0f3` | 最終完整回歸、PDF 與安全資料 |
| 10 | 本報告所在的後續 `docs` commit | 最終彙整；可用下列指令取得 SHA |

```powershell
git log -1 --oneline -- reports/round2/10_final_report.md
git log --oneline --reverse 9236822..maintenance-round2
```

每個主要模組獨立提交，可用對應 `git revert <SHA>` 建立可審查的回復；有相依關係的重構應依反向順序回復，再執行測試。沒有重設第一輪或原始快照，也沒有提交個人設定、虛擬環境、PDF 輸出或測試憑證檔。AGPL 授權／版權保留。

## 7. 未完成驗證與後續順序

1. **GUI 依賴安全升級**：先處理 Gradio 6／PDF 元件 Svelte 資源不相容，才能安全更新 Gradio、Pillow、Starlette。保留 68 項公告及可信任存取限制的建議。
2. **真實服務與取消**：未使用真實憑證／付費 API；供應商 SDK、真實配額、長時間 Timeout、Windows 行程中途硬取消及壓力測試未完成，不能以 Mock 結果替代。
3. **完整發行**：未執行 GitHub Actions 發行、重新下載 runtime、離線包產生／重新下載、其他電腦／作業系統。已完成本機 EXE 的實際 GUI 與 PDF 流程。
4. **PDF 品質**：既有公式附近文字重疊、斷字與字型警告尚未修復；本次非 Debug 前後無新差異。真實譯文品質、所有 PDF 類型及公式語義校對未涵蓋。
5. **文件／格式**：一般文件建置原有 `partials/languages/zh_TW.html` 缺失，strict 另有斷鏈；已比較修改前後同樣失敗。Python formatter 仍有 7 檔，全根目錄含文件區塊共 30 檔；67 個棄用警告未消除。這些沒有當成通過，也沒有放寬標準。
6. **後續架構**：在現有 adapter 邊界上增加真實 session／任務控制測試，再逐步拆 GUI 宣告與控制器；供應商策略、IPC、取消與快取不宜在目前證據下直接抽象合併。

本次交付範圍的安全修補、功能修正、有價值的檔案／模組拆分與可執行回歸已完成；上述剩餘風險與未執行項目仍需另外處理。
