# Windows 發行狀態

更新：2026-10-27。**目前只有 Rust CLI 開發原型，不是完成的 Windows 發行包。** 沒有 Push，也沒有建立 GitHub Release；原第四輪候選包沒有被覆寫。

## 目前可執行產物

`target/debug/pdf2zh.exe` 為 Rust GNU x64 開發建置。`--version` 純 Rust 可執行；PDF 翻譯須明確指定開發 `.venv` Python、`python-worker/worker.py` 與資產 home。這不能證明使用者不需另裝 Python。

Rust 依賴由 `Cargo.lock` 鎖定；本機 Rust 1.99.1，使用 `.cargo/config.toml` 的 bundled `rust-lld` 與 self-contained GNU runtime，不依賴寫死的使用者路徑或全域 PATH 修改。正式可重現工具鏈、最佳化 release build、完整 Rust crate 授權與漏洞盤點尚未驗收。

## Runtime／資產界線

規劃為 Rust GUI／CLI EXE、隔離 Python runtime、最小 Worker、BabelDOC 必要依賴、模型／字型、config 與第三方 notices。現 installed metadata 顯示 BabelDOC 無 GPU extra 的閉包為 85 distributions；不能把原開發 `.venv` 整包當成最小發行內容。`openai`、`peewee`、`tenacity`、Pillow 仍是上游引擎閉包的一部分，不能因 Rust 接管應用邏輯就假定能刪除。

本機資產盤點仍缺四個字型；不能宣稱完整離線首次啟動已通過。Windows DirectML provider 可安裝也不代表鎖定 BabelDOC 的實際 PDF 路徑使用 GPU，見 DEPENDENCIES。

## 未完成發行門檻

- Rust GUI、PDF 預覽、全部服務與設定／cache 相容性。
- 無 Gradio／舊 Python app 的獨立 runtime 組包與逐檔 manifest。
- Python runtime、ONNX／PDF／字型相關原生 DLL、授權與對應來源盤點。
- 確認新包未包含個人設定、API key、翻譯 cache 或測試資產。
- 依實際版本重新評估 Pillow 等安全公告及可觸發路徑。
- 乾淨 Windows Sandbox／VM 的無 Python、無 Rust、無開發工具驗收。本機開發測試無法替代；目前未執行。

因此目前沒有新的正式包大小、完整 DLL 驗證或乾淨環境通過結果。後續組包必須另建輸出目錄，保留現有候選包及使用者設定。
