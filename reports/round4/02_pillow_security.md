# Pillow 安全分析與保留穩定版本

2026-10-24 查核。第四輪前後均為 Pillow **12.3.0**、Gradio **6.30.0**；本階段沒有變更依賴或第三方程式碼，也沒有宣稱修補任何剩餘漏洞。

## 可相容的修補路徑

[PyPI Gradio 中繼資料](https://pypi.org/pypi/gradio/json) 目前最新仍為 6.30.0，要求 `pillow>=8.0,<13.0`。[Pillow 官方 13.0.0 發行說明](https://pillow.readthedocs.io/en/stable/releasenotes/13.0.0.html) 與逐項公告均指定 13.0.0 修補；[PyPI Pillow 中繼資料](https://pypi.org/pypi/pillow/json) 顯示該版要求 Python >=3.11，而專案仍支援 Python 3.10。沒有可取得的 12.3.x 安全回補，也沒有已發行且允許 Pillow 13 的 Gradio 線。官方 metadata 快照見 `02_official_metadata.json`，實際套件 Requires-Dist 見 `01_environment.json`。

因此選擇保留穩定組合。不使用 override、不移除第三方上限、不改 vendored Pillow，也不為此替換主要圖像處理機制。Python 版本政策變更仍不足以解決 Gradio 上限。

## 掃描與 Windows 實際版本

`uv audit --locked --output-format json`：208 套件、47 筆去重 GHSA，exit 1；Pillow 41、filelock 4.1.0 共 6。與第三輪最終相同。原始輸出保存在 `.test-output/round4/baseline-audit.json`；精簡逐項 ID、aliases、來源、修補版本與分類在 `02_audit.json`。沒有忽略 ID 或停止警告。

直接從第三輪 Windows 發行包的 dist-info 重新盤點（`02_windows_baseline_packages.json`）：134 套件，Pillow 12.3.0、Gradio 6.30.0、filelock **4.2.4**、**沒有 olefile**。按精確安裝版本比對為 Pillow 41 筆；filelock 的 6 筆是 Python 3.10 鎖檔分支，不能算成這份 Windows 發行包受影響。最終新包另重新核對。

## 各項實際風險

- **直接風險**：沒有在本輪可信 PDF／GUI 流程中證明 Pillow 漏洞可被直接觸發；這不代表套件安全。GUI 僅供本人處理可信文件的使用前提很重要。
- **間接風險／待確認**：BabelDOC 的 vendored pdfminer `image.py` 確實包含 CMYK JPEG、JPEG2000 的 `Image.open`，以及 raw `Image.frombytes`。目前翻譯使用 `PDFConverterEx`，不是有 ImageWriter 的 HTML/XML 圖片匯出器；仍保守保留 JPEG decoder、bundled OpenJPEG、native integer 與 raw unpack 四項待確認風險，不以單一樣本證明所有可選路徑都安全。
- **目前安裝不可觸發的前提**：[FPX 公告](https://github.com/python-pillow/Pillow/security/advisories/GHSA-5x8w-8jj5-q52h) 及 MIC 解析需 olefile，本機 stock 發行包未安裝。額外安裝此套件會改變結論。
- **受檢應用路徑沒有對應操作**：其餘 35 項涉及 GIF／TIFF／EPS／WMF／BMP／PNG EXIF／SPIDER／MPO／DCX 等格式或 rank、morph、Arrow、FreeType stroker、PDF append/save_all 等特殊操作。專案預覽 `ui/preview.py` 由 PyMuPDF 開 PDF，再以 RGB `Image.frombytes` 建立最大邊 1600px 的單頁影像；Gradio 輸出儲存 WebP／PNG。實際 GUI config 沒有互動圖片輸入。這是受檢流程的呼叫前提分析，並非整個相依樹的不可利用證明。逐項 ID 分類見 JSON。

已檢查專案、BabelDOC 0.6.3 的 `format/pdf/{legacy_parse,converter}.py`、`pdfminer/{image,converter,high_level}.py`，以及 Gradio `components/image.py`、`image_utils.py`、`processing_utils.py`。Gradio 通用圖片函式確有格式自動探測，不能只憑 `.pdf` 副檔名當安全控制；目前產品未將原始上傳資料傳入這個通用圖片入口。未執行惡意 exploit。

## 緩解與驗證

維持 127.0.0.1、share=False、限定輸出 allowlist、封鎖設定目錄，僅處理可信 PDF；模型與字型快取保持本人可寫。這些既有措施減少暴露，**不算漏洞修補**。等待相容官方版本後再升級與完整重驗；公開或接收不可信文件的正式發行仍受阻。

本階段定向測試 **11 passed，4.19 秒**；完整基準 237 passed。指定 PDF 第一頁單語／雙語輸出、圖片處理與預覽已於基準通過。最終 Windows 新包建置、實際瀏覽器及完整 PDF 回歸在後續階段執行，不預先列為通過。
