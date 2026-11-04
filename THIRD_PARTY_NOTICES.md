# BabelOxide — 來源、修改與第三方告知

**BabelOxide 1.0.0 是經修改與重新實作的衍生作品。** 更名與 Rust 實作不代表原始功能、演算法或全部程式均由本專案獨立原創。保留上游、既有作者及各貢獻者的權利；沒有虛構新增著作權所有人。

## 上游來源

- [PDFMathTranslate-next v2.9.0](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/tree/v2.9.0)：設定、CLI、翻譯服務與相容行為的來源；[原始 LICENSE](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/blob/v2.9.0/LICENSE) 為 AGPL-3.0。
- [BabelDOC v0.6.4](https://github.com/funstory-ai/BabelDOC/tree/v0.6.4)：PDF 解析、排版、公式／圖表保留與生成引擎；[原始 LICENSE](https://github.com/funstory-ai/BabelDOC/blob/v0.6.4/LICENSE) 為 AGPL-3.0。Python import、套件名稱與第三方版本均保留。
- 上游 v2.9.0 的 pyproject authors／maintainers 與 BabelDOC wheel METADATA 標示作者 **awwaawwa**；BabelDOC v0.6.4 LICENSE 的專案告知另載 `Copyright (C) 2024 <funstory.ai limited>`，其完整原文隨引擎授權保留。其他作者及貢獻者依上游原始碼、Git 歷史與各元件 notices 保留。LICENSE 開頭的 FSF copyright 屬於授權文字，不據此宣稱 FSF 是本程式作者。

## 修改聲明

本分支於 **2026-11-01** 修改：升級固定 BabelDOC 0.6.4；Rust 原生 egui GUI／CLI、服務、重試、限流、設定、快取、任務與 PDFium 預覽的維護及重新實作；最小 Python Worker IPC；工作區、預設組合、佇列與結果操作；BabelOxide 品牌與原創圖示；Windows／macOS 打包與 CI。較早 Rust 移植及其他修改的實際日期與內容保留於 Git 歷史，不將所有改動追溯為同一日期。

本專案以 **AGPL-3.0-only** 提供，完整條款見 [LICENSE](LICENSE)。在適用法律允許範圍內，本程式不提供任何擔保，包含適售性及特定用途適用性。互動介面的「關於 BabelOxide → 授權與第三方元件」可查看授權與來源告知。

## 第三方元件與資產

Rust crates、Python wheels、PDFium、ONNX Runtime、CPython、模型及字型仍受各自授權約束。必要文件保留在原始碼的 `resources/licenses/`；完整執行包另產生 `licenses/`、`THIRD_PARTY_COMPONENTS.json`、`NATIVE_DEPENDENCIES.json` 及 `provenance/`，保留 wheel／Cargo／資產版本、來源、大小與雜湊。BabelDOC、PyMuPDF 等 AGPL 元件、PDFium 及 ONNX 的 notices、Adobe CMap、DocLayout 模型與各字型的授權不可因品牌重整而移除。

`resources/babeloxide.svg` 及從它產生的 PNG／ICO／ICNS 為本專案新增幾何圖示，以相同專案授權提供，沒有使用上游 logo 或第三方字型。

Rust workspace 內缺少獨立授權檔的元件，另保存固定 commit／SHA256 的補充原文。objc2 類元件同時保留目標版本的 LICENSE.md 及已驗證的共用 MIT 文字／署名；其 Apple SDK 條件不因收集 notices 而消失。

`dispatch 0.2.0` 由 Mac 的 objc2-foundation／winit／eframe 路徑引入。已按 Cargo.lock SHA256 核對發布 crate：其 Cargo.toml 宣告作者 Steven Sheldon、MIT，未附獨立 LICENSE 或著作權年份。`resources/licenses/rust/dispatch-0.2.0/` 保留發布 manifest、[OSI 標準 MIT 條款](https://opensource.org/license/mit)與來源告知，並記錄上游 commit。標準條款的 `<YEAR>`／`<COPYRIGHT HOLDER>` 是原範本欄位，沒有代填或宣稱為上游 LICENSE；生成包以 `supplemental-` 區分這些補充文件。缺少授權文字的盤點項目據此處理，不表示取得額外權利或法務認證。

完整 BabelDOC 0.6.4 授權另存 `resources/licenses/python/BabelDOC-LICENSE.txt`，與固定 wheel 內的原文逐位元組相同，保留 funstory.ai 的專案告知。Builder 同時集中收集 ONNX Runtime 的 `ThirdPartyNotices.txt` 及其他 wheel notices；字型附完整 OFL 1.1 與 NAVER 官方 MaruBuri 條款，逐檔著作權／保留名稱仍保留在資產 notices。

## 散布與網路使用

公開原始碼、提供執行檔或以網路提供修改版功能前，應依 AGPL §5、§6、§13 的適用條件提供修改告知、適當法律告知及對應原始碼／取得方式。對應原始碼包括實際使用的 Rust、Worker、打包腳本、lock／manifest 與必要建置資料；套件 hash 與本清單本身不是原始碼提供義務的替代品。未宣稱已發布公開來源網址、正式法務認證、簽章或公證。

散布執行檔時，應同時提供與 BUILD_INFO 來源提交相符的對應原始碼及建置說明／必要依賴來源。`BabelOxide-source-review.zip` 排除私人資料與大型執行資源，供原始碼審查；它不自動取代 AGPL 第三方相應來源或原生元件的散布義務。GUI 已保留容易找到的「關於」入口、無擔保／再散布告知、完整授權與來源／修改頁；本輪僅做來源與非互動式檢查，實際 GUI 驗收由使用者完成。
