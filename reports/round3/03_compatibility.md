# Gradio 相容性與升級

實測 Gradio 6.30.0、FastAPI 0.161.1、Starlette 1.9.0、Pillow 12.3.0、gradio-i18n 0.3.4。Python 支援維持 >=3.10,<3.14；lock 依 Python 分支選擇相容 Starlette。未使用 override。候選環境獨立於原 .venv。

## 根因與方案

官方 gradio-pdf 最新 0.0.24 宣告支援 Gradio >=6,<7，但在真實瀏覽器的最小單一 PDF 元件程式，仍持續停在載入中。Console 明確失敗載入 /gradio_api/custom_component/.../client/component/svelte_runtime_entry.js 及 index.js；見 03_browser_errors.json。這是自訂元件發布資源／Gradio 動態載入契約問題；不是後端 HTTP 200 能證明正常。

替換為 ui/preview.py：隱藏 gr.File 保留事件輸出檔案契約，gr.Image 顯示 PyMuPDF 有界單頁渲染，gr.Number 與前後頁按鈕提供頁碼。最大邊 1600px，檔案以 context manager 關閉，沒有額外框架、CDN 或 PDF browser plugin。分頁私有事件、無預覽分享按鈕。Gradio 6 的 theme／CSS 移至 launch。

瀏覽器實證：主頁顯示、指定四頁 PDF 上傳、1→2 翻頁、設定切換、空白 Timeout 保存、實際本機 identity 翻譯、完成後預覽、單語／雙語／ZIP 實際下載，以及清除上傳。native console error 0；另有 Gradio 的 Method not implemented warning，未隱藏。截圖 03_browser_preview.png。Windows EXE 留待最終回歸。

## 官方來源（2026-10-23 查閱）

- [Gradio 6 migration](https://gradio.app/guides/gradio-6-migration-guide)：app 層參數改放 launch。
- [Gradio PyPI metadata](https://pypi.org/pypi/gradio/6.30.0/json)：Pillow <13、Starlette >=1.0.1,<2。
- [PDF 元件 metadata](https://pypi.org/pypi/gradio-pdf/json) 與 [維護者原始碼](https://github.com/freddyaboulton/gradio-pdf)。
- [Pillow 官方安全公告](https://github.com/python-pillow/Pillow/security/advisories/GHSA-5x8w-8jj5-q52h)：13.0.0 修補，但 Gradio 上限與 Python 3.10 相容性阻擋採用。
- [Starlette release notes](https://www.starlette.io/release-notes/)。

## 安全掃描

完整鎖檔由 214 個受稽核套件、98 筆原始公告，降為 213 個套件、39 筆原始公告；剩餘皆 Pillow 12.3.0。不是全部通過；exit 1。資料庫重複 aliases 需去重後另列，不能以原始筆數當漏洞數。

loopback 限制縮小遠端暴露面；只處理可信 PDF。預覽從 PyMuPDF RGB 像素建立影像，避免直接對上傳檔呼叫 Pillow Image.open；這不是對其他 BabelDOC 影像路徑安全的完整證明。待 Gradio 支援 Pillow 13 與對應 Python 版本後再升級。
