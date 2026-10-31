# GUI 與 BabelDOC 相容性稽核

稽核基準為 `06ac7699a5d8254b87b1ba073e015ed62b28e307`，官方對照為 [PDFMathTranslate-next v2.9.0](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/tree/v2.9.0)（`f8dffcf`）。本文件的「基準」指該 commit，並非已完成之新介面的驗收結論。[GUI_SETTINGS.json](GUI_SETTINGS.json) 記錄嵌入 schema 的全部欄位，分別保留基準與目前程式中的 GUI 控制項。來源判讀是靜態分析，未宣稱實際呼叫每家服務。官方 Windows 大型發行包未下載或執行，本次比較僅依 v2.9.0 原始碼，不是發行包實測。

基準的 Rust workspace、Worker 35 項與 Builder 31 項測試已由主工作流程執行通過；既有 Release GUI 已啟動、截圖並完成介面語言切換。整合新介面後 `cargo test -p pdf2zh-app --locked` 45 項測試通過，包含 URL／ZIP 模組 16 項受控測試。Computer Use 起初授權等待逾時，後已恢復；新 Release 尚待建置，其逐項操作、實際公網下載、翻譯和完整 Release 視覺驗收仍待完成。

## 設定欄位

`crates/config/src/schema_data.json` 有 **235 個不同 path**：Bool 100、String 120、Integer 10、Float 4、StringSet 1。這包含 24 個正文服務旗標、14 個術語服務旗標和輸入檔案欄位，不能描述為「235 個通用文字框」。基準版「所有設定」排除這 39 個欄位，靜態計算呈現 196 個欄位：62 個勾選框、134 個非布林文字框；部分常用欄位另在上方重複呈現。API、URL、指令等部分敏感欄位均依本地 `secret` 標記遮罩，這個標記不只表示密碼。

JSON 每欄保留 path、型別、目前 schema 預設值、本地驗證與官方 GUI 範圍、條件式必填、服務、進階分類、敏感標記及兩版 GUI 呈現。`gui_settings.ui_lang` 基準預設為 `en`，目前預設為 `zh-TW`；已保存設定照常載入。`nullable` 不等於是否必填；「未指定」表示未找到足夠證據，不能當成無限制。選用特定服務時的必填條件以 `crates/config/src/providers.rs` 為準。官方 GUI 的滑桿邊界與本地核心的有效範圍不一定相同，不能逕自把 GUI 邊界當成所有配置的有效範圍。

例如官方自訂 QPS 輸入顯示 1–1000，基準核心允許正的 `u32`；官方 pool workers 的 0 轉成自動，本地正文 `pool_max_workers` 顯式 0 則不通過驗證。本地術語 `term_pool_max_workers` 接受 0 或空值：bridge 傳給 Worker 前將其轉為正文的執行緒數；服務提供的建議值可優先覆蓋該值。官方 GUI 的術語自訂 0 先轉為 `None`。若直接繞過 bridge 把 0 傳給 BabelDOC 0.6.3，則不會觸發其 `None` 預設處理；不能將該直接呼叫的行為誤認為本專案正常翻譯路徑。官方的 `max_pages_per_part` GUI 輸入最小值是 0，而基準驗證接受空值、0 或至少 50。官方短行分割滑桿為 0.1–1.0，本地僅在該功能開啟時檢查最小 0.1。官方兩個圖表／非公式線閾值滑桿和本地驗證皆為 0–1。具體參見 [官方 GUI 原始碼](https://raw.githubusercontent.com/PDFMathTranslate-next/PDFMathTranslate-next/v2.9.0/pdf2zh_next/gui.py)、本地 `crates/config/src/lib.rs` 與 `crates/babeldoc-bridge/src/lib.rs`。

## 官方 GUI 與基準桌面 GUI

- **語言。** 官方兩個下拉與交換按鈕使用相同的 153 項 `lang_map`；`en`、`zh-CN`、`zh-HK`、`zh-TW`、`ja`、`ko` 均為原始碼中明列的代碼。該清單不依服務篩選，因此不是各服務均支援全部語言的證據。基準版使用文字輸入，現有設定預設目標為 `zh`，需保留舊代碼而不是靜默覆寫。[官方語言及頁面選項原始碼](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/blob/v2.9.0/pdf2zh_next/gui.py#L101-L275)
- **服務限制。** 本地 Qwen-MT 的 `qwen_language` 僅接受大小寫完全相符的 `zh-CN`、`zh-TW`、`en`、`fr`、`de`、`ja`、`ko`、`ru`、`es`、`it`；既有 `zh` 與 `zh-HK` 不在此實作的允許清單。DeepL 來源、目標各有白名單及不同映射，詳見 `crates/core/src/providers/legacy/deepl.rs`。Google、Bing、Tencent 有中文別名轉換；對未實證的服務與語言組合，支援情況應標為未知。
- **服務與動態欄位。** 官方 `enabled_services` 以逗號拆分、不分大小寫比對服務名稱；空值提供全部服務，無任何匹配會失敗。基準版服務選單列出所有引擎，未套用此設定；術語服務限 `supports_llm`。官方依選中服務顯示欄位，密碼及其他敏感欄分別由清單管理。DeepSeek thinking 可選未設定、enabled、disabled，reasoning effort 在 enabled 時顯示 high／max；實際 thinking 選項僅用於 `deepseek-v4-` 模型。[官方 provider 設定](https://raw.githubusercontent.com/PDFMathTranslate-next/PDFMathTranslate-next/v2.9.0/pdf2zh_next/config/translate_engine_model.py)
- **頁面與術語。** 官方頁面快捷為全部、第一頁、前五頁、指定範圍；範圍接受不連續及開放端寫法。基準版已有頁面字串解析及 CSV 選擇、自動擷取、獨立術語服務和限流設定；沒有官方頁面快捷與術語停用提示。術語 CSV 僅供支援 LLM 的服務使用；官方提供多檔上傳與內容預覽。不要以官方快捷限制底層原有範圍語法。
- **批次結果。** 官方在單檔下載外，提供全部、單語、雙語和術語 ZIP。基準桌面版有多檔任務及單個結果另存，未見 ZIP 匯出。封裝時應處理重名和部分結果，不應覆蓋其他輸出。
- **網址輸入與安全。** 官方有連結輸入，但其 `requests.get(..., timeout=10)` 的 GUI 呼叫未傳入可選大小限制；所查程式沒有內網／重新導向檢查，不能直接作為安全實作。基準版未見 URL 匯入；若加入，應檢查每次 DNS 解析與重導向、封鎖非公網目的地，限制大小和時間，驗證 PDF，並處理取消與暫存清理。這是本次需求的安全條件，非宣稱官方具備。
- **繁體中文。** 基準版具中英文切換，部分固定標籤已翻譯，但通用欄位顯示 CLI flag 與英文 help，錯誤訊息亦不完全中文；預設設定 `gui_settings.ui_lang` 為 `en`。僅憑程式碼掃描不能宣稱所有畫面均已完成翻譯。
- **資料更新。** 基準 `Editor::add_files` 在逐一驗證時即推入清單，後續檔案無效時可能留下先前加入的部分；設定 snapshot 在翻譯和儲存路徑的驗證範圍不同，需分別核對。這些是基準版觀察，新實作與測試結果須另行記錄。

## 目前桌面實作與剩餘差異

目前程式已整合官方語言清單、交換與自訂代碼，Qwen-MT／DeepL 採本地核心可證實的語言限制；其他服務的支援情況仍需由服務確認。`enabled_services` 控制正文與術語選單，管理區可重新啟用服務，停用不刪除各服務設定。欄位依服務、型別與條件顯示，包括可空布林的三態選擇、數值微調、DeepSeek 模式、短行分割及部分發送參數的相依條件。介面也提供頁面快捷、術語啟用狀態及限流提示、輸入整批驗證和設定儲存前驗證。上述為程式碼及受控測試的結果，尚不等同桌面互動驗收。

網址下載限公開 HTTP／HTTPS、標準連接埠，檢查 DNS 和每次重新導向，限制為 100 MiB、總逾時 60 秒及最多五次重新導向，並檢查 PDF 和支援取消。成功批次可匯出全部、單語或雙語 PDF ZIP；官方另有術語 ZIP，目前桌面版保留單個術語檔另存，尚無術語 ZIP。新設定預設繁體中文，既有偏好設定保留；是否所有實際畫面、錯誤提示和流程均已妥善翻譯，仍需桌面操作驗證。

## BabelDOC 0.6.3 與 0.6.4

[0.6.4 發行說明](https://raw.githubusercontent.com/funstory-ai/BabelDOC/v0.6.4/docs/release-notes/v0.6.4.md) 記載兩項有價值的改動：限制異常大型頁面的光柵化預算以降低記憶體壓力，以及修正 Latin／CJK 混排行距重疊。後者明確會改變受影響頁的垂直排版，因此不應以輸出逐字元座標必須不變來宣稱升級無影響。[0.6.4 變更 commit](https://github.com/funstory-ai/BabelDOC/commit/17480db9df92ddcb37349ce34b312335226e8ec9) 涉及本地及 RPC 版面偵測、掃描檢測、排版及 translator。兩版 `OpenAITranslator` 的來源比較顯示 0.6.4 新增 `thinking` 參數及 cache key；本專案使用 Rust translator adapter，不能因此推定其 Worker 相容性已經驗證。

完整 [v0.6.3...v0.6.4 tag 比較](https://github.com/funstory-ai/BabelDOC/compare/v0.6.3...v0.6.4) 包含兩個 commit、18 個檔案，而非僅 0.6.4 release commit 的 15 個檔案。中間 [980fd28](https://github.com/funstory-ai/BabelDOC/commit/980fd2821d54cbabd270349fe509e8177c35e4c3) 是 CMap manifest、loader 與 ImageWriter 的註解及發行說明補充，所顯示的程式差異未改變執行邏輯。0.6.3 已包括其發行說明所述 CMap 與 ImageWriter 防護；此判斷來自 tag 原始碼與該 commit 差異，不是本地安全測試結果。

[0.6.3](https://raw.githubusercontent.com/funstory-ai/BabelDOC/v0.6.3/pyproject.toml) 與 [0.6.4](https://raw.githubusercontent.com/funstory-ai/BabelDOC/v0.6.4/pyproject.toml) 宣告相同的 Python 範圍 `>=3.10,<3.14` 和相同的主要、optional 套件需求。兩版 [0.6.3 資源中繼資料](https://raw.githubusercontent.com/funstory-ai/BabelDOC/v0.6.3/babeldoc/assets/embedding_assets_metadata.py) 與 [0.6.4 資源中繼資料](https://raw.githubusercontent.com/funstory-ai/BabelDOC/v0.6.4/babeldoc/assets/embedding_assets_metadata.py) 所列 DocLayout ONNX 預期 SHA3-256 均為 `60be061226930524958b5465c8c04af3d7c03bcb0beb66454f5da9f792e3cf2a`；這並非已在新版實際驗證完整離線資源包。兩版的離線資源還原程式會核對資源 tag 與檔案雜湊。

本專案的 `python-worker/worker.py` 和 Rust bridge 握手均明確要求 0.6.3；Windows builder、wheel lock 與資源清單也固定該版本。直接替換 wheel 不能啟動。官方 v2.9.0 原始專案的 [pyproject](https://raw.githubusercontent.com/PDFMathTranslate-next/PDFMathTranslate-next/v2.9.0/pyproject.toml) 宣告 `babeldoc>=0.6.2,<0.7.0`，不能以特定發行包版本反推所有 v2.9.0 安裝使用同一版本。

建議本次保留 0.6.3：0.6.4 的穩定性修正值得後續評估，但新版尚未在本 Worker、固定依賴及離線資源上做隔離驗證，而且有預期排版變動。若另行升級，需同步處理兩端版本檢查、鎖定依賴與建置清單，驗證資源，再以相同翻譯結果比較 PDF 版面及完整 Worker 回歸。未執行這些驗證，不宣稱 0.6.4 可直接替換。
