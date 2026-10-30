# 每份 PDF 的服務初始化

更新：2026-10-30。GUI／CLI 的共用 `task_from_settings` 現使用舊 Python factory 的每 PDF 生命週期；不是每 batch 一組已初始化服務。設定載入、建 task、保存、開 GUI／預覽與資產維護仍不送健康檢查。

先驗輸入、glossary 路徑及必要輸出，再從純設定 prototype 建新的 main service。`prepare()` 完成 SDK preflight 後，在其初始化鎖外呼叫普通 `translate("Hello", ignore_cache=true)`，沿用既有 retry／QPS／usage 與取消。它不使用 LLM raw-prompt 入口，不讀寫應用 cache。main 失敗就保留 typed cause 並停止，Python 尚未啟動。

接著採用 main 推薦的 QPS／pool。只有 engine 設定相同，而且**原始 term_qps 明確等於更新後的 main QPS**，才共用 main；`None` 不等於 main 的整數值。需要獨立 term 時，在 main 健康檢查後建立真正的 term instance，以有效 term QPS 初始化，再作其 preflight／Hello。此順序亦適用 Free 的遠端推薦值。最後才啟動 Worker。

| 受控案例 | 實際 oracle |
| --- | --- |
| LLM、raw term_qps=None、關閉自動 glossary | 每 PDF main／term 各一次；兩份不同檔案四次；每份 metrics 獨立、prototype calls 保持 0 |
| 相同 engine、明確相等 QPS | 每 PDF 一次，main／term 共用、term metrics 不重算 |
| 不同 QPS 或不同 engine 設定 | 兩次；main 先於 term |
| 非 LLM、沒有 term | 一次普通翻譯，Google 真 loopback GET 含 Hello |
| Free 回覆 QPS=7／pool=9 | raw None／20 為獨立，明確 7 才共用；主服務的 3 checks、config、Hello 完成後才到 term，Worker 收到 7／9／term pool 9 |
| Cache／usage／QPS | 注入會回 Cancelled 的 cache 仍完全不被 get/set；兩次 Hello 的 calls／admissions／attempts 各 2、usage 16，virtual-clock 限速一次 1 秒 |
| main 401 或輸入遺失 | main 401 只送一次，不送 term、不啟 Worker，typed cause 保留；輸入遺失連 Hello 都不送 |
| HTTP 健康檢查取消 | stalled loopback 中取消，2 秒界限內終態 Cancelled，不啟 Worker |
| 原生 CLI 健康檢查取消 | 受控 rustc child／grandchild，grace=0，返回前固定 HANDLE 都已退出；Python 未啟動 |

最新完整 workspace **225 passed、0 failed、1 ignored**；all-targets Clippy `-D warnings` 通過。證據為 `.test-output/rust-migration/factory-final-workspace-tests.log`、`factory-final-clippy.log`。ignored 的 PDFium native smoke 沿用先前另啟用的驗收，未宣稱此輪重新執行。既有 GUI／CLI 假 Worker tests 已接到 loopback，沒有因新增 Hello 而呼叫真 Google 或其他服務。

早期兩個 GUI fixture 原本不提供 HTTP，新增健康檢查後失敗；補上 loopback 後通過。兩份輸入 oracle 一開始重複同一路徑，被設定層正確去重，已改成兩個不同合成檔。wire 曾觀察到一次 SDK connect attempt 重試；wire oracle 驗精確邏輯呼叫／admission／用量及已接受的請求，精確 attempt 序列由可控 executor 驗證，沒有假稱 Windows 連線永不重試。失敗 log 保留。

原指定四頁 PDF 使用 Pillow 13 隔離引擎與新版 Rust CLI：exit 0、**107 個 identity 請求**（main 106、term 1）、36 batch；回到原 Python baseline 的兩次額外 Hello。mono／dual 共八頁文字、座標、圖片與 96 DPI RGB 像素差均為零，見 [比對數據](PHASE6_FACTORY_PDF_COMPARISON.json)。這是開發建置的功能回歸；`dist/hybrid-diagnostic-20261030-v2` 仍保存較早的 `f17abb8` EXE，沒有被偷偷更新。

GUI 以新編譯 EXE 搭配測試複本的包內 runtime 重做桌面驗收。指定的 Terra xhigh 子代理確認第一輪 Complete 1/1 100%、Translated／Bilingual 可顯示；第二輪處理中點 X 顯示取消與安全清理，之後視窗消失。兩輪合計 109 個 loopback requests／36 batch，fixture exit 0、無合成 key；整合者另查此特定複本無殘留 Python Worker。第一輪的單語／雙語八頁亦零差異，見 [GUI 比對](PHASE6_FACTORY_GUI_COMPARISON.json)。未把測試複本更換 EXE 說成原 v2 manifest 仍相符。

低層 `translate_pdf_with_services` 仍可接收呼叫端管理的 service；應用入口明確選 `FactoryInitialization`。`fresh_for_document` 保留共用 cache／legacy provenance binding，但重建 HTTP session、preflight、limiter、動態 provider 狀態與 counters。沒有把 factory 或健康檢查移入 Python Worker。

這一輪沒有新增 Astra 最終簽核，其用量限制仍未解除；以上是整合者的 source review／fixtures／PDF 證據。Tencent machine credential chain、DeepL prepared cookies、Azure operation deadline 邊界、HTTPX write／pool 與發行門檻仍各自待處理，不能以健康檢查已修正為由刪除舊 provider 基準。
