# 移植安全查核

查核：2026-10-30。這是開發建置及鎖定依賴的查核，**不是正式發行安全認證**。未修改系統安全設定、未使用付費 API 或使用者憑證、未向外傳送 PDF／設定。套件掃描只傳公開名稱與版本。

## 依賴公告

`script/audit_hybrid_dependencies.py` 使用 [OSV 官方 batch API](https://github.com/google/osv.dev/blob/master/docs/api/post-v1-querybatch.md)。最新以新包捕獲的 `WHEELS.json` 查詢 287 個 Windows Cargo registry 版本與 85 個 Worker Python distributions，共 **372 筆**；結果只有 `block-buffer 0.10.4` 一筆聚合公告命中，85 項 Python 未命中。原始回應保留於 `.test-output/rust-migration/phase6-worker13-osv-audit/osv.json`，機器可讀摘要見 [DEPENDENCY_SECURITY_FINDINGS](DEPENDENCY_SECURITY_FINDINGS.json)。

先前基準為 277 Rust + 85 Python，共 362 筆，兩個套件、60 筆公告記錄，其中 Pillow 12.3.0 有 59 筆。GHSA／PYSEC aliases 可能指同一問題，不能稱作 60 個不同 CVE。原始基準仍在 `phase6-osv-audit/osv.json`。新增 HTTP compression 增加 10 個 Windows registry 版本；新掃描使用當前 Cargo resolve，沒有沿用過期 metadata。

| 項目 | 查核結論 |
| --- | --- |
| `block-buffer 0.10.4` | OSV 聚合 range 由 0 起而命中；[維護者公告](https://github.com/RustCrypto/utils/security/advisories/GHSA-qwgh-2vcv-g2f7) 明列影響 `>=0.11.0,<0.12.1`，本版在範圍外。本機鎖版來源亦使用獨立 `pos:u8` 欄位，沒有公告的 inline cursor／ReadBuffer 結構。依 upstream 範圍判為本版誤報，未為清空掃描而盲升不相容版本。 |
| Worker `Pillow 13.0.0` | 基準 12.3.0 的 59 筆命中包含 [JPEG2000 記憶體寫入問題](https://github.com/python-pillow/Pillow/security/advisories/GHSA-78g4-xqwv-q3g8)。已按 [13.0.0 官方版本](https://github.com/python-pillow/Pillow/releases/tag/13.0.0) 的固定 Windows wheel 更新獨立 Worker；新版 OSV 未命中。舊 `uv.lock`／`.venv` 仍是歷史回歸環境，不把它們當安全更新後的產品 runtime。 |

Worker 更新只經 [wheel-overrides.json](../../python-worker/wheel-overrides.json) 更換 Pillow，其他 84 項版本保持不變。wheel 為 `pillow-13.0.0-cp313-cp313-win_amd64.whl`，7,284,777 bytes，SHA256 `9dda0788a9932776a75d8bcc020076edc308ce2508b51f6739a45e3158b3fc42`，來源固定官方 PyPI metadata／files.pythonhosted.org。lock 記錄 inventory、uv.lock 與 profile 的 digest；封裝使用捕獲快照並校驗版本、Python 條件、所有 active extras／Requires-Dist 及精確 85 項閉包。新包的 embedded import／native probe 已通過。

先在測試複本只更新 Pillow，原生 imports／metadata constraints 通過，再執行原四頁 PDF：exit 0、105 個本機 identity 請求、36 batch。mono／dual 各四頁文字、字元座標、圖片與 96 DPI RGB 像素差均為零，見 [Pillow 13 比對](PHASE6_PILLOW13_PDF_COMPARISON.json)。這是固定回應的引擎相容性證據，不是真實翻譯品質或所有 PDF 選項的證明。

Pillow 仍是引擎必要閉包。Rust/PDFium 預覽不經 Pillow；鎖版 BabelDOC 的主要 PDF 流程使用 `format/pdf/new_parser/native_parse`。其保留的 `pdfminer/image.py` 仍含 JPEG／JPEG2000 `Image.open` 與保存，compatibility converter 的 ImageWriter 預設為 None。這些來源降低了「所有公告都必定從目前 GUI 可達」的推論可信度，但**沒有證明全部 engine 選項或第三方動態路徑不可達**。正式發行前仍須處理 Pillow 修正版本與 PDF 回歸，不能將來源觀察當作豁免全部公告。

這次套件 API 沒有涵蓋 PDFium／CPython／ONNXRuntime 的全部內嵌 native library 公告。未命中也不是沒有漏洞的證明。

## 應用邊界與實際證據

| 邊界 | 已實作／驗證 |
| --- | --- |
| Credentials | Rust 設定 Debug 遮罩、安全 HTTP 錯誤、重疊 credential 遮罩後截斷、跨 origin 不轉送 credential；GUI 真 paint output 不含合成 key；Worker start 不含 API key／endpoint。設定 TOML 的保存方式仍沿用明確使用者設定，未宣稱加密儲存。 |
| IPC／程序 | 私有不可繼承 stdio、1 MiB frame、版本/session/task/ID 驗證、有界 writer／進度；Windows suspended spawn 納入 Job 後才 resume，取消等待整樹退出。這是程序與協定隔離，不是 OS 權限 sandbox。 |
| 路徑／輸出 | PDF 路徑與要求產物 canonical 邊界；資產 staging basename/tag；原子保存設定、資源 ZIP 及 GUI 另存；錯誤時保留舊目的檔，無 delete-then-copy fallback。 |
| ZIP | 真 CLI 錯 tag／缺 entry／壞 SHA3 失敗；額外 `../escape-from-archive.txt` 不被解出，182 項正常資產仍 SHA3 相符；見 ASSET_VALIDATION。restore 是逐檔流程，取消不承諾全庫 rollback。 |
| DLL | PDFium archive／DLL 固定 hash、絕對 bundle 路徑、AMD64 PE 與受控 Windows 搜尋；13 unit + 1 真 native smoke。完整 Python native 閉包另由 builder 盤點，不能由 PDFium 成功外推。 |

## Native／授權發行缺口

首輪完整 stage 的 380 個 PE 盤點發現 `pdh.dll` 與 `msvcp140_1.dll`。`pdh.dll` 是 [Microsoft 文件明列的 Windows DLL](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhopenquerya)，已正確分類；ONNXRuntime 的兩個模組仍需 `msvcp140_1.dll`，沒有把它列成 OS 元件或從 System32 複製。

[Microsoft 再散布文件](https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files) 對 Redistributable package／單檔的分發有授權條件。正式包仍需固定且可再散布的來源、條款與實際載入證據；本機已有 runtime 不能代替這些項目。診斷模式明列缺項且 `release_ready=false`，不代表無預裝 VC runtime 的機器已通過。

完整模型／字型／CMap／tokenizer notices、AGPL 對應來源、全部 Rust license texts與乾淨 Windows VM 尚未全部完成。安全更新後的 imports／依賴條件與固定 PDF 回歸已有上列證據，不能代替這些門檻。保留既有穩定 Python 原始碼與候選，避免在功能／SDK 差異未關閉前刪掉比對基準。
