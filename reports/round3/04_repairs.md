# 剩餘問題修復

## 設定檔並行寫入

基準實際遇到 WinError 5。新增兩項故障注入測試先確認失敗：Windows 暫時拒絕應能恢復、永久拒絕應有上限且保留舊檔。原子 replace 僅對 Windows 5／32／33 最多嘗試五次，總等待最多 150ms；其他錯誤立即傳遞。保留唯一暫存檔、fsync、原子替換及 finally 清理，不採用截斷舊檔的 fallback。這只處理設定儲存，不改動翻譯服務 Retry／Backoff。

## 文件

mkdocs-static-i18n 的 reconfigure_material_theme 在載入個別語言設定後強制將 theme.language 設為 locale，將 zh-TW 覆蓋成 zh_TW。以 docs/overrides 的薄模板別名轉送到 Material 官方 zh-TW macro，不複製第三方翻譯。保留既有 zh_TW 目錄及 URL。

根 docs/index.md 原本 include README.md，導致 GitHub 專用相對連結在文件站點失效；改成簡短站點首頁。其餘各語系首頁與翻譯貢獻指南修正相對連結。未停用 Strict 或降低 link 檢查。

## 警告

- 65 個既有 Pydantic instance model_fields 來源（新增測試時 74 個）：改由 class 存取。
- argparse 巢狀 argument group：僅在 root parser 建立群組，不再建立非法巢狀 group；原 duplicate-field 驗證仍在。
- Gradio Dataframe 的 col_count／tuple：改 column_count=2；此表 interactive=False，不需未實作的 column_limits。
- 舊版 websockets legacy 警告隨 Gradio 升級消失。
- BabelDOC 實際 PDF 仍會提示 table_model retired；屬上游 API，另於架構／PDF 報告說明。未全面忽略警告。

## PDF

指定 PDF 使用相同本機 identity 回應，升級前後 CLI 均 107 次請求、36 批次、兩份輸出。既有公式附近重疊／斷字需逐頁比較，不以 identity 服務結果代表中文翻譯品質。BabelDOC、PyMuPDF、字型排版核心保持原版本，不直接改第三方套件。

## 已執行驗證

標準完整 Pytest：224 passed，14.31 秒，無警告。設定／BabelDOC adapter 定向測試通過。一般 MkDocs build 成功（128.50 秒）；Strict build 成功（126.93 秒）。其後又依實際渲染 ID 補上跨語系穩定錨點；最後版本在最終回歸重新建置。

warnings-as-errors 額外檢查：223 passed、1 failed，Gradio 元件建立時未關閉的 Windows Proactor event loop／socket 被 GC 捕捉為 PytestUnraisableExceptionWarning；未全面忽略，未宣稱這項額外檢查通過。

確認 BabelDOC 0.6.3 TranslationConfig 將 table_model 無條件設 None；停止建立已退休的 RapidOCRModel，保留設定格式及 None 參數。使用者啟用舊選項時明確記錄此功能不可用。測試確認不載入該模型，沒有直接修改上游。

## 重構期間發現的既有錯誤

GUI 的 structured TranslationError handler 讀取不存在的 .message，讓 TranslationError／BabeldocError 原始診斷被 AttributeError 覆蓋。兩項新增測試在修復前失敗，改為保留可選 .message 並回退 str(error) 後通過；成功與取消契約一併通過。這是獨立功能修復，不是宣稱所有搬移均零語意差異。

## 最終套件匯出檢查

新增公開星號匯入契約測試，先重現 AttributeError：根套件 __all__ 包含 ClaudeCodeSettings 卻未綁定名稱。補上既有 config 匯出的一行匯入，保留原 API 清單。此修復不觸及翻譯、重試或 IPC；重新建立 wheel 與 EXE。
