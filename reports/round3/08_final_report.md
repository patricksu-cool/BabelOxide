# PDFMathTranslate-next 第三輪最終報告

日期：2026-10-24。工作分支 **maintenance-round3**，延續 1345fc2 的第二輪歷史，未重新初始化 Git。已完成安全預設、Gradio 相容升級、可確認缺陷修復、逐檔清理與分批架構拆分；自動化回歸 **237 通過、0 失敗**。安全稽核仍 **47 項去重公告，未通過**，不宣稱沒有漏洞。

## 交付與快速入口

- [完整回歸與未覆蓋範圍](07_regression.md)
- [GUI 網路安全](02_network_security.md)、[相容性與官方參考](03_compatibility.md)
- [其他修復／警告](04_repairs.md)、[格式獨立提交](04_format.md)
- [清理說明](05_cleanup.md)、[逐檔刪除清單](05_deletion_manifest.json)
- [重構前後架構與保留邊界](06_architecture.md)
- [集中開發、測試、EXE 組包指南](../../docs/en/development.md)

本機交付包：.test-output/round3/release-wheel/pdf2zh_next-2.9.0-py3-none-any.whl；Windows 目錄 .test-output/round3/exe-release/（連同 runtime、site-packages、launcher 檔使用）。組包腳本 script/build_windows.ps1 已納入版本控制。沒有 push、發布 release 或呼叫付費 API。

## 安全修復與依賴

GUI 預設 127.0.0.1、share=False；刪除啟動失敗後自動改成所有網卡／公開隧道的 fallback。CLI、環境與 TOML 共用 server_name 設定；明確非 loopback 或 share 時警告。空的 auth 檔案 fail closed。檔案 allowlist 限現用 logo／輸出目錄，封鎖應用 config，移除整個 TEMP 的廣泛授權。Python GUI／EXE GUI 實際 socket 均為 loopback；未藉防火牆掩蓋所有網卡監聽。

| 套件 | 基準 | 實際 Windows Python 3.13 發行環境 |
| --- | --- | --- |
| Gradio | 5.35.0 | **6.30.0** |
| gradio-pdf | 舊 custom component | **移除** |
| Pillow | 11.3.0 | **12.3.0**，Gradio 仍限制 <13 |
| FastAPI | 舊相容線（見基準鎖檔） | **0.161.1** |
| Starlette | 0.52.1 | **1.9.0**（鎖檔保留 Python 分支） |
| gradio-i18n | 0.3.4 | **0.3.4** |
| filelock | 跨版本鎖檔 | Windows 3.13 為 **4.2.4**；Python 3.10 分支仍 4.1.0 |
| virtualenv（開發） | 21.14.12 新公告 | **21.14.13** |
| BabelDOC／PyMuPDF | 0.6.3／1.26.7 | **維持原版本** |

基準 214 個套件、98 原始紀錄、68 去重公告；候選時 39；最終 208 個套件、47 原始／去重公告，均有完整或精簡機器可讀證據。ID／alias 比對為 30 個基準群組不再命中、9 個新增群組；不同時間的資料庫會增加／合併公告，不把淨差值當成精確修補率。Gradio／Starlette 舊公告已不再命中。

剩餘為 Pillow 41、filelock 6（Python 3.10 分支）。安裝版本比對 EXE 為 Pillow 41，非另一份安全稽核通過。Pillow 多項修補要求 13.0.0，與目前 Gradio <13 衝突；filelock 新修補線要求 Python >=3.11。維持宣告 Python >=3.10,<3.14，未以 override、忽略 ID 或縮小支援版本掩蓋問題。只處理可信文件／圖片，維持 loopback 與私有快取；若部署可信 LAN 仍需明確設定位址與驗證，並審慎處理 URL 下載的 SSRF 面。

## 相容性與實際功能

在 Gradio 6.30 搭配最新可取得 gradio-pdf 重現 custom_component Svelte runtime／index 動態資產失敗及 loader 停留；不是單純 HTTP 成功即可排除的問題。改用原生隱藏 File、Image、頁碼、前後頁控制，PyMuPDF 每次有界渲染一頁，保留原檔案回呼與清除行為；沒有新增前端框架。theme／CSS 使用 Gradio 6 launch 介面。

Python 與 EXE 均有真實瀏覽器操作及 Console 證據；指定 PDF 上傳／設定／翻譯／預覽／翻頁／下載已完成，另以 client 驗證 191 個設定回載、19 個輸出、mono／dual／ZIP。交付包最後只追加 ClaudeCodeSettings 公開匯入後，重新跑完整 237 測試、建置、EXE PDF、首頁與 client 流程及包內容比對。59 個 wheel 檔案與 EXE 內容一致，沒有 editable 路徑，config 位於 EXE 旁。

指定 PDF 最終 Python／EXE 與基準的文字、頁數、圖片與公式符號指標相同，mono／dual 各四頁；16 組 Poppler 渲染比較像素差異 0。原有公式附近重疊／斷字保留於基準和最終，不能宣稱已修復，亦不以 identity 流程當成真實中文品質驗收。

## 清理、缺陷修復與架構

安全刪除 55 檔、368,287 bytes，僅移除兩個確認空的中繼目錄。逐檔 manifest 記錄用途、引用、理由、風險；保留所有測試、LICENSE、現用 GUI 資源、PO／翻譯快取、歷史報告及不明所有權資料。沒有清除使用者設定、API Key、翻譯產物或虛擬環境。報告體積與產品內容分開量測，見清理報告及 07_inventory.json。

修復 Windows 原子設定檔暫時存取拒絕、optional 數值空白設定、Pydantic／argparse／Gradio 棄用 API、GUI structured error 訊息遺失、根套件 __all__ 缺少 ClaudeCodeSettings 匯入；對可重現缺陷先加入失敗測試。MkDocs zh_TW 以薄模板別名修復，跨語系連結與穩定錨點更正，各語系開發工具改為 uv／Ruff，進階指令與現行 parser 核對。

實質拆分 UI 輸入、TaskContext 任務控制、結果／ZIP 投影、主／術語元件工廠、原生預覽、CLI 批次與 Config parser。gui.py 2,803→2,100，high_level.py 646→564，config/main.py 659→561 行。GUI 與任務流程改透過明確依賴入口協作，四份 ZIP 共用 writer；建置集中於可重用 PowerShell 腳本。每單元依測試→拆分→驗證→Commit 推進。

保留大型 setup_gui 的宣告式畫面、ConfigManager singleton、公開 facade／re-export 及 IPC／例外 module path；不為減少行數破壞相容性。未統一 provider 重試策略。23 個 provider／IPC 檔案及 3 個核心函式 AST 與基準相同，34 Retry／Backoff characterization tests 通過。

## 主要 Git 紀錄

| Commit | 內容 |
| --- | --- |
| 1e7d1b0 | test: record round three baseline and Windows storage failure |
| a0e2d7e | fix: bind GUI to loopback and remove implicit public sharing |
| 07f0ca0 | fix: replace incompatible PDF component and upgrade Gradio stack |
| 9d3271a | fix: repair documentation links, deprecated APIs and Windows config writes |
| abe8b9a | style: apply repository Ruff formatting and import ordering |
| 216dc0b | chore: remove verified obsolete artifacts and secure locked container builds |
| 01ba7a2 | refactor: isolate GUI input preparation with task characterization tests |
| 3b99334 | refactor: separate GUI translation task controller from component wiring |
| 7dabcb2 | refactor: isolate GUI result projection and download archive assembly |
| 3c17110 | refactor: separate CLI batch coordination from translation process transport |
| 2f41386 | refactor: separate typed CLI parser from configuration persistence |
| a306139 | refactor: share provider field component declarations without session globals |
| 0e4e31b | fix: preserve structured translation error messages in GUI tasks |
| addac42 | refactor: centralize locked Windows bundle assembly |
| c6fe26a | fix: update development tooling security and documentation |
| 564b526 | docs: align localized CLI examples with current parser |
| e0e0966 | fix: expose the declared ClaudeCode settings public API |

本報告及最終證據另以收尾 Commit 提交；完整歷史可用 git log 1345fc2..maintenance-round3 查看。

## 剩餘問題與後續順序

1. **安全公告尚未清零**：等待／驗證支援 Pillow 13 的 Gradio 線；評估 Python 3.10 支援政策或 filelock 上游安全回補。不要強制超出相依限制。
2. **BabelDOC 既有排版與 retired table-text**：第二頁公式重疊、斷字／字型疑慮需要最小上游重現與真實翻譯回應；保留原核心，未擅自修補第三方。
3. **Gradio 資源警告**：額外 -W error 診斷曾失敗於未關閉 loop／socket，標準測試沒有警告不等於此問題已消除。
4. **未實機範圍**：外部裝置、公網 tunnel、惡意檔案滲透、付費 provider、長時高併發／硬終止壓力、全部支援 Python 與其他 OS。Docker／GitHub 遠端 release job 也未執行。
5. **離線發行**：本機組包明確略過離線資產 ZIP，需快取或首次下載。CI 腳本保留資產生成，但本輪沒有聲稱乾淨離線機通過。
6. **進一步重構**：setup_gui 仍大，應先針對實際事件順序建立更多瀏覽器契約；transport 拆分應先增強真實取消／逾時／強制終止測試。現階段不改其已受保護的執行順序。

測試產物留在忽略追蹤的 .test-output/；本輪建立的伺服器與分頁於驗證後關閉。早期版本探測曾在未隔離 profile 的新測試 bundle 複製 legacy 設定，原設定沒有更動，該 bundle 不交付；最後 exe-release 使用隔離 profile，交付前移除其合成測試設定。
