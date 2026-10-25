# Windows 候選版建置與交付驗證

已建立乾淨的 `dist/release/` 與獨立 ZIP，完成本機隔離使用者環境的 EXE、GUI、CLI、PDF、退出及完整性驗證。**這是候選版，不是已通過獨立乾淨 Windows 驗收的正式發行版。** 本機沒有 Windows Sandbox，亦未提供 VM；已有系統 runtime，不能模擬完全沒有開發工具的另一台電腦。

## 建置與版本來源

共用 `script/build_windows.ps1` 使用獨立環境 `.test-output/round4/release-env`，依 `uv.lock` 執行 no-dev、no-editable 安裝；137 套件的 `uv pip check` 通過。主要 Windows workflow 已共用鎖定流程，詳見 `06_build_preparation.md`；未執行遠端 CI、推送或發佈。

- embedded Python 3.13.16，官方 ZIP 的 SHA256 與 PSF 有效簽章已核對；沒有變更系統 Python。
- PyStand 1.1.4；runtime、PyStand 與主程式授權均保留。
- Gradio 6.30.0、Pillow 12.3.0、BabelDOC 0.6.3、PyMuPDF 1.26.7、filelock 4.2.4。
- 保留 DirectML profile，安裝實作 onnxruntime-directml 1.24.4，確認 DmlExecutionProvider 存在。BabelDOC 本身在此 Windows PDF 路徑使用 CPU，未宣稱 GPU 加速或效能驗收。
- `BUILD_INFO.json` 記錄 installed wheel 的建置來源 e84a05b、授權／說明補件 fd95cb1、最終 launcher fa48998、runtime 版本、lock 與原生 launcher 雜湊。建置後只補入這些已提交的說明／授權與 launcher；沒有變更依賴或套件程式。獨立來源 ZIP fa48998 包含全部相對應程式變更。

第一個本輪候選保留在 `.test-output/round4/first-candidate-archive`；中間 ZIP 也保留於測試目錄。沒有覆寫第三輪穩定包、原始使用者設定或既有金鑰。

## 交付內容與大小

| 項目 | 結果 |
| --- | --- |
| 執行目錄 | `dist/release/` |
| 完整檔案數（含 manifest） | 16,528 |
| 目錄總大小 | 729,077,981 bytes，約 695.3 MiB |
| ZIP | `dist/pdf2zh-next-2.9.0-round4-rc-win64.zip` |
| ZIP 大小 | 264,486,713 bytes，約 252.2 MiB |
| ZIP SHA256 | `75dbed11f8f06e7e6e9315f4a287c350d700a854a10a734766f6cfb21c95b411` |
| 主程式對應來源 | `dist/source/pdf2zh-next-round4-source-fa48998.zip` |
| 官方第三方來源 | `dist/source/third-party/`：BabelDOC、PyMuPDF、Levenshtein sdists，共約 101 MB |

逐檔清單是包內 `RELEASE_FILES.json`，16,527 筆含大小與 SHA256，排除 manifest 自身以避免自我雜湊。所有檔案雜湊、無額外檔案／無遺漏均已核對。7z ZIP CRC 測試通過；ZIP 內檔案集合及大小與其 manifest 相同，另核對最新 launcher、BUILD_INFO、README、設定範本的 SHA256。交付檔及來源封裝雜湊見 `06_release_artifacts.json`。

包含必要 embedded runtime、套件／DLL、GUI CSS／翻譯／圖像、設定範本、首次下載說明、授權與通知。Python `.py` 是執行期套件，不是將 repository checkout 或虛擬環境原封不動交付。剔除 tests／test_*.py、bytecode、環境 hook、direct_url、測試與使用者快取；保留名稱含 `_test.py` 但確為 SciPy 運算功能的模組。Ruff 等 CLI 是 BabelDOC → xsdata[cli] 的已宣告生產相依，不是複製開發環境；沒有強行刪除而破壞相依一致性。

## 本機 EXE 與功能驗證

交付目錄保持未執行狀態，實際測試使用其獨立副本 `.test-output/round4/final-candidate-test`。HOME／USERPROFILE／APPDATA／LOCALAPPDATA／XDG／TEMP 隔離，PATH 僅 Windows 與 System32，工作目錄為非來源根目錄。這仍是同一 Windows 主機，不能取代 VM。

| 檢查 | 結果與限制 |
| --- | --- |
| `--version`、`--help` | exit 0，正確版本／用法；不同工作目錄可啟動 |
| GUI | HTTP 200、337 components；實際只監聽 127.0.0.1，沒有 share link |
| 首次下載 | 第一個候選在空白使用者快取下載官方模型／字型／CMap 成功；最終副本重用本輪隔離快取，未宣稱再次空白下載 |
| 真實 GUI PDF | 第一個候選 SiliconFlowFree 第一頁完成，預覽、mono／dual／ZIP 下載及設定重新讀取通過；詳見第三階段與瀏覽器截圖 |
| 最終副本 CLI PDF | 固定本機回應完成四頁，exit 0、107 requests／36 batches；mono／dual 都產生 |
| 最終副本 PDF 回歸 | 兩種模式各四頁，與第四輪來源版共 8 次 Poppler 96 DPI 比較，全部像素差 0 |
| Ctrl+C | 修復 launcher 後沒有 fatal traceback；獨立 process handle 觀察原生退出碼 0；同步 GUI 在 asyncio.run 中實測需要兩次中斷，保留操作限制 |
| 程序與埠 | 最後檢查候選 EXE／worker 0、17882／17884／17886／17888／17890 的監聽 0 |
| 設定位置 | 第一個候選實際儲存、讀回 EXE 旁 config；原始設定雜湊未變更；隔離家目錄未建立 `.config/pdf2zh` |
| 交付包狀態 | 沒有 config、PDF 輸出或快取；測試狀態只在測試副本／封存候選 |

最後一次完整 PDF 測試之後，唯一產品變更是 launcher 的中斷分支；正常翻譯路徑沒有變更，另外完成其回歸測試與真實 GUI 退出驗證。第一個候選的實際上傳／預覽／真實下載證據仍有效於相同 GUI／PDF 實作，並非聲稱每個中間壓縮包都重新做過真實 API。

證據：`06_final_exe_smoke.json`、`06_native_gui_exit.json`、`06_shutdown.json`、`06_final_exe_pdf_comparison.json`、`06_exe_real_api_preview.png`。PTY 父 PowerShell 接收 Ctrl+C 後回傳 1，不等於 EXE 原生退出碼；早期以此推斷 native launcher 退出碼的結論已更正。

## DLL、資源與外部需求

PE 靜態盤點 441 個映像，全部 AMD64（0x8664）；解析 normal／delay imports 共 88 個 DLL 名稱，皆為包內同名檔、Windows API-set 或本機 System32 可見檔，沒有未分類名稱。這不包含所有動態 LoadLibrary、GPU driver 或 loader 搜尋路徑，不能據此保證另一台 Windows 能執行。

**msvcp140_1.dll 來自本機 System32，未在包內**，所以明列 Microsoft Visual C++ v14 x64 runtime 為外部需求。README 提供[Microsoft 官方下載說明](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)與官方 x64 連結；本輪沒有安裝、升級系統 runtime，也沒有從本機系統目錄複製 DLL 冒充可再散布套件。

此包採首次網路下載模式，未內附模型／字型離線 ZIP；不要把無網路首次啟動或缺少系統 runtime 的情境標示通過。CI 原有離線資產流程仍保留，但本輪沒有遠端建置驗收。開發者模型／字型快取沒有混入交付。

## 憑證、授權與品質界線

已確認根層無設定、憑證、測試資料與快取；9,285 個文字檔納入檢查。四份現有設定依長度與樣板篩選後沒有可做精確比對的非樣板長憑證（candidate_values_compared=0），因此 **0 matches 不是普遍無秘密的掃描證明**。真實測試只用無 key 的免費服務；本輪沒有把使用者設定送入 builder。檔案範圍及檢查限制保留在 `06_bundle_verification.json`。

保留 137 套件 metadata／授權清單；五個 wheel 沒有獨立授權檔，已從各官方原始碼庫補入四份全文（TencentCloud 兩套件共用），來源／雜湊見包內 ADDITIONAL_LICENSES.json。另附三個官方 source distributions，均以 PyPI metadata 的大小與 SHA256 驗證。這些不等於所有原生第三方／系統元件的完整再散布合規認證；正式外發仍需核對完整來源／建置資料與條款。

## 最終檢查與發行判定

- 全部 Pytest **253 passed，18.80 秒**，含原 34 項 retry characterization 與兩個新 launcher 中斷案例。
- Ruff lint 通過；format check **336 files already formatted**。
- 最終嚴格 `-W error`＋tracemalloc：**251 passed、2 failed，75.61 秒**，另有退出時 unraisable ExceptionGroup。配置來源仍為 Gradio loop／socket，不過濾、不刪測試。
- 最新 `uv audit --locked`：210 套件、47 公告，exit 1；相較第二階段沒有新增／移除 advisory ID。Windows 實際版本符合 **41 項 Pillow**；六項 filelock 僅 lock 的 Python 3.10 分支，非本 Windows 4.2.4。

目前適合本人對可信 PDF 做候選試用，**不建議正式對外發行或公開處理不可信文件**。阻礙為 Pillow 相容安全修補未解、Gradio 嚴格資源檢查失敗、缺乏獨立乾淨 Windows／外部 runtime 驗收，以及正式再散布資料與中文全文品質尚未完整驗收。原始版本已有的排版問題依要求保留，不把它當成本輪修復成功。
