# Engine maintenance 操作審查

日期：2026-10-30。Astra 唯讀審查 bridge共用session、`assets.rs`、Python worker維護dispatch、CLI維護路徑與新增fixtures。未修改實作、未執行cargo或Python測試。以下以本次直接讀取的source為準；父代理回報graph服務不可用，未安裝或重配工具。第6階段先前167項通過結果不能套用到本次新增維護操作。

本輪已核對 `.test-output/rust-migration/phase6-app-maintenance-tests.log`：bridge 4 unit＋19 integration、CLI 3 unit＋6 process tests通過，包括下列ASSET01／02／03案例。父代理另回報Python測試277（tests＋worker32）及cache8，共285通過；此Python數字採父代理結果，審查者沒有重跑或據此宣稱真assets下載／還原已驗收。

## 具體發現與狀態

| ID | 類別／觸發 | 修正或驗證要求 | 狀態 |
|---|---|---|---|
| ASSET01 | CLI行為：`Invocation.load`／Settings.from_layers不做完整validate；warmup=false且generate與restore同時設定，原先直接選generate，忽略restore | 在選擇maintenance操作前明確拒絕互斥組合；保留warmup既有優先語意。CLI fixture驗非零退出且worker從未啟動 | 已修正並通過 `conflicting_asset_operations_fail_before_worker_starts`；核對append式worker-starts marker不存在、無finish且輸出目錄未建立 |
| ASSET02 | oracle缺口：absolute maintenance deadline的source不被progress續命，原先asset_hang只測手動cancel | 短deadline＋持續progress、沒有finished，驗deadline觸發、舊archive不變；程序樹退出另有固定HANDLE oracle | 已補並通過 `resource_progress_cannot_extend_the_operation_deadline`；150ms deadline對每10ms發progress的worker仍逾時，容量1進度通道未讀也不阻塞終止。此case沒有直接檢查固定HANDLE，不能獨自證明deadline路徑的整樹退出 |
| ASSET03 | oracle缺口：原先cancel發生於worker尚未finished，未進Rust copy／persist；成功replace只比最終內容 | 補發布階段拒絕replace，驗舊檔byte-identical、temp清除。若宣稱copy中取消，仍需受控gate／確定性時序 | 已補並通過 Windows `locked_publication_target_keeps_old_archive_and_removes_temporary_file`；鎖定目的檔時保留舊bytes並清temp，解鎖後成功。不是copy中途取消oracle，也不證明ZIP內容完整性 |

## 已核對的source契約

`AssetOperation`與PDF wrapper分開，不建立假PdfJob、假翻譯器或API credential。SessionPlan的services=None；維護worker若要求translate，立即protocol error。ready中的operations capability需包含指定操作，仍使用v1、session、task_id、bounded JSONL與原有stdin/stdout隔離。

`run_session`抽出既有process owner：保留持久partial-frame buffer、子程序exit觀察、exit0後短drain期限、bounded writer/progress、stderr排出、cooperative provider／cache join及Windows Job terminate-and-wait。`finished`仍要求無pending翻譯且result為object；成功返回還需child exit0與tree cleanup成功。PDF結果路徑及必要mono／dual驗證移到共用runner返回之後，現source未發現這項移動削弱既有清理保證；本輪19項lifecycle回歸已包含既有PDF terminal／output、process tree、CLI provider、cache cleanup與bounded progress案例，全部通過。這些controlled fixtures不取代真PDF與桌面流程驗證。

Worker沿用engine thread，直接呼叫BabelDOC的warmup、generate_offline_assets_package、restore_offline_assets_package。API返回None不當作失敗，改由get_offline_assets_tag建立receipt。調用後與finished前再檢查取消；SystemExit亦由BaseException處理成安全terminal error。引擎下載沒有cooperative取消入口時，host可於grace後終止Job並等待；cancel_ack僅代表收到取消，不代表清理完成。

Generate只把私有staging目錄交給worker，未給最終archive目的檔。收到成功session結果後，Rust驗operation kind、64位hex tag、精確basename、canonical source位於staging且為file。來源大小限定大於0且不超過2GiB；分塊複製到目的目錄內NamedTempFile，await每次write，flush與sync_all完成後，再檢token，最後persist取代。沒有delete-then-copy fallback。取消／錯誤在commit前返回會由temp owner清除暫存檔。

`persist`是不可分割的發布動作；最後取消檢查到同步persist間沒有await，但並行取消仍可能與commit競速。已完成commit只能得到完整新檔，不能承諾任意晚到的取消都恢復舊檔。來源size檢查不是ZIP CRC或manifest完整性驗證，不能將fake byte payload的成功發布說成真assets package驗證。

維護操作把worker.idle_timeout用作啟動後absolute operation deadline；progress不延長此deadline。它不包括startup及後續cleanup耗時。PDF仍依原本activity更新idle deadline。CLI的JSON輸出失敗會設取消token並持續await維護future；operation完成清理後才返回stdout錯誤。現fixtures尚未從CLI層證明broken stdout、Ctrl-C與資源維護的整條路徑。

## 目前oracle實際覆蓋

五個Rust maintenance lifecycle tests全部使用純stdlib fake worker，不呼叫BabelDOC或網路：

1. 三種維護操作在刪除fixture PDF後仍可運行；fake worker拒絕收到job／translator欄位，generation驗發布內容。
2. error、finished後非零退出、錯tag、basename traversal、缺archive、非法translate六種情境均失敗，舊archive byte-identical且無staging殘留。
3. 成功generation替換既有archive，最後目錄僅保留目標檔；沒有並行reader，因此本case本身不證明任意時刻都未觀察到partial檔案，原子性另依persist source。
4. asset_hang先建立descendant；取消前取得固定Windows HANDLE，maintain_assets返回後立即驗root與descendant已退出，舊archive不變且無staging殘留。
5. asset_progress_forever每10ms發progress，150ms absolute deadline仍使操作以timeout失敗；保留未讀的容量1進度通道可證terminal不依賴UI消費進度，舊archive不變且無staging殘留。此worker未建立descendant，測試也未取得固定HANDLE。

發布函式另有Windows locked-target unit test，直接走Rust copy／flush／sync／persist，不只在worker執行期間取消。CLI process fixtures則確認三個操作無PDF／provider credentials也可成功，且衝突操作在spawn前拒絕；它們仍使用fake assets receipt與任意byte payload。

Python新增mock cases核對：三個assets API的參數與None返回receipt、不建立pending translator request；API返回前取消不報成功；SystemExit變成安全error且無finished；未知operation不啟動engine。這些mock不驗真正下載、ZipFile生成、restore tag／entry／SHA3行為。

## 明確界限

鎖版BabelDOC restore按可信manifest中的已知名稱讀ZIP entry，不使用extractall；既有hash正確的cache檔會跳過。它逐檔寫入後驗SHA3，不是整個cache的transaction。中斷可能留下缺檔／壞檔，應重跑修復；不能承諾restore失敗時整個cache回到原狀。真restore的錯tag、缺entry、hash壞、額外traversal entry，以及真generate的ZIP內容，尚無本輪runtime證據。

ASSET01修正與ASSET02／03新增oracle已在上述通過紀錄內，本輪限定範圍未留下已證實的source阻擋。完整copy中途取消、CLI broken stdout／Ctrl-C、真assets ZIP與restore hash分支仍保留為未驗邊界。這份審查不宣稱維護操作已完整驗收，也不外推非Windows tree cleanup或異常host強制終止的同步清理保證。
