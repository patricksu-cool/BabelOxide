# Windows 發行狀態

更新：2026-10-30。**已有包含 Rust GUI／CLI、隔離 Python／BabelDOC、PDFium 與完整引擎資產的 Windows 診斷套件；尚未達正式發行門檻。** 沒有 Push，也沒有建立 GitHub Release；原第四輪候選包沒有被覆寫。

## 目前可執行產物

`target/debug/pdf2zh.exe` 為 Rust GNU x64 開發建置。`--version` 純 Rust 可執行；PDF 翻譯須明確指定開發 `.venv` Python、`python-worker/worker.py` 與資產 home。這不能證明使用者不需另裝 Python。

`target/debug/pdf2zh-gui.exe` 已實際以原生視窗完成翻譯、預覽、設定保存、取消、選檔與另存；它不需要 Python 啟動 UI。開發預覽使用固定 PDFium 8125，archive／DLL hash 與 native smoke 皆通過，見 NATIVE_DEPENDENCIES、GUI_TEST_REPORT。翻譯引擎仍需隨包提供隔離 Python，尚未以正式 bundle 驗證。

新診斷包為 `dist/hybrid-diagnostic-20261030-v2`：Rust EXE 來源 `f17abb8`，CPython 3.13.16、BabelDOC 0.6.3、85 項 engine distributions（Pillow 13.0.0）、PDFium 8125、已還原的 182 項資產。manifest 列 **13,042 檔、1,248,017,820 bytes**（不含 manifest 自身），SHA256 `3086c43405de7442f47c1cdad1b50515d5929f8f9adbc3e4758bf7d98b276094`。發布後另讀全樹核對完整集合、每檔 bytes／SHA256，沒有生成 DB／loose bytecode；見 [完整性證據](PHASE6_BUNDLE_INTEGRITY.json)。首版診斷包另留存，沒有覆寫。

將整包複製到 `.test-output/q 包 v2` 後，不指定 Python／Worker／worker-home，CLI 使用包內預設佈局完成原四頁 PDF，exit 0、105 個 loopback 請求、36 batch、68 progress、1 finish。mono／dual 八頁與 main 文字、座標、圖片、96 DPI RGB 像素差均為零，見 [新包比對](PHASE6_BUNDLE_PDF_COMPARISON.json)。同複本 GUI 由指定 Terra xhigh 實際操作：Complete 1/1 100%、兩種結果預覽與第二頁、再次處理中關閉／安全清理。這是開發機的搬移驗證，沒有另外呼叫開發 `.venv` 作為被測 Worker，仍不能代表沒有預裝 VC runtime 的乾淨 Windows。

Rust 依賴由 `Cargo.lock` 鎖定；本機 Rust 1.99.1，使用 `.cargo/config.toml` 的 bundled `rust-lld` 與 self-contained GNU runtime，不依賴寫死的使用者路徑或全域 PATH 修改。兩支 EXE 的最佳化 release build 已成功；builder 不自行編譯，逐檔 hash 並保存 locks／profile／inventory／builder 快照。未宣稱 EXE bit-for-bit 可重現；授權缺項與完整 native 漏洞盤點仍未關閉，見 [封裝說明](HYBRID_BUILD.md) 與 [安全查核](SECURITY_REPORT.md)。

## Runtime／資產界線

規劃為 Rust GUI／CLI EXE、隔離 Python runtime、最小 Worker、BabelDOC 必要依賴、模型／字型、config 與第三方 notices。現 installed metadata 顯示 BabelDOC 無 GPU extra 的閉包為 85 distributions；不能把原開發 `.venv` 整包當成最小發行內容。`openai`、`peewee`、`tenacity`、Pillow 仍是上游引擎閉包的一部分，不能因 Rust 接管應用邏輯就假定能刪除。

實際 BabelDOC manifest 為 182 項，已真實產生 ZIP 並從空白 home 還原、逐檔核對 SHA3。先前缺四個字型的結論是靜態盤點未扣除 import 時剔除的未引用項目，已更正，見 ENGINE_ASSET_INVENTORY。這不代替整包離線首次啟動驗收。Windows DirectML provider 可安裝也不代表鎖定引擎使用 GPU，見 DEPENDENCIES。

## 未完成發行門檻

- 全部服務剩餘 SDK 相容性與各服務真 API／長時間驗收。
- 診斷包已排除 Gradio／舊 Python app，原始碼保留直到服務相容性缺口關閉。
- Python runtime、ONNX／PDF／字型相關原生 DLL、授權與對應來源盤點。
- 新包未複製個人設定／API key／翻譯 cache／測試 PDF；正式發布仍須最後重核交付內容。
- Pillow 13 已通過固定 PDF 回歸，372 項依賴 OSV 查詢的唯一命中依 upstream range 為誤報；native 內嵌 library 公告與全部可達選項仍須另查。
- 乾淨 Windows Sandbox／VM 的無 Python、無 Rust、無開發工具驗收。本機開發測試無法替代；目前未執行。

新包的大小與本機測試均有上述證據；`BUILD_INFO.release_ready=false` 保留兩個 ONNXRuntime 模組對未內附 `msvcp140_1.dll` 的依賴、asset notices／crate texts／對應來源與乾淨機缺項。沒有從 System32 複製再散布 DLL。後續組包仍須另建輸出目錄，保留現有候選包及使用者設定。
