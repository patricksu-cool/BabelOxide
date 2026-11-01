# BabelOxide 操作與視覺規則

以 Rust egui 與 PDFium 延續既有翻譯管線。參考 [UI UX Pro Max 的官方 quick reference](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill/blob/main/.claude/skills/ui-ux-pro-max/references/quick-reference.md) 的資訊層次、漸進揭露、鍵盤替代操作、對比與縮放建議；egui 不在其 stack 資料庫內，採適合原生桌面的規則，不引入 Web／Node／Python UI 執行期。

- 主路徑：加入／拖放 PDF → 沿用或切換組合 → 開始翻譯 → 佇列選擇文件 → 原文／譯文／雙語預覽、開啟、另存或開啟資料夾。
- 工作區只顯示常用設定；密鑰、服務細節與其他設定放到基本／服務／PDF／效能／進階。完整 schema 編輯器與型別驗證保留。
- 「論文雙語」「快速單語」只明確改變輸出與術語擷取選項。自訂組合保存語言、輸出模式、頁碼、服務與該服務模型；白名單排除密鑰、網址、檔案、快取、QPS／Concurrency／Timeout／Retry。保存使用既有 ConfigStore 原子寫入，無效組合不部分套用。
- 佇列沿用 TaskManager(1)，不增加排程器或重試策略。逐份狀態來自真實 results／current_file；批次失敗時未執行尾端顯示取消，可手動重試未完成文件。重試沿用提交時的設定，先前 PDF 結果有獨立輸出保護。
- 8pt 間距、16pt 正文與按鈕、24pt 標題；主要開始按鈕使用單一深青色。狀態有文字，不僅依顏色。跟隨系統深淺色與 egui DPI 縮放。
- 小於 900pt 寬時切成工作區／設定／預覽頁籤，避免壓縮 PDF；各設定區可垂直捲動，長檔名有完整路徑提示。最小邏輯視窗 640×520。
- Ctrl／⌘+O 加入、Ctrl／⌘+S 保存、Ctrl／⌘+Enter 開始；egui 的 Tab／Shift+Tab 與原生開檔對話框提供拖放替代路徑。取消按鈕與安全關閉沿用 Worker 清理流程。

桌面操作、高 DPI 與平台實測範圍記錄於 [STATUS](STATUS.md)，未實測項目不視為已通過無障礙或跨平台認證。
