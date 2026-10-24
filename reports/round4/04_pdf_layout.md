# 原始／第三輪／第四輪 PDF 排版比較

指定文件四頁，在同一台 Windows、同一組 34 個字型資產（SHA256 清單在 JSON）、相同 Poppler 96 DPI、lang-out zh-CN、無浮水印、mono／dual 模式與 loopback identity 固定回應下重跑。每份均忽略翻譯快取、關閉自動術語擷取、正常非 debug 模式。所有原始 PDF 與測試輸出保留在忽略追蹤的 `.test-output/round4/`。

| 來源 | 隔離方式／關鍵依賴 | 結果 |
| --- | --- | --- |
| 原始 `277d1f1` | Git archive；獨立 original-env；BabelDOC 0.6.2、PyMuPDF 1.25.2、Gradio 5.35、Pillow 11.3 | exit 0；107 請求、36 批；mono／dual 各 4 頁 |
| 第三輪 `0147d19` | Git archive；目前鎖定依賴 | 相同 |
| 第四輪 | 目前來源與相依 | 相同 |
| 原始程式＋目前 stack | 額外受控實驗，隔離程式碼差異與依賴差異 | 相同 |

原始 Git 快照沒有 lockfile；先原樣解析，確實重現 BitStream 匯入失敗。只在原始獨立環境將 bitstring 選為 4.4.0、Tencent TMT SDK 選為 3.1.121，均符合其原始寬鬆宣告，與第一輪可執行基準相同。未改原始來源、未修改第三方內部程式碼。`uv pip check` 136 套件相容；該環境只測可信 PDF、本機固定回應，不啟動舊 Gradio 服務。完整版本見 `04_original_packages.json`。

## 可重現結果

`04_pdf_comparison.json`：四組執行皆有完整逐頁文字、公式符號代理值、嵌入圖片／頁數及輸出 SHA。原始 vs 第三輪、第三輪 vs 第四輪、原始程式＋目前 stack vs 第四輪，共 **24 組頁面比較，changed_pixel_fraction=0、mean_channel_difference=0**。檔案 bytes／SHA 仍可能受 metadata、序列化與字型物件影響，沒有以二進位相等作為標準。

mono 圖片物件計數合計 23,998、dual 47,996，各組相同；這是 Poppler 列出的物件數，不能理解成幾萬張獨立圖表。公式符號數與文字雜湊是同流程比較用的代理指標，不能證明公式語意／翻譯品質。

逐頁渲染已產生，人工重點檢查第二頁公式 (4) 與電路圖：輸出存在公式附近重疊、局部斷字／縮排與小字；**在原始 v2.9.0 的輸出已存在**，且所有受控輸出像素相同。原始 PDF 自身也有缺字型與 Type3／替代字型渲染疑慮；Poppler 提示 Symbol、ArialUnicode、Bookman-Light。不能把這些全部歸因於第四輪。

證據：`04_source_page2.png` 為來源，`04_translated_page2.png` 為原始版本處理後；第四輪相同頁的像素比較為 0。有文字／公式與電路圖保留，但版面不完美。

## 結論與不修改範圍

在本次固定回應情境 **沒有發現後續應用修改或安全依賴升級引入的視覺 regression**，因此依要求不改核心排版演算法。既有問題是原始程式／BabelDOC 處理及來源字型的相互作用；尚未逐一證明每個缺陷的單一根因，不聲稱所有 PDF 或真實中文譯文都不會退化。

Identity 會觸發 same-as-input／simple translation fallback；此比較能隔離回應差異並檢查回歸，**不能當作真實中文翻譯品質驗收**。真實 API 尚缺授權，故中文排版與品質仍待確認，未為了這個限制擅自調整排版。
