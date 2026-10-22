# 第二輪第二階段：BabelDOC 安全修補（2026-10-23）

## 版本選擇與相依限制

- 修改前實際安裝 BabelDOC 0.6.2；維護者的 [GHSA-m8gf-v64p-gfmg](https://github.com/funstory-ai/BabelDOC/security/advisories/GHSA-m8gf-v64p-gfmg) 將 `<=0.6.2` 列為受影響版本、0.6.3 列為修正版。本次未製作或執行攻擊性 PDF。
- 選用最低維護者修正版 **BabelDOC 0.6.3**，固定於 `pyproject.toml`；已安裝版 `cmapdb.py` 使用 `load_verified_cmap_data`，不再直接呼叫 `pickle.loads`。此檢查只輔助版本與來源核對，不取代安全公告及實際 PDF 測試。
- 0.6.3 的新最低要求 `PyMuPDF>=1.26.7` 與原專案 `pymupdf<1.25.3` 衝突；`uv lock` 明確回報無解。因此固定最低相容 **PyMuPDF 1.26.7**。PyPI 發布檔含 Windows 與 Linux ARM64 wheel；Windows 已在本階段實際測試，其他平台尚未執行。
- 保留 `bitstring==4.4.0` 與 Tencent TMT SDK 3.1.121。新鎖檔僅將 BabelDOC 0.6.2→0.6.3、PyMuPDF 1.25.2→1.26.7；`uv.lock` 從原 `.gitignore` 排除項改為正式追蹤，避免下次解析靜默選到不同套件。
- [BabelDOC 0.6.3 發行說明](https://github.com/funstory-ai/BabelDOC/releases/tag/v0.6.3) 同時提及 Type3 字型修正，以及不再支援外部 `CMAP_PATH`。本專案未發現自身設定該環境變數；使用者若依賴自行提供的外部 CMap，屬尚未驗證的相容性變化。

## 安裝與測試

- 使用 `UV_PROJECT_ENVIRONMENT=.test-output/round2-env uv sync --locked` 在工作區內建立全新 Python 3.13.16 環境，成功安裝 185 個套件；`uv pip check` 通過。安裝版本為 BabelDOC 0.6.3、PyMuPDF 1.26.7、bitstring 4.4.0。
- 新增最低安全版本測試；bitstring 介面測試同時通過。完整自動測試 **140 passed、68 warnings、0 failed**；修改前為 139 passed、73 warnings。
- 指定 PDF 使用離線 identity Mock 執行完整 CLI/BabelDOC 管線，exit 0；翻譯入口 233 次、快取 232 次、Mock 呼叫 1 次。單語／雙語都產生可由 Poppler 開啟與渲染的四頁 PDF，公式符號分別 48／98、圖片 23998／47996，與修改前相同。
- 新版單語可擷取字元 508152（修改前 505751），雙語 2073294（修改前 2060433）。50 DPI 的逐頁渲染差異，單語變動像素 0.37%–1.97%，雙語 0.32%–1.59%；第 2 頁差異最大。已檢視 150 DPI 第 2 頁電路圖、公式及文字，沒有發現圖像缺失，但 Debug 標記與文字重疊使細節判斷有限。第三、四階段須以非 Debug 版本再檢查實際閱讀版面。詳細指標見 `02_babeldoc_metrics.json` 及兩份渲染 JSON。
- 按原本 PyStand 目錄方式，從新環境的 wheel 安裝及新 `site-packages` 重建本機 EXE 目錄（沿用上一輪取得的 PyStand 1.1.4／Python embedded runtime）。從不同工作目錄執行 `--version` 回傳 2.9.0、exit 0；EXE 旁建立 `config/default/2.9.0.toml`，工作目錄沒有 `config/`，也沒有複製使用者設定。此為本機打包／CLI 啟動測試，完整 CI 發行包與 EXE GUI 操作留待後續階段。
- `uv audit --locked` 檢查 218 個套件，回傳 99 筆原始漏洞記錄；**BabelDOC 0 筆**。稽核仍因其他套件公告回傳 exit 1；第六階段逐項分類，不能把版本命中視為已可觸發。

## 兼容性界限

Debug 輸出不逐位元組相同，PDF 內的可擷取字元與部分版面有變化；BabelDOC 0.6.3 本身包含 Type3 版面修正。尚未在真實翻譯服務、其他操作系統、完整 CI 打包或全部 PDF 類型驗證。非 Debug 和 GUI 實際流程安排在第四階段。產品 Retry／Backoff 程式碼在本階段未更動。
