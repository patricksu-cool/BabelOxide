# 第三輪架構重構

## 分析與邊界

重構前主要結構：gui.py 2,803 行，high_level.py 646 行，config/main.py 659 行（Git 基準 1345fc2；不含尾端空白行）；已有 ui/files、session、settings、resources，以及 translation/configuration、usage。

codebase-memory 檢查相關檔案 coverage／freshness，查詢 gui 與 high_level 函式及呼叫鏈，並以 Serena references 和原始碼確認 Gradio 事件動態入口。圖上的 CALLS cycle 為 0，但不是動態 import／執行期無循環的完整證明。

保護範圍為所有 translator 與 utils/asynchronize 套件，共 23 個實際 Python 檔案；與基準 1345fc2 AST 完全一致。上一輪敘述中的 utils/asynchronize.py 為簡寫／過時路徑，實際使用 package/__init__.py。IPC／例外的公開 pickle module path 不移動。

## 6a 輸入準備

將 _get_unique_dest_path、download_with_limit、_prepare_input_file 從 gui.py 原樣移到 ui/inputs.py，三個函式 AST 相同。ui/files.py 維持元件事件，inputs.py 負責實際檔案複製／URL 下載；輸入服務不再依賴 GUI 宣告或設定全域變數。舊 gui 名稱 re-export 以保留外部匯入。

先新增並通過 7 項 GUI 任務／輸入 characterization tests，涵蓋成功、錯誤、取消、19 個事件輸出、ZIP 項目名稱、檔名碰撞、上傳狀態 fallback。搬移後相關測試通過。

後續單元均在測試後逐一提交，對應紀錄見 08_final_report.md。

## 6b GUI 任務控制器

將翻譯批次、進度事件、取消、Session 結果整合從 gui.py 拆到 ui/tasks.py。TaskContext 明確注入五個入口（UI 資料轉換、輸入準備、設定組裝、目前設定、單檔任務），控制器不讀取 GUI 元件或 ConfigManager 全域。BabelDOC stream 與公開 TranslationError 由相容入口注入，保留 root gui 回呼名稱／簽章和 monkeypatch 邊界。

取消函式原樣搬移；逐檔 task 建立、等待、取消、finally 清理及 19 個事件輸出順序保持。完整回歸 **231 passed，11.96 秒**；沒有觸及 provider 重試或 IPC。

## 6c 結果與下載

ui/results.py 接收 Session 字典並建立進度／結果輸出，抽離原控制器內的閉包；四份 ZIP 使用同一個 writer，維持原結果順序、mono／dual／glossary 前綴、空 ZIP 建立及可見性旗標。新增多檔混合 glossary 測試，在重構前先通過，拆分後 18 項相關測試通過。沒有改變設定保存、翻譯、重試或取消順序。

## 6d CLI 批次與核心傳輸

將 high_level.do_translate_file_async 的批次 loop 移到 translation/batch.py，透過 factory／stream／例外型別／logger 注入保持原行為。high_level 保留同簽章公開入口、同步 wrapper、跨行程函式與例外類別 module path。重構前新增 RuntimeError 與 CancelledError 傳遞測試；拆分後 CLI、adapters 與 34 項 Retry 測試全部通過。

## 6e Config CLI 解析

config/cli.py 專責由 Pydantic 設定模型建立 argparse；MagicDefault 與允許重複欄位規則由同一模組提供，config/main.py 保留相容 re-export 與 ConfigManager 的載入／優先順序／儲存管理。parser 函式 AST 原樣搬移。完整回歸 **234 passed，11.86 秒**，含原 Config 測試與新增取消／多檔結果測試。

## 6f 元件宣告去重

ui/components.py 集中主／術語服務欄位的 Dropdown、Textbox、password、Checkbox 宣告，外層仍保留服務選擇、敏感欄位政策、欄位順序、相依事件與索引；未合併翻譯服務策略。既有完整控制值契約與 DeepSeek visibility 測試保護兩條路徑，完整回歸通過。

## 6g. Windows 組包流程

新增 `script/build_windows.ps1`，集中鎖定環境、非 editable 安裝、相容性檢查、資源複製與 BUILD_INFO，工作流程只負責取得官方 runtime/launcher 並呼叫腳本。輸出限定專案內且拒絕覆寫。以既有官方 Python 3.13.3/PyStand 1.1.4 本機資產實際組包成功，134 套件相容；本機明確省略離線資產 ZIP，使用既有模型快取進行後續測試。CI 遠端執行與重新下載完整離線模型未執行。

## 最終結構與依賴

```text
pdf2zh_next/
  gui.py                     GUI 入口、事件簽章、畫面組裝
  high_level.py              公開 API、子行程與 stream transport
  config/
    model.py, cli_env_model.py, translate_engine_model.py
    cli.py                   typed argparse 建立（新增）
    main.py, storage.py      設定優先順序、遷移、持久化
  ui/
    inputs.py                檔案／URL 輸入準備（新增）
    tasks.py                 TaskContext、Session 任務／取消（新增）
    results.py               結果投影、ZIP 組裝（新增）
    components.py            主／術語欄位宣告（新增）
    preview.py               原生 PDF 預覽（新增）
    files.py, session.py, settings.py, rate_options.py
    resources.py, resources/gui.css, resources/translation.yaml
  translation/
    batch.py                 CLI 批次協調（新增）
    configuration.py, usage.py
  translator/                保留各 provider 策略
  utils/asynchronize/        保留 IPC module path
script/build_windows.ps1      鎖定式 Windows 組包（新增）
```

基準已有 ui/ 與 translation/，本輪在既有分層上完成 7 個有實際職責的新增模組，沒有搬動 provider 或建立通用重試框架。gui.py 2,803→2,100 行，high_level.py 646→564 行，config/main.py 659→561 行；行數含格式影響，實質責任分離與契約測試才是驗收依據，詳見 07_module_lines.json。元件工廠消除約 99 行重複，四份 ZIP 迴圈共用單一 writer。

依賴方向為 gui → UI controllers／settings／preview；tasks 透過 TaskContext 接收應用入口；high_level → translation.batch；config.main → config.cli → typed models。最終 AST 匯入分析覆蓋 55 個 Python 模組、126 條內部邊，未發現靜態 import SCC 循環；不代表動態載入不存在循環，具體依賴保存在 07_import_graph.json。實際非 editable 安裝於其他工作目錄成功匯入 9 個主要入口。

保留 gui.py 中較大的 setup_gui：多數是宣告式畫面與相依事件，拆成大量小檔容易破壞元件順序／回呼索引。ConfigManager singleton 與畫面入口的既有全域設定仍保留；控制器已不直接依賴這些全域。公開 facade／re-export 用於現有匯入、事件名稱與 monkeypatch 相容，並非未驗證即可刪除的空包裝。

跨行程 wrapper、子行程函式、stream transport 及 23 個 provider／IPC 檔案均以基準 AST 保護；沒有因共用程式碼而改寫各服務 timeout／backoff。後續若再拆 transport，應先增加真實長任務取消／強制終止壓力測試；本輪保留其實作。
