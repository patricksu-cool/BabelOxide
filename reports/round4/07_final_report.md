# 第四輪總結與發行判定

本輪可在現有環境安全執行的工作已完成，保留原 Git 歷史，分支為 **maintenance-round4**。已建立可回溯的 Windows 候選包並完成真實免費 API smoke test；**目前不建議正式對外發行**，沒有把缺少 VM、上游安全公告或嚴格測試失敗宣稱通過。

## 完成的修復與驗證

1. 重新建立基準：237 tests 通過，實測 GUI／CLI／Config／PDF／舊 EXE，並保存環境版本。
2. Pillow 實際版本、Gradio 限制與官方公告逐項分析。Pillow 保留 12.3.0，沒有忽略相依限制強裝 13；41 項 Windows 版本符合公告仍在，沒有宣稱本輪已消除漏洞。六項 filelock 公告屬 lock 的 Python 3.10 分支，實際 Windows 4.2.4 不符該受影響版本。
3. 取得使用者「可用 siliconfree」授權後，以無 key 的 SiliconFlowFree 完成短句、指定 PDF 第一頁 GUI／CLI 真實翻譯；單語、雙語、預覽、下載與設定重新讀取通過。沒有動用原本設定中的憑證，沒有執行付費服務或真實 429／Timeout 壓力。錯誤處理使用本機 HTTP／Mock。
4. 原始 v2.9.0、第三輪、第四輪與原始程式＋新依賴受控對照，共 24 個逐頁比較皆像素差 0；最終 EXE 另有 8 個比較同樣為 0。已觀察的既有重疊／斷字未證明為後續 regression，因此不改核心排版。
5. Gradio 6.30.0 同步建構可獨立重現 11 個 unclosed loop 警告；沒有修改第三方內部或過濾警告。GUI 三次啟閉、真實 worker 取消均驗證清理。另修復本專案 launcher 的 Ctrl+C traceback，回歸測試確認 pending task 的 finally 執行；真實 EXE 原生退出碼 0、候選程序與測試埠無殘留。同步 GUI 仍實測需兩次 Ctrl+C，未更改全域 signal 行為。
6. 修復 release／rebuild／fork Windows 流程未共用鎖定 builder 的問題；保留 DirectML／離線資產流程，增加打包契約。清除交付中的測試、bytecode、環境 hook、設定及快取，補入授權、無 key 設定範本、繁中說明、元件與逐檔雜湊清單。必要 Python 執行期模組及上游生產依賴仍保留。

Retry／Backoff、Timeout、Rate Limiting、Concurrency 的原政策沒有改動；原 34 項 characterization 通過，26 個 retry 相關檔案／函式 AST 與穩定基準一致。

## 最終數字

| 檢查 | 結果 |
| --- | --- |
| 標準完整 Pytest | **253 passed、0 failed，18.80 秒** |
| 嚴格 `-W error`＋tracemalloc | **251 passed、2 failed，75.61 秒**，另有退出時 unraisable ExceptionGroup |
| Ruff lint／format | 通過；336 files already formatted |
| 發行環境相依一致性 | 137 packages，uv pip check 通過 |
| 最新 lock audit | 210 packages、47 公告，exit 1；與第二階段 advisory ID 無增減 |
| Windows 版本符合的公告 | Pillow 41；filelock 0 |
| 候選檔案／大小 | 16,528 files；729,077,981 bytes |
| ZIP／CRC | 264,486,713 bytes；7z 測試通過 |
| PE 依賴盤點 | 441 AMD64 映像，88 DLL 名稱；系統需求仍須實機驗證 |

253 相較基準增加 16 個案例：API 傳輸 5、Windows 打包 9、launcher 中斷 2。嚴格檢查的兩個失敗為 GUI metadata 與 Azure retry 測試執行時 GC 報出的 Gradio event loop／socket 配置警告，並非已證明這兩個業務行為失敗；完整堆疊與上游最小重現保留，不把一般模式通過等同嚴格模式通過。

## 交付位置

- [Windows 發行測試報告](06_windows_release.md)：完整建置、範圍、退出與外部 DLL 需求。
- [執行候選 ZIP](../../dist/pdf2zh-next-2.9.0-round4-rc-win64.zip)，解壓後保留整個 release 目錄。
- [候選使用說明](../../dist/release/README_zh_TW.md)與 [EXE](../../dist/release/pdf2zh.exe)。
- [主程式對應來源](../../dist/source/pdf2zh-next-round4-source-fa48998.zip)；[來源說明](../../dist/source/README_zh_TW.md)，另有三個官方第三方 sdist。
- [交付雜湊](06_release_artifacts.json)、[完整包驗證](06_bundle_verification.json)、[最終檢查](06_final_checks.json)、[安全掃描](06_final_audit.json)。

ZIP SHA256：`75dbed11f8f06e7e6e9315f4a287c350d700a854a10a734766f6cfb21c95b411`。

補件與 launcher 的來源 commit 各自記在 BUILD_INFO；來源快照 fa48998 對應最終產品程式，後續提交僅新增驗收報告。所有執行測試使用隔離副本，`dist/release/` 沒有被執行後的 config／快取污染。沒有覆蓋第三輪穩定包，沒有推送或發佈。

## 分階段報告與 Git 紀錄

| 階段 | 證據 | Commit |
| --- | --- | --- |
| 基準 | [01_baseline.md](01_baseline.md) | c46c120 |
| Pillow | [02_pillow_security.md](02_pillow_security.md) | 1c42607 |
| API 傳輸 | [03_api_validation.md](03_api_validation.md) | eef2c9c |
| PDF 對照 | [04_pdf_layout.md](04_pdf_layout.md) | b2db50d |
| Gradio 重現／清理 | [05_gradio_resources.md](05_gradio_resources.md) | 19f38dd |
| 鎖定建置及打包 | [06_build_preparation.md](06_build_preparation.md) | 6780cc7、325d806 |
| 授權後真實 API | [03_real_api_pdf.json](03_real_api_pdf.json) | e84a05b |
| 授權及 VC runtime 補件 | Windows 說明／官方文字 | 3795ff5、fd95cb1 |
| launcher 中斷修復 | asyncio 清理／GUI 與 CLI 退出語意 | 2f0d7ae、fa48998 |
| 最終候選驗收 | [06_windows_release.md](06_windows_release.md) | 4b887bd |
| 最終整理 | 本報告與 reports 索引；提交 hash 見 git log | 本報告提交 |

## 尚未完成與正式發行條件

1. **相容安全修補**：取得並驗證 Gradio／Pillow 可相容方案，或有充分證據的最小緩解；目前仍有間接與待確認路徑，不能宣稱 41 項全不可觸發。不要以 override／忽略限制消除掃描數字。
2. **Gradio 資源警告**：上游 loop 所有權問題需要安全修正版或經證明的修補；嚴格檢查目前未通過。取消／關閉的全部競爭時序與長時壓力也未全部驗收。
3. **獨立乾淨 Windows**：需要 VM／Sandbox，確認無 Python／uv／來源／本機快取時首次下載、VC++ v14 x64 runtime、啟閉、設定、PDF 與下載正常。本機隔離 HOME／PATH 無法取代此項；沒有偷偷安裝或啟用新環境。
4. **再散布資料**：已保留 notices、補五套件授權文字與三份官方來源；仍需核對所有原生第三方元件、完整對應來源／建置資料與正式外發條件。本輪不是完整法務合規認證。
5. **使用品質範圍**：真實測試只涵蓋一頁，模型版本未由代理回應揭露；完整中文文件、所有翻譯服務、所有 PDF、離線首次啟動及 GPU 效能均未驗收。公式附近排版與翻譯術語仍有既有／回應相關限制。

剩餘項目不應以「本機能跑」標示通過。此候選可供本人使用可信測試 PDF 驗收，尚不適合作為已完成安全與獨立環境驗證的公開正式發行版。
