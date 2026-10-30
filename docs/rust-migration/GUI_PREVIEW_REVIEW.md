# GUI 與 PDF 預覽可靠性審查

審查日期：2026-10-30。範圍為 `crates/app/**`、`crates/pdf-preview/**`，並只沿 GUI 實際呼叫核對 `TaskManager`、`task_from_settings` 與設定保存介面。圖服務目前不可用，本輪採限定範圍的原始碼與測試閱讀。審查者未修改實作、未執行 Cargo、未操作桌面、未讀取真實憑證。

本輪發現的結果另存問題 GUI01 已由整合者修正；已核對修正原始碼與相關通過紀錄。其餘已讀範圍沒有新增的具體阻擋項。此結論只涵蓋下列來源與 oracle，不代表完整桌面流程、乾淨機器打包或所有原生失敗情境均已驗收。

## GUI01：結果另存可能破壞既有目的檔，已修正

最初的 `background::copy_result` 直接呼叫 `std::fs::copy(source, destination)`。複製過程中的失敗可能讓目的檔只剩部分內容；原本僅以字面路徑判斷同檔，且正常複製測試無法證明錯誤時保留舊檔。

目前先檢查來源類型、絕對目的路徑與相符副檔名，再 canonicalize 來源及可解析的目的路徑以處理同檔別名。來源開啟後，在目的目錄建立 `NamedTempFile`，完整複製、`flush`、`sync_all` 後才 `persist`；錯誤分支沒有先刪除目的檔的 fallback，暫存檔由 owner 清理。GUI 在背景 blocking pool 執行複製，關閉時會等該工作完成。

已核對下列實際測試與 `phase6-gui-atomic-save-tests.log` 的通過紀錄：

| Oracle | 實際證明的範圍 |
| --- | --- |
| `result_copy_uses_exact_paths_without_command_interpretation` | 含空白與 `&` 的來源路徑按資料處理；複製 bytes 正確；拒絕不同副檔名 |
| `result_copy_to_an_alias_of_itself_preserves_the_source` | 含 `.` 的同檔別名保留來源，不產生殘留暫存檔 |
| `result_copy_read_failure_does_not_touch_existing_destination` | Windows 來源被獨占開啟，讀取前即失敗；舊目的 bytes 與目錄項目數不變 |
| `result_copy_publication_failure_preserves_old_destination` | Windows 目的檔不允許刪除，原子發布失敗後舊 bytes 保留且暫存清理；解鎖後可成功替換 |

來源鎖定案例不是「複製到一半」的注入測試；磁碟滿、程序斷電與檔案系統崩潰沒有逐項重現。程式中的原子發布順序支持錯誤前保留舊目的檔，但本輪不把它外推為所有檔案系統上的斷電耐久保證。

## 已核對的生命週期與資料邊界

| 範圍 | 原始碼結論 | 實測證據與邊界 |
| --- | --- | --- |
| 正常視窗關閉 | `NativeApp::begin_close` 將 renderer 移交背景 owner；UI 停止新動作，直到 `done` 才允許 viewport 關閉。背景先等 `TaskManager::shutdown`，runtime 結束後再 `preview.close()`/join，最後發布 `done`。外層 `BackgroundThread` 在原生 event loop 返回後 join | GUI fixture 驗任務啟動、cancel、`done` 前已有 Cancelled、拒絕新提交。沒有把固定 Windows process HANDLE、受控 cache writer 與受控 preview 同時接入一個 GUI close 測試 |
| 任務與 cache 清理 | 背景不 abort translation future；沿用 TaskManager 與 bridge 的 cooperative cleanup。Save、Copy 與 cache open 的 blocking 工作都直接 await；關閉不將它們遺留到背景繼續寫入 | 既有第六階段 167-test run 已涵蓋 TaskManager 與 CXL01 cache 清理；GUI 此層是來源組合核對，不能稱作新的完整 GUI cache cleanup oracle |
| 背景通道 | command 容量 16，UI 使用 `try_send`；event 容量 32，`emit` 同時等待獨立 shutdown Notify，且循環檢查 atomic flag。關閉不必塞入已滿的 command queue；進度使用 watch 最新快照 | 未發現正常關閉路徑的滿載互等；尚無 event queue 填滿且 UI 停止接收後的專門關閉測試 |
| 設定保存 | Editor 在 clone 上套用 typed drafts；Save 明確觸發，使用 `save_user_defaults` 排除 `basic.input_files`，保留已載入的未知欄位。Save 等待期間停用設定修改與拖放，避免 Saved 回覆清掉其後的可見編輯 | GUI persistence fixture 實際核對 unknown 欄位、synthetic credential 保留與 inputs 排除；headless frame 證明一般編輯不自動寫檔。外部同時修改設定時沒有版本衝突偵測，不能宣稱合併另一 writer 的更新 |
| 敏感資料 | `LaunchOptions`、Editor 與 command/event 不 derive 可輸出設定的 Debug；依 `FieldSpec.secret` 使用 password widget；敏感輸入禁用設定套用到該欄位。設定錯誤只回欄位名；Save/Copy UI 錯誤為固定訊息 | 實際 egui paint shapes 不含 synthetic OpenAI key；invalid draft 不回顯值。這是指定敏感欄位與目前顯示路徑的證據，不是任意文字、第三方 panic hook 或所有可能 URL 內容的全域遮罩證明 |
| PDF/result 路徑 | 路徑以 `PathBuf` 傳遞；開啟結果先檢查絕對路徑、存在與 PDF/CSV 副檔名，再以單一 filename 呼叫 `ShellExecuteW`，無 shell command 拼接。預覽保留原生 Unicode 路徑，I/O 在 render thread | 特殊字元 copy、Unicode 原生預覽及有效結果路徑已有測試。OS 檔案關聯及原生另存對話框仍屬桌面 QA 範圍 |
| GUI/Python 邊界 | GUI 與 preview 為原生 Rust/egui/PDFium；開啟 GUI、設定編輯與預覽沒有 Python 啟動呼叫。明確 Translate 才經共用 TaskManager 建立 BabelDOC worker | headless GUI 與空 background shutdown 用不存在的 worker 路徑仍可完成；背景任務測試使用 stdlib 假 worker，不載入 BabelDOC、provider SDK 或呼叫 API |

正常關閉順序不是原生 crash 隔離。`PreviewRenderer::close` 必須等待正在執行的同步 PDFium 呼叫返回；若原生程式卡住，關閉可持續等待。若背景 owner 自身發生未處理 panic，目前也沒有把它納入「一定發布 done 且完整同步清理」的保證。不能以 TaskManager 的 CompletionGuard 推導整個 GUI/原生執行緒具有相同保證。

## PDFium 載入、併發與預覽身分

`platform::bundled_library` 只接受執行檔 bundle 下固定 `native/pdfium/pdfium.dll`；canonical 目錄必須留在 bundle 內，DLL 必須位於該目錄且通過 AMD64 PE32+ DLL header 檢查。可寫的 `PDF2ZH_APP_DIR` 不充當 native bundle root。Windows 先安裝 `SYSTEM32 | USER_DIRS` 搜尋政策，再以 owned cookie 加入受信任 DLL 目錄；沒有 PDF 目錄、CWD、PATH 或系統 PDFium fallback。此 header 檢查不驗證發行檔 hash、簽章或全部 ABI，這些仍由打包與實際 native smoke 負責。

每個 render thread 同時最多一個原生 render、一個待處理 request 與一個待領取 result。`request`/`try_take_result` 使用 `try_lock`，UI 不等原生工作；新 request 覆寫 pending，並以單調 ticket 排除過期結果。初始化失敗先關閉 admission 再發布不依附 ticket 的錯誤，避免新 request 清掉唯一錯誤。GUI 再核對 ticket、generation 與 document key；換文件或 reload 會清舊 texture 並遞增 generation。相同路徑但新 task 使用不同身分。

PDFium 僅在同一 render thread 建立、使用及釋放；每次 render 重新開文件，先釋放 bitmap/page/document/File reader 再發布 owned RGBA。width/height 上限各 1600、checked bytes 長度及不透明 alpha 在 renderer 端驗證，GUI 也檢查尺寸與長度。這個上限只限制單一 bitmap；原生 bitmap、RGBA、ColorImage 與 GPU texture 可短暫並存。PDFium wrapper 的全域 bindings 成功初始化後不可重建 renderer；此限制有明確錯誤與測試。

## 已讀取的驗證紀錄與尚未完成的驗收

| 紀錄 | 已確認結果 |
| --- | --- |
| `.test-output/rust-migration/phase6-preview-native-tests.log` | Preview 13 unit + 1 明確啟用的 native smoke 通過。四種 synthetic 幾何測試涵蓋直向、橫向、Rotate、CropBox、實際 RGBA 顏色及白底；另遍歷指定真實 PDF 各頁、核對上下頁界、missing/invalid PDF。Windows 無分享模式的 File handle 證明發布 frame 後 reader 已釋放，強於僅能 rename 的檢查 |
| `.test-output/rust-migration/phase6-gui-atomic-save-tests.log` | GUI 17 tests 通過，含 GUI01 三個新增案例、原有實際 egui frame/password/drop，以及假 worker 任務提交和關閉 |
| `.test-output/rust-migration/phase6-native-verified-tests.log`、`phase6-native-verified-clippy.log` | 後續完整 workspace log 的非零 suite 合計 205 tests 通過；原生 smoke 在普通 workspace run 中明確標為 1 ignored，其成功依上方獨立 native log。Clippy log 含全部 workspace crate 檢查與成功 Finished。這些紀錄在 GUI01 修正後產生 |
| `.test-output/rust-migration/phase6-core-cli-tests.log` | 前輪整合者的 167 Rust tests 通過，作為 TaskManager/cache/config 既有下層證據；不是這輪新增 GUI/preview 的測試總數 |

Preview unit tests 另實際驗證受控 in-flight render 下的新 generation 淘汰、100 次 pending replacement 只執行首末請求、unread result replacement、factory 初始化期間可提交、Busy 不耗 ticket、panic/invalid pixels 不傳壞 bitmap、close 不 detach 且丟棄 queued work、同執行緒 construct/render/drop。這些 fake backend oracle 與真 DLL smoke 的證據分開計算。

截至本報告落盤，整合者回報已在原生視窗看見原 PDF 翻頁並啟動 BabelDOC；先前桌面 fixture 的 300 秒觀察窗結束時雖有 mono/dual 產物，沒有觀察到 GUI 完成狀態，因此不列為完整 GUI 翻譯成功。新的獨立桌面 QA 尚待其結果；後續 workspace/clippy 結果已依上表實際紀錄更新。沒有因 unit/native smoke 通過而宣稱完整桌面驗收、乾淨機器可攜部署、非 Windows tree cleanup 或無限期 native hang 可強制安全退出。
