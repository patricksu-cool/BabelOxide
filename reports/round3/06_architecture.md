# 第三輪架構重構

## 分析與邊界

重構前主要結構：gui.py 約 2,730 行，high_level.py 約 649 行，config/main.py 約 659 行；已有 ui/files、session、settings、resources，以及 translation/configuration、usage。

codebase-memory 檢查相關檔案 coverage／freshness，查詢 gui 與 high_level 函式及呼叫鏈，並以 Serena references 和原始碼確認 Gradio 事件動態入口。圖上的 CALLS cycle 為 0，但不是動態 import／執行期無循環的完整證明。

保護範圍為所有 translator 與 utils/asynchronize 套件，共 23 個實際 Python 檔案；與基準 1345fc2 AST 完全一致。上一輪敘述中的 utils/asynchronize.py 為簡寫／過時路徑，實際使用 package/__init__.py。IPC／例外的公開 pickle module path 不移動。

## 6a 輸入準備

將 _get_unique_dest_path、download_with_limit、_prepare_input_file 從 gui.py 原樣移到 ui/inputs.py，三個函式 AST 相同。ui/files.py 維持元件事件，inputs.py 負責實際檔案複製／URL 下載；輸入服務不再依賴 GUI 宣告或設定全域變數。舊 gui 名稱 re-export 以保留外部匯入。

先新增並通過 7 項 GUI 任務／輸入 characterization tests，涵蓋成功、錯誤、取消、19 個事件輸出、ZIP 項目名稱、檔名碰撞、上傳狀態 fallback。搬移後相關測試通過。

後續單元會在測試後逐一提交，本報告隨最終實作補足。

## 6b GUI 任務控制器

將翻譯批次、進度事件、取消、Session 結果整合從 gui.py 拆到 ui/tasks.py。TaskContext 明確注入五個入口（UI 資料轉換、輸入準備、設定組裝、目前設定、單檔任務），控制器不讀取 GUI 元件或 ConfigManager 全域。BabelDOC stream 與公開 TranslationError 由相容入口注入，保留 root gui 回呼名稱／簽章和 monkeypatch 邊界。

取消函式原樣搬移；逐檔 task 建立、等待、取消、finally 清理及 19 個事件輸出順序保持。完整回歸 **231 passed，11.96 秒**；沒有觸及 provider 重試或 IPC。
