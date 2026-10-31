# 疑難排解

## Worker 或 BabelDOC 無法啟動

使用完整程式資料夾，直接執行 `pdf2zh-gui.exe`；不要只搬 EXE。確認 `runtime/python/python.exe`、`babeldoc/worker.py`、`resources/worker-home/` 與 assets manifest 都存在。正式包不需安裝 Python、Rust、uv 或手動設定 Worker 路徑。

缺少套件時先核對 `RELEASE_FILES.json`／`THIRD_PARTY_COMPONENTS.json`，依鎖定 profile 重建整包，避免向 embedded runtime 任意 pip install。BabelDOC 所需的 openai、Peewee、tenacity、Pillow、ONNX Runtime 等套件仍須保留。

## DLL 與預覽

ONNX Runtime 的部分模組需要官方 Microsoft Visual C++ v14 x64 Runtime；目前個人包沿用本機已安裝的 Runtime，未內附 `msvcp140_1.dll`。需要時使用 [Microsoft 官方安裝說明](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170)。不要從 Windows 系統目錄複製 DLL。包內 `NATIVE_DEPENDENCIES.json` 列出靜態 import 與尚未解析項目；本機成功不代表乾淨 Windows VM 已通過。

預覽只載入包內 `native/pdfium/pdfium.dll`，不會猜測 PATH 中的其他版本。若預覽失敗，核對 DLL／版本檔、PDF 是否仍存在或被其他程式獨占鎖定，使用 Reload。翻頁／縮放先等待目前渲染完成；有界佇列會丟棄舊結果。圖形驅動或 native 函式長時間阻塞仍可能影響關閉，未涵蓋所有驅動組合。

## 翻譯 API

先確認服務、Base URL、模型與 Key，再少量測試自己的 API。只開啟 GUI 或保存設定不會呼叫翻譯服務；開始翻譯會執行原版初始化健康檢查，可能計入服務用量。術語服務依設定可能另有一次健康檢查。

HTTP 400／401／403 通常是參數、權限或 Key；429／5xx 的重試層、次數與等待依服務而異，見 [相容性表](PROVIDERS.md)。重試有界，但回應遺失時無法承諾遠端服務絕不重複計費。不要將含 Key 的設定檔、環境內容或完整 provider 回應貼入公開問題。

Tencent 目前支援設定中的靜態 ID／Key，以及完整環境 ID／Key 的優先覆蓋；不支援 profile、CVM／TKE 動態憑證鏈。完整 TKE 設定會明確報錯。HTTPX write/pool timers、企業 proxy／自訂 CA 等 SDK 邊界也未宣稱完全等價。

## 設定、cache 與路徑

預設設定為 EXE 旁 `config/config.v3.toml`，Rust cache 為 `cache/translations.v2.db`，BabelDOC HOME 為 `cache/babeldoc-home/`。不依賴開啟程式時的工作目錄。保存設定會保留未知欄位；壞掉或未來版本設定不會被載入流程覆寫。

資料夾不可寫時，GUI／CLI 顯示使用者資料備援路徑。備援依來源資料夾識別，已有設定不會被初始化取代。`PDF2ZH_APP_DIR` 可明確選擇設定與 cache 根目錄；`--worker-home` 與 `--cache-file` 可另外指定。選擇自己可寫的位置，並保持完整 bundle 可讀。

Windows 的超長路徑還受啟動器與原生套件限制。Worker 使用獨立暫存工作目錄，並優先採用磁碟既有的 8.3 短檔名啟動 Python，以避開 OpenCV 的原生長路徑限制；不會修改系統政策。若磁碟未提供短檔名，或 Windows 啟動器拒絕超過 260 字元的 EXE 路徑，請把完整程式資料夾移到較短的可寫位置。開發 smoke harness 會為超長 EXE 加上 Windows extended-path prefix。

若 BabelDOC 資產與內附 manifest 不同，程式會保留檔案並停止。先備份自己的模型／字型，再改用新的空白 `--worker-home`；不要直接刪除不明資料。清 cache 前關閉所有任務，保留需要的 glossary 與輸出 PDF。

取消可能先顯示清理中，正常情況會等待 Worker 結束。不要在清理中重複點擊或搬動整包。沙箱限制與乾淨機／不同實體磁碟的未驗證範圍，以 [最終狀態](FINAL_STATUS.md) 為準。
