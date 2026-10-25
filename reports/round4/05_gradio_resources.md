# Gradio event loop 與 socket 資源警告

已定位為 **Gradio 6.30.0 同步元件初始化的上游 loop 所有權問題**，不是本專案翻譯 task 未取消的同一個問題。沒有修改第三方內部程式碼，沒有加入 sleep、強制退出或全域 warning filter 來讓結果消失。

## 最小重現與配置來源

新增可獨立執行的 `script/reproduce_gradio_resources.py`，完全不匯入 pdf2zh_next。兩次 Gradio helper 呼叫即可顯示先前 loop 被替換且未關閉；兩次單純 `Blocks`／`Textbox` 建構與 `demo.close()` 後，GC 捕捉 **11 個 unclosed ProactorEventLoop ResourceWarning**。全部警告保留於 `05_gradio_mre.json`，工具回傳 **exit 1**，不將捕捉當作通過。

```powershell
.\.venv\Scripts\python.exe script\reproduce_gradio_resources.py --output reports\round4\05_gradio_mre.json
```

`-W error`＋tracemalloc 的單一 GUI 測試檔重現：**17 passed、1 failed**，配置堆疊直接指向 `gradio/utils.py:124` 的 `asyncio.new_event_loop()`，呼叫者包括 routes App lock 與 queueing locks。完整嚴格檢查在新增 API 測試完成後為 **240 passed、2 failed**，另有退出時 unraisable ExceptionGroup；失敗測試名稱受 GC 時機影響，包含 GUI metadata 與 Azure retry 測試，但配置堆疊仍是 Gradio。

CPython `BaseEventLoop.__del__` 先發 ResourceWarning 再 close；`-W error` 會使 destructor 在警告處中斷，接著可出現底層 socket 警告。這解釋為何嚴格模式還會看到 socket，不能據此指認某個被 GC 打斷的測試洩漏網路連線。

## 官方核對

[Gradio 上游原始碼](https://github.com/gradio-app/gradio/blob/main/gradio/utils.py) 仍在沒有 running loop 時建立並設定新 loop。[Issue #11728](https://github.com/gradio-app/gradio/issues/11728) 與 PR #11734 是此 helper 的歷史 deprecation 修正背景；該 Issue 關閉 **不表示本輪發現的所有權／資源警告已修好**。[官方 changelog](https://gradio.app/changelog) 與 PyPI 目前最新仍是 6.30.0，本機最新版本仍可重現，沒有已確認安全且相容的發行版修補可採用。

## 專案流程驗證

`05_lifecycle.json`：同一 GUI **三次正常啟動／close／同端口再啟動均通過**，每次 HTTP 200，close 後 17880 無法連線。Harness 只為了可呼叫 close 使用官方 debug=False、prevent_thread_lock=True；產品預設參數未改。

另啟動真正 Windows multiprocessing 翻譯 worker，確認已到達受控 loopback API 後取消上層 task；server／thread 正常關閉、**新殘留子行程 0**。該 fixture 同時回傳合成 400，取消與失敗競爭時記錄既有 closed-pipe WinError 232 診斷；沒有憑此宣稱所有取消時序零錯誤日誌。既有 GUI cancel 與 IPC 關閉 characterization 亦通過。

標準完整回歸 **242 passed、0 failed**，嚴格模式仍未通過。這是如實保留的 upstream 問題，沒有把測試降低標準或把相關測試刪除。長時壓力、瀏覽器斷線後的長工作、硬關視窗全部時序未驗證；只關瀏覽器分頁不等於停止 Gradio 伺服器。Windows EXE 啟動／退出與新包測試列於下一階段。

第六階段另確認本專案 PyStand 啟動器未捕捉 KeyboardInterrupt：兩次 Ctrl+C 都先關閉伺服器、釋放程序，隨後印出 traceback 並 exit 1。已在 asyncio.run 返回清理之後加入專門的中斷處理，以標準中斷碼 130 結束，沒有強殺或跳過清理。新增回歸測試建立真正 pending asyncio task，驗證 finally 被執行、退出碼及沒有 fatal traceback；3 個 launcher 測試通過。這項啟動器修復不能當成 Gradio 上游 ResourceWarning 已修好。
