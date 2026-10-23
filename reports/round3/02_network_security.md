# GUI 網路安全

預設明確綁定 127.0.0.1、share=False，Gradio 環境預設不會擴大監聽。刪除失敗後切換 0.0.0.0／公開 tunnel 的 fallback。新增 --server-name，沿用 config／環境設定的動態解析；CLI 與 PyStand 共用 main。明確使用非 loopback 或 share 時記錄安全提醒。指定空的認證檔會拒絕啟動。

移除整個系統 TEMP 的 allowed_paths；上傳檔案由 Gradio 自身 cache 授權，輸出限 pdf2zh_files，logo 為單一檔案。設定目錄加入 blocked_paths。公開 GUI 仍能觸發 URL 下載，僅應在有認證且可信任的網路明確啟用，不是多租戶服務。

單元驗證：本機預設、Gradio 環境值、明確 0.0.0.0 警告、失敗不重試／不分享、空認證檔、CLI 參數。真實瀏覽器／端口與上傳下載留至相容性及最終回歸。
