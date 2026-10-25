# PDFMathTranslate-next 2.9.0 第四輪 Windows 候選版

這是待驗證候選版，尚未核准正式對外發行。仍有 Pillow 安全公告與 Gradio
資源警告；只供本人處理可信 PDF。SiliconFlowFree 已通過短句及第一頁 GUI／CLI
翻譯測試，完整文件品質與獨立乾淨 Windows VM 尚未驗收。

將整個資料夾解壓到本人可寫的目錄。保留 runtime、site-packages 與 launcher
檔案在一起；不需要另外安裝 Python 或 uv。執行 pdf2zh.exe 開啟 GUI，瀏覽器
會連到 http://127.0.0.1:7860 。設定翻譯服務及自己的金鑰後，上傳 PDF、先試
少量頁面，再下載單語或雙語結果。請先確認服務費率與可用額度。

預設僅本機可存取；不要隨意啟用 share 或對外監聽。關閉瀏覽器分頁不會停止
伺服器；回到程式視窗按 Ctrl+C，程式結束後若顯示 Press Enter 再按 Enter。

首次處理會從 BabelDOC 官方來源下載模型、字型與 CMap 等資源（數百 MB），
需要網路、磁碟空間與本人可寫的使用者快取目錄。此包不含開發者模型快取或
使用者設定，也不含離線資產 ZIP；沒有網路的首次啟動不能完成 PDF 處理。
翻譯服務一般仍需網路。可用 `pdf2zh.exe --warmup` 預先下載並驗證資源。

設定儲存在 EXE 旁的 config 資料夾。程式不以 ~/.config/pdf2zh 為新設定輸出
位置；既有舊設定可能被唯讀遷移，因此乾淨測試使用空白使用者環境。
config.example.toml 只是無金鑰範本，不會自動覆寫設定。

CLI 範例：

```powershell
.\pdf2zh.exe --help
.\pdf2zh.exe --version
.\pdf2zh.exe document.pdf --pages 1 --openai --lang-out zh-TW
```

請先在 GUI 儲存服務設定，勿將真實金鑰放在可被他人查看的命令列或交付包。
啟動失敗時保留錯誤訊息；不要刪除 config 或停用系統安全防護來嘗試修復。

授權：主程式 LICENSE；Python runtime/LICENSE.txt；PyStand-LICENSE.txt；各
第三方套件 dist-info 內的 licenses 與 THIRD_PARTY_COMPONENTS.json。原始碼
另外提供於同次候選交付的 dist/source/，不混入可執行資料夾。對外發行時須
一併提供相對應版本原始碼及第三方通知；目前只完成本機候選交付。
