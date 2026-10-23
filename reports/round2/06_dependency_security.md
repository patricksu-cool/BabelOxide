# 第二輪第六階段：依賴安全評估與修復（2026-10-23）

## 稽核範圍與結果

以 `uv audit --locked --output-format json` 掃描完整鎖檔，另以 `--no-dev` 掃描執行依賴；相同公告的重複來源按 `display_id` 去重。逐項 ID、修補版本與來源連結見 [06_dependency_audit.json](06_dependency_audit.json)。`uv audit` 因仍有公告回傳 exit 1；沒有把它當作通過。

| 掃描 | 套件數 | 原始記錄 | 去重公告 | GitPython | Gradio | Pillow | Starlette |
|---|---:|---:|---:|---:|---:|---:|---:|
| 修改前完整鎖檔 | 218 | 99 | 69 | 1 | 7 | 56 | 5 |
| 修改後完整鎖檔 | 214 | 98 | 68 | 0 | 7 | 56 | 5 |
| 修改前／後執行依賴 | 164 | 98 | 68 | 0 | 7 | 56 | 5 |

`BabelDOC 0.6.3` 已無本次稽核公告。掃描結果代表版本命中，不等於已確認可利用；`uv` 的公告資料與 JSON 格式為實驗功能，仍需依公告條件和實際程式碼判斷。

## 已修復：文件開發依賴 GitPython

[GitPython GHSA-83vg-56qc-22m7](https://github.com/advisories/GHSA-83vg-56qc-22m7) 指出 `create_submodule()` 在驗證路徑前複製檔案的問題，修補版為 3.2.1。原先 3.1.62 只由文件插件引入，`--no-dev` 沒有 GitPython；專案原始碼沒有呼叫 `create_submodule()`。因此這是開發依賴的版本風險，並非已證實的產品執行路徑。

`mkdocs-material[recommended]` 會安裝未在 `mkdocs.yml` 啟用的 `mkdocs-rss-plugin`，該插件要求 `GitPython<3.2`，使直接指定修補版時 `uv lock` 無解。改用不帶 `recommended` extra 的 Material，單獨保留實際啟用的 `mkdocs-minify-plugin`，並在 dev 群組指定 `gitpython>=3.2.1,<4`。新鎖檔成功解析並安裝 GitPython 3.2.1；`uv pip check` 通過。沒有變更產品執行依賴。

## 保留的 68 項公告與實際使用判斷

| 套件／來源 | 分類與依據 | 修補障礙及剩餘風險 |
|---|---|---|
| Gradio 5.35.0，直接 GUI 依賴，7 項 | [Windows／Python 3.13+ 任意檔案讀取公告](https://github.com/gradio-app/gradio/security/advisories/GHSA-39mp-8hj3-5c49)的版本與平台條件符合本機，且 `setup_gui()` 預設綁定 `0.0.0.0`；未執行攻擊請求，故列為**高優先、可達性待實證**。專案未建立 FileExplorer、Audio、OAuth 或呼叫 `gr.load()`，相關功能公告的本專案路徑未發現；Gradio 自身其他路由仍需評估，不能據此宣稱全部不受影響。 | 公告修補版為 6.x。實測 6.16 與 6.30 的瀏覽器 GUI 都成空白，不能在目前 PDF 預覽元件下安全升級。維持 5.35，對外提供 GUI 時應限制為可信任的存取範圍。 |
| Pillow 11.3.0，Gradio／BabelDOC 的間接依賴，56 項 | 程式本身只在比較 PDF 的開發腳本直接使用 Pillow；Gradio、`scikit-image`、`imageio` 也引入它。各公告涉及不同影像解碼／處理 API，無法由套件存在推定全部可經 PDF 觸發，列為**版本命中、路徑待確認**。[Pillow 13.0.0 公告例](https://github.com/python-pillow/Pillow/security/advisories/GHSA-rfgq-q3w2-pmv2)。 | 18 項可由 12.3.0 修補、38 項需要 13.0.0；Gradio 5.x 約束 `Pillow<12`。連 Gradio 6.30 仍要求 `<13`，而 [Pillow 13.0.0](https://pillow.readthedocs.io/en/stable/releasenotes/13.0.0.html)不支援專案容許的 Python 3.10。不能強行覆寫相依限制。 |
| Starlette 0.52.1，FastAPI／Gradio／SSE 的間接依賴，5 項 | [Windows StaticFiles UNC 公告](https://github.com/Kludex/starlette/security/advisories/GHSA-wqp7-x3pw-xc5r)與本機平台相關；專案未直接建立 `StaticFiles`，但 Gradio 的底層路由可能使用，列為**待確認**。其餘 Host、HTTPEndpoint、URL、表單限制公告依各自功能條件另行判斷，未以掃描結果宣稱可觸發。 | 修補到 1.3.1 以上需要升級 Gradio；目前 Gradio 5.x 要求 `Starlette<1`。暫不違反相依宣告強升。 |

## Gradio 6 升級試驗與回復

在隔離的 `.test-output/round2-stage6-env` 試裝 Gradio 6.16、`gradio-pdf` 0.0.24、Pillow 12.3、Starlette 1.x：160 項自動測試通過，後端 GUI Client 可完成上傳／設定／翻譯／下載；指定四頁 PDF 單語與雙語的八頁 50 DPI 渲染與舊版逐像素相同。**真正的瀏覽器畫面卻載入後空白。** 瀏覽器記錄顯示 `gradio-pdf` 需要的 `svelte_runtime_entry.js` 與 `/assets/svelte/...` 回傳 404。`gradio-pdf` 0.0.23／0.0.24 官方 wheel 均未包含前者；後者是 Gradio 6 不再提供的資源路徑。Gradio 6.30 試驗仍出現相同 404 和空白畫面。舊版 Gradio 5.35／`gradio-pdf` 0.0.22 在同一瀏覽器能正常顯示檔案上傳、語言選擇、翻譯按鈕和預覽區。

試驗中另見 Gradio 6 對頁面載入 JavaScript 的語法錯誤；改成函式形式後該錯誤消失，但 PDF 元件資源仍失敗。所有 Gradio 6、Pillow、Starlette 與臨時 JavaScript 修改均已回復，沒有把僅後端測試通過的候選版交付。進一步升級需要維護或替換 PDF 預覽元件，先補足瀏覽器畫面、預覽及跨版本事件測試；這是獨立的高風險相容工作。

## 回歸測試與既有問題

- 正式變更後：`uv sync --locked --all-groups`、`uv pip check`、`ruff check .`、`git diff --check` 通過；完整自動測試 **160 passed、67 warnings、0 failed**。
- 指定 PDF 使用本機 OpenAI 相容 identity Mock，CLI exit 0、107 次請求（批次 36），產出四頁單語／雙語 PDF；與第四階段的逐頁文字、公式符號和圖片指標 **8/8 頁相同**。合成金鑰未出現在日誌。沒有使用付費 API。
- Gradio 5 GUI Client 事件：上傳、設定儲存／重載、翻譯及下載成功；一頁單語／雙語 PDF 均可開啟，ZIP 含兩份 PDF。對照瀏覽器畫面正常，沒有白畫面。
- `mkdocs build --strict` 因現有文件失效連結退出 1；一般 `mkdocs build` 因缺少 `partials/languages/zh_TW.html` 退出 1。使用**修改前**的隔離環境重跑一般建置，得到相同缺失範本錯誤；這是既有文件建置問題，沒有當成通過，也沒有為了本次安全修補擴大文件改動。
- 本階段未重新製作 Windows EXE，因正式變更僅為 dev 群組、`--no-dev` 的 164 個產品依賴與公告數完全相同；第七階段仍需重新驗證 EXE。沒有執行真實翻譯供應商、攻擊性測試資料或其他作業系統測試。

結論：只交付已驗證的 GitPython 開發依賴修補。其餘 68 項保留為明確風險，不以破壞 GUI 的重大升級交換掃描數字；後續架構工作不得將這些風險誤記為已修復。
