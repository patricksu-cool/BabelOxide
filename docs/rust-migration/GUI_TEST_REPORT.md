# 原生 GUI 實際驗收

日期：2026-10-30。Windows x64 開發建置，eframe 0.37.0／PDFium 8125。computer-use 由使用者指定的 **GPT-6.1 Terra、xhigh 子代理**執行；整合者另外核對產物、設定、程序與 SHA256。不是乾淨 Windows VM 或正式可攜包驗收。

所有設定、HOME／USERPROFILE／TEMP 與輸出隔離在 `.test-output/rust-migration/phase6-gui-qa2`、`phase6-gui-qa3`，HTTP 只接本機 identity fixture。未使用使用者真實 API key、付費服務或既有設定。

| 操作 | 觀察與核驗 |
| --- | --- |
| 原文預覽 | 授權四頁 PDF 可渲染，切換第 1／2 頁；初始 Fit page 可見整頁 |
| 翻譯 | GUI 顯示 Complete 1/1、100%；105 次固定回應、36 個 batch；產生 mono 與 dual |
| 結果預覽 | Translated 第 1／2 頁、Bilingual 可見；1.25→1.75 zoom 及 Fit page 正常 |
| 設定保存 | 顯示 Settings saved；實際 TOML 的 `input_files=[]`，未將本次文件保存成預設輸入 |
| 取消 | 第二輪在 Loading PDF engine 按 Cancel，先顯示清理中，約 10 秒內成為 Cancelled 0/1；之後仍可翻原文頁 |
| 選檔 | 同一原檔再加入維持 1；加入既有 mono 測試產物成為 2；移除後回到 1 |
| 另存 | Original 另存至全新 `phase6-gui-qa3/saved-original.pdf`，顯示 Saved；沒有覆寫提示。SHA256 與來源皆為 `d0a8367aa014b52929af86f42b5164ade3bb551c69717734a975ba122369498f` |
| 關閉 | 兩輪皆點 X 正常消失，fixture exit 0；整合者另唯讀查詢該專案 worker 路徑，沒有殘留符合的 Python 程序 |

GUI 產出的 mono／dual 與 Python main 比較：頁數、文字、字元座標、圖片均一致，八頁 Poppler 96 DPI RGB 渲染像素差 0，見 [比對數據](PHASE6_GUI_PDF_COMPARISON.json)。這是固定回應排版驗證，不能代表真實中文翻譯品質。

最初 picker 因測試隔離 profile 沒有 Desktop 而出現 Windows 提示；補齊 fixture Desktop／Documents／Downloads 後再次操作，不再出現。沒有修改系統使用者資料夾。最初 300 秒 fixture 在額度中斷期間超時的紀錄保留，不列為正常完成證據。

17 個 GUI 自動測試另涵蓋 headless 真正 egui frame、password paint 不含合成 key、drop 多檔、共用任務提交／關閉與原子另存。Windows 鎖定目的檔的舊 bytes 保留由 unit oracle 驗證，桌面另存只選新檔。沒有逐項操作全部 235 個設定、所有 24 個服務、語言字型、所有錯誤提示或原生驅動組合；未宣稱那些項目完整驗收。

## 獨立 runtime 診斷套件

首版整包複本位於 `.test-output/p 包`，未指定 Python／Worker／worker-home，Terra xhigh 實際完成翻譯與兩種預覽，再在第二輪處理中按 X。先顯示「Cancelling tasks and closing safely...」／「Cancelling and cleaning up」，約 10 秒後視窗消失；fixture exit 0。原生 watch 的 stdout progress 為 0，不用它取代 UI 證據。整合者另核對 105 requests、36 batch、八頁 PDF 零差異，見 [首版包 GUI 比對](PHASE6_PORTABLE_GUI_COMPARISON.json)。

新 `dist/hybrid-diagnostic-20261030-v2`（Rust `f17abb8`／Pillow 13）複製至 `.test-output/q 包 v2` 後重做相同步驟。Terra xhigh 回報 Complete 1/1、100%，Translated 與 Bilingual 均可顯示並翻到第 2/4 頁；第二輪處理中關閉顯示取消／安全清理後視窗消失。fixture exit 0、105 requests／36 batch，產生 mono／dual 四頁；完整比對另見 [新版包 GUI 比對](PHASE6_BUNDLE_GUI_COMPARISON.json)。整合者對此特定複本唯讀查詢 Python Worker，沒有符合的殘留程序。

兩輪都用隔離 HOME／設定／TEMP、原指定四頁 PDF、本機 identity API。被測引擎是包內 embedded Python 與 BabelDOC；開發 `.venv` 僅執行外部測試 harness／PDF 比對，不作被測 Worker。這些結果不代表乾淨機、唯讀安裝或所有顯示驅動已驗收。

後續新 Hello factory 流程再編譯 GUI，只替換 `.test-output/q 包 v2` 測試複本的 EXE；原 `dist` 成品未改。Terra xhigh 再次完成翻譯／譯文與雙語預覽，第二輪處理中關閉亦安全清理退出。兩輪合計 109 requests、36 batch、exit 0，指定 Python Worker 無殘留；首輪八頁零差異。詳見 [FACTORY_INITIALIZATION](FACTORY_INITIALIZATION.md)。
