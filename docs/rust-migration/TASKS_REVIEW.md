# 第 6 階段任務生命週期審查

日期：2026-10-29；續審：2026-10-30。Astra 唯讀審查 `crates/tasks/src/lib.rs` 與 `crates/tasks/tests/batches.rs`，主要涵蓋任務生命週期、排隊、取消、進度與 shutdown；另依父代理要求抽查 config 原子寫入與 cache 取消的重大跨層風險。未改實作、未執行 cargo，也未重審第5階段 provider。執行結果由父代理提供，並另讀現存測試log核對，不把source檢查本身當作runtime證據。

本輪 codebase-memory coverage generation `2026-10-29T09:49:29Z`：tasks lib 為 `metadata_match / no_recorded_issue`，batches tests 為 `metadata_changed`；因此以直接讀取當前 source 為準，未重建或重配工具。為確認組合契約，另讀 bridge 的取消前檢查、progress try_send、cooperative provider join、worker tree cleanup，以及既有純 stdlib fake worker 的相關 scenario。

10/30續審 coverage generation `2026-10-29T09:55:57Z`：tasks lib 已變動，以直接source為準；tasks batches、config storage、core cache與lib均為 `metadata_match / no_recorded_issue`。此訊號僅表示索引相符，不能代替動態生命週期證據。

父代理統一驗證結果：workspace **167/167 Rust tests通過**，fmt與clippy `-D warnings`通過；worker **26/26通過**。已讀 `.test-output/rust-migration/phase6-core-cli-tests.log` 的suite結果與相關case名稱：Tasks為2個state unit及7個batch integration，bridge為14個integration，包含CXL01的HTTP＋cache cleanup gate。此快照涵蓋下列修正，不代表之後GUI、PDFium preview或engine maintenance新增實作已驗證。

## 具體發現

| ID | 觸發與影響 | 必要修正／oracle | 狀態 |
|---|---|---|---|
| TSK01 | CompletionGuard 在 spawned async body 首次 poll 才建立。submit 後 runtime 在首次 poll 前結束，future 被丟棄卻沒有 guard，外部 TaskHandle 保留 sender，wait 可永遠停在 Queued／Cancelling | spawn 前建立 guard，再 move 進 future。current-thread runtime 只 enter、submit，從未 block_on 即 drop；外部 handle 必須得到 terminal，且無 worker spawn | 已關閉；drop runtime後Failed、worker.pid不存在，source與fixture核對且167項快照通過 |
| TSK02 | 正常完成在 watch 寫鎖外判斷 cancel，再呼叫 finish。兩者之間 cancel 可先把狀態寫成 Cancelling，finish 卻改成 Succeeded／Failed，令已接受的取消被覆寫 | 正常完成時，取消決策與 terminal 寫入放同一 watch 臨界區；明確分開 panic／異常終止。精準測取消先於正常 finalize，以及 finalize 先於 late cancel 兩種順序 | 已關閉；鎖內重查token、獨立unexpected_exit與兩種順序state unit已核對，167項快照通過 |
| TSK03 | failure-stop fixture 只檢查0個結果與Failed；若錯誤實作繼續跑完3個全部失敗的檔案，仍會通過 | 檢查實際 spawn／start 次數或不同檔案的啟動紀錄，確認首個失敗後剩餘檔案從未啟動 | 已關閉此oracle缺口；啟動紀錄僅1行斷言於167項快照通過。部分成功後失敗的結果保留仍是額外覆蓋界限 |
| CXL01 | HTTP provider附cache時，bridge仍依process-only requires_reap決定grace後abort；可drop正在interrupt-and-join的cache future，留下spawn_blocking writer | 將任意cache backend納入cooperative cleanup判斷，正常取消後不能abort該request。HTTP成功後set進受控gate、cancel且grace0，驗release前bridge未返回、cache future未提早drop | 已關閉受測路徑；requires_cooperative_cleanup、main／term接線與ControlledCache gate／Drop oracle已核對，167項快照通過 |

## 已核對的設計

- semaphore 的 permit 涵蓋整個 batch，限制同時執行的批次；每批內 PDF 依序進行。這與 provider QPS、文字 request concurrency 及 BabelDOC pool 分開。
- submit 持 tasks mutex 檢查 closed、登記 weak state 並 spawn；shutdown 先設 closed，再取得同一 mutex 的 inventory。因此已通過 closed 檢查的 submission 會納入 shutdown，之後的 submission 會被拒絕。
- 排隊時 biased select 優先處理已取消 token；取得 permit 後、每個檔案開始前再次檢查取消。bridge 在啟動前亦檢查 token。已排隊但取消的 batch 不應啟動 worker。
- 執行中的取消只送 token，沒有對 bridge future 使用 abort 或 cancellation select。進度接收與同一個 pinned bridge operation 一起 poll；正常路徑等 bridge 返回後才寫 terminal，因此沿用 bridge 的 cooperative kill／wait 契約。
- progress channel 容量32，bridge 使用 try_send，可在背壓下丟棄中間進度；watch 只保留最新 snapshot。terminal 與結果不依賴 progress channel，晚訂閱者及重複 wait 可取得 terminal。這不保證每個進度事件都會送達。
- manager 只保存 weak task entries；正在執行的 future 與外部 TaskHandle 持有 state。完成歷史由外部 handle 持有。這避免 manager 強制保留所有已完成結果，但不是提交佇列長度上限。
- Drop manager 只發取消；需要確認清理完成的呼叫端必須 await shutdown 或 task.wait。完成的 shutdown 表示 inventory 內所有任務已到 terminal，不等於 Drop 可同步等待程序樹。
- CompletionGuard 現在於spawn前建立並被future持有，未poll即drop仍會發布Failed。unexpected_exit與正常finish分開，異常終止不會因自行取消token而偽裝成一般Cancelled。此guard不能聲稱panic、runtime強制結束或host崩潰也會同步完成所有native descendant的kill／wait。

## 目前 fixture 的證據範圍

所有 scenario 使用專案 Python 或 `PDF2ZH_TEST_PYTHON`，執行純 stdlib fake worker；不是 BabelDOC 或真 provider API。

| fixture | 已檢讀的斷言 | 證據界限 |
|---|---|---|
| 兩檔 batch 成功 | completed_files=2、results.len=2、晚訂閱與重複wait維持Succeeded、late cancel不改終態 | 兩個job目前複用同一路徑，未驗不同輸入的結果排序與路徑對應 |
| queued cancel | 第一批佔slot，第二批保持Queued、取消後無worker.pid，第三批可成功 | 有真worker未啟動的檔案證據 |
| shutdown | running與queued均Cancelled、queued無worker.pid、之後submit被拒絕 | 未在此新層另開固定OS HANDLE驗shutdown返回時的程序退出；目前依賴bridge既有測試與不abort的source組合證據 |
| drop manager | running batch被取消，外部handle最終Cancelled | 只保證之後可等待；不應改寫成Drop同步等待 |
| failure stop | startup_crash使batch Failed，無成功結果且有failure；worker-starts.log精確1行 | 現可抓出繼續啟動剩餘檔案的錯誤；未驗先成功後失敗的部分結果 |
| 兩批並行、獨立取消 | 兩者先各有progress；取消第一批後第二批未terminal，之後可另取消 | 證明分離token與兩個slot；未涵蓋submit/shutdown跨執行緒競態 |
| 未poll就關runtime | 只enter與submit的current-thread runtime直接drop；handle由Queued進Failed，worker.pid不存在 | 不需要BabelDOC或實際worker啟動；驗future被drop時guard已存在 |
| terminal線性化 | cancel先發生時預算出的Succeeded／Failed均變Cancelled且不保留failure；完成先發生時late cancel與guard不改Succeeded | 兩種順序的確定性state unit；非壓力測試，不能外推所有scheduler interleaving |

TSK01／TSK02的確定性時序與TSK03啟動紀錄已執行通過。其他尚可補強的oracle為Tasks層正常cleanup前不能提前發布terminal，以及無訂閱者／慢訂閱者時大量progress不阻塞完成；bridge cache cleanup gate已提供其中一段等待鏈的runtime證據。若宣稱panic保證，需另注入受控panic並驗terminal；不能只由一般錯誤fixture推論。

## Config／cache 的限定抽查

`ConfigStore`以同目錄暫存檔write_all、flush、sync_all後persist取代，未先刪除目的檔；Windows sharing/access錯誤僅作有限次重試，沿用失敗回傳的完整temp。migration使用persist_noclobber，racing新檔不會被覆蓋。讀取／parse失敗不自動寫預設值。已讀concurrent writer完整marker／payload與Windows拒絕delete時舊檔不變、temp清除、解除鎖後可save的oracle；相關suite在167項快照通過。此限定source範圍未見新增阻擋，但原子替換不等於多個完整設定snapshot寫入時可避免last-writer-wins。

`CacheStore::run`的gate等待可取消；SQLite busy_timeout為0，以短等待重試Busy；持有操作的取消者發interrupt後await blocking task，才回傳Cancelled。這個設計依賴上層不drop其future。CXL01已改成獨立 `requires_cooperative_cleanup = requires_reap || cache.is_some()`，bridge的main／term任一需要清理即cooperative join；process-only語意仍由requires_reap表示。ControlledCache fixture先確認進入set，再確認收到cancel；grace0且release未給時bridge不得返回、Drop guard不得提早觸發，release後必須cleaned=true才可返回；此case已在167項快照通過。TaskManager本身不abort bridge，因此這條等待鏈可供UI沿用，但尚未由完整UI操作驗證。取消可能與已進行的COMMIT競速；正確保證是返回後無背景writer，不能宣稱任何Cancelled都表示資料列一定不存在。

## 尚未關閉的界限

本輪以第6階段167項通過快照凍結，未見限定範圍內剩餘阻擋。非Windows tree cleanup、異常host/runtime強制終止、UI完整接線、磁碟歷史持久化、提交佇列長度限制不在本輪已驗證範圍。取消與正常完成的線性化由同一watch鎖決定；異常退出使用獨立Failed路徑，不能將這兩種契約混為一談。
