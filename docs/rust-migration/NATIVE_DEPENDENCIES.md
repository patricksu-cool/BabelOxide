# Windows 原生依賴與組包設計

查核更新：2026-10-30，Asia/Taipei。**新 Rust GUI／最小 Worker 正式包尚未建立，沒有乾淨 Windows VM 驗收。** 10/29 的來源設計後，10/30 已固定下載 PDFium、驗 hash 並通過真 native 渲染與 GUI 操作；舊 `dist/release` 未修改。預覽 API 見 [PREVIEW_DESIGN](PREVIEW_DESIGN.md)；Python 套件版本沿用 [DEPENDENCIES](DEPENDENCIES.md)，完整 runtime 組包仍待驗收。

## 建議交付形狀

第一個目標固定 Windows x64：Rust GUI／CLI、PDFium native 庫、application-local embedded CPython 3.13.16、最小 Worker、BabelDOC 0.6.3 必要閉包、完整引擎資產、授權／對應來源。使用者不需自行安裝 Python 或 Rust；能否免裝其他 runtime，必須由實際 PE 閉包與乾淨機證據證明。

以下目錄**尚未建立**。builder 必須使用新 staging／artifact 名稱，不裁切、覆寫或追加舊候選：

```text
release-hybrid-win64/
  pdf2zh-gui.exe
  pdf2zh.exe
  native/pdfium/pdfium.dll
  runtime/python/python.exe, python313.dll, python313.zip, python313._pth, ...
  runtime/python/site-packages/   # 必要 wheel 內容、metadata、資料
  babeldoc/worker.py
  engine-assets/                 # 完整且驗證過的官方離線來源
  licenses/                      # Rust / Python / native / fonts notices
  sources/                       # 必須提供的對應來源及建置資料
  BUILD_INFO.json
  NATIVE_MANIFEST.json
  RELEASE_FILES.json
```

bundle 位置從 EXE 取得，不靠 CWD、全域 PATH、系統 Python 或 `.venv`。唯讀安裝資料與可寫設定／cache／資產解包目錄分開。Rust 明確設定 Worker 的資產 home；BabelDOC 按 `Path.home()/.cache/babeldoc` 尋找，僅設 XDG 不足。首次恢復官方離線資產應可驗證、可重試，不複製開發者整份快取。

## PDFium 固定來源與雜湊

2026-10-29 已重新確認最新 release 為 **PDFium 158.0.8125.0**，tag `chromium/8125`，2026-10-26 發行；release 頁標示 immutable。選 shared、非 V8 的 **`pdfium-win-x64.tgz`**。發行者 `bblanchon/pdfium-binaries` 是 wrapper 推薦的獨立 binary publisher，**不是 Google／Foxit 官方 binary 服務**；上游原始碼另屬 Chromium PDFium。[release](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium/8125)、[publisher 說明](https://raw.githubusercontent.com/bblanchon/pdfium-binaries/master/README.md)。

| 8125 資產 | 發行者公布的 SHA256 |
| --- | --- |
| [pdfium-win-x64.tgz](https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8125/pdfium-win-x64.tgz) | `9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b` |
| [pdfium-attestation.json](https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8125/pdfium-attestation.json) | `a0964f520afc57593d285b081e23e401169cc42234f2d14f5bdb4a3add08c9b9` |

來源為 [8125 publisher assets 清單](https://github.com/bblanchon/pdfium-binaries/releases/expanded_assets/chromium/8125)，時間 `2026-10-26T15:18:56Z`。10/30 實際下載 x64 archive **3,904,987 bytes**，本機 SHA256 與上列 publisher digest 相符；沒有借用 8111 的 digest。不要用可移動的 `latest` URL 作 build pin。

本輪先驗 archive hash、列出 tar 成員，再在新 staging 解包，保存 LICENSE／licenses／headers／import lib／VERSION／args.gn。DLL **7,586,816 bytes**，SHA256 `4b0dec92555d69437bef0df37be630181ab3e83a6823ca264ede355a4f2ef994`。VERSION 為 158.0.8125.0，args.gn 確認 x64、V8=false、XFA=false。未完成 attestation 簽章／subject 驗證，不能把 JSON digest 當作已驗供應鏈簽章。

`pdfium-render 0.9.4` 的 API feature 為 `pdfium_7881`；本輪與 8125 DLL 實際 bind／render 成功，13 unit + 1 明確啟用 native smoke 通過，含真四頁 PDF、RGBA／Rotate／CropBox／檔案 handle 釋放。固定 pair 的證據見 [測試報告](TEST_REPORT.md)。[wrapper manifest](https://raw.githubusercontent.com/ajrcarey/pdfium-render/master/Cargo.toml)。

publisher 的 [staging script](https://raw.githubusercontent.com/bblanchon/pdfium-binaries/master/steps/07-stage.sh) 會放 `bin/pdfium.dll`、import library、headers、`args.gn`、`VERSION`、`LICENSE`；[授權 script](https://raw.githubusercontent.com/bblanchon/pdfium-binaries/master/steps/08-licenses.sh) 另收集 `licenses/`。動態載入成品不需 C++ headers／import library，但建置 provenance 應保存；LICENSE／完整 licenses 目錄不可為精簡而刪除。

publisher 自己的 [LICENSE](https://raw.githubusercontent.com/bblanchon/pdfium-binaries/master/LICENSE)、[PDFium 上游 LICENSE](https://raw.githubusercontent.com/chromium/pdfium/main/LICENSE)、內嵌第三方元件條款不是同一份。現行上游 LICENSE 除 BSD 文字亦含 Apache 文字；正式交付以**固定 archive 的全文**與 build inputs 為準，不用 Rust wrapper 的 MIT OR Apache-2.0 標籤代表全部 DLL。non-V8 build 設定同時關閉 XFA，取得包後仍要讀 `args.gn` 核實。[build 設定](https://raw.githubusercontent.com/bblanchon/pdfium-binaries/master/steps/05-configure.sh)。

## Windows 載入與 Rust GNU

本機已確認 Rust 1.99.1；`.cargo/config.toml:3` 使用 `rust-lld`、`link-self-contained=yes`、`ld.lld`。10 月 27 日既有 debug CLI 的 PE imports 只觀察到 Windows 名稱（包括 msvcrt、bcryptprimitives、ws2_32 等），未見 libgcc／libstdc++ DLL；這只涵蓋該檔的靜態 imports，不證明新的 eframe EXE 或 Python native modules 沒有 runtime 依賴。

PDFium 主檔使用 manifest 對應的絕對路徑。GUI 程序啟動早期設定 `SetDefaultDllDirectories(SYSTEM32 | USER_DIRS)`，只 `AddDllDirectory` 受信任的 PDFium bundle 路徑；不將 PDF 來源目錄／Python site-packages 全體放入 GUI 搜尋路徑，不靠 CWD／PATH／`Pdfium::default()` fallback。多個 user directory 的順序不保證，同名 DLL 衝突必須在 staging 排除。[Microsoft DLL 搜尋規則](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order)、[程序搜尋設定](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-setdefaultdlldirectories)。

絕對主 DLL 路徑不限制所有間接 DLL。本輪 LLVM 列出的 PDFium imports 為 KERNEL32／ADVAPI32／GDI32／USER32，未見額外 CRT import。這不替代 GUI、Python PYD 的整包正常／delay／動態依賴驗證。只有明列 Windows 元件可作外部 OS 相依；本機 System32 找得到不等於可任意複製或另一台已安裝。Python child 須另外建立自己的確定 loader 路徑，GUI 程序設定不會代替它。

首版 eframe glow 要求 OpenGL ≥2.0，不打包顯示驅動；若改 DX12，shader compiler 與 native manifest 必須另驗。未來 macOS 的 `.dylib`、架構／loader path／code signing／notarization 由 `cfg` 平台模組處理，未有實作驗收。

## 本機舊資產可重用範圍

10 月 27 日只讀盤點 `dist`／`python-worker`，深度 ≤7 的 PDFium DLL／tgz／zip 查找無結果，root `runtime` 不存在；這不是全磁碟查找。沒有可直接驗收的既有 PDFium asset。

舊 `dist/release/BUILD_INFO.json` 記錄 embedded Python **3.13.16**、非 editable、DirectML profile、未要求離線資產；source commit 是舊 Python 應用的 `e84a05bc141bd3789a08daa3ecffab2c227b2174`。舊報告記錄官方 Python ZIP hash／簽章已核對。10 月 27 日另直接比對以下五檔，都符合舊 `RELEASE_FILES.json`，沒有複製或修改：[舊候選報告](../../reports/round4/06_windows_release.md)。

| 舊 runtime 檔案 | bytes | 查得 SHA256 |
| --- | ---: | --- |
| `python.exe` | 105,696 | `31fe10b4ed82a2f960af0ef9931f46a4b84ff286f8b6ffd9dee098cf4f10f17f` |
| `python313.dll` | 6,142,176 | `3b29bf9ff39bed8f660f5b935f19657f7ce7f145009851741b11364f6ebcb862` |
| `python313.zip` | 3,837,053 | `33c48dd2c3031d27784950aaee96d3d65f916e3c43dd3637ace333a7b9e8021a` |
| `python313._pth` | 80 | `35ddf94682ff9aa713a8d63557242ad00f3f28fdd39337f02c3bda4c0f791577` |
| `LICENSE.txt` | 33,861 | `62bec384df47b0328307db41455ff6ea2559e5546b394ac69148561b21703120` |

runtime 另有 python3、OpenSSL、libffi、sqlite、vcruntime DLL。它們可作**新 staging 的來源候選**，使用前仍核對完整 manifest、新 Worker 的真實載入與授權；上述五檔不是全 runtime 再驗證。不能沿用會啟動 `pdf2zh_next` 的 `_pystand_static.int`／舊 PyStand launcher。

現有 `python313._pth` 只列 `python313.zip`、`.`，`import site` 被註解，不包含 sibling site-packages。新 staging 要建立自己的明確 `_pth`，列必要的 `../site-packages`／Worker 路徑，不依賴 PYTHONPATH／CWD；如 wheel 必須使用 `.pth`／site 初始化，列具體依賴與測試，不概括啟用環境 hooks。官方 embedded distribution 由應用供應相依；不要要求使用者首次啟動再 pip install。[Python embedded 說明](https://docs.python.org/3.13/using/windows.html#the-embeddable-package)。

舊候選有 **`msvcp140_1.dll` 從 System32 載入但未內附**（`reports/round4/06_windows_release.md:59`）。新包減少依賴後是否仍需它，要依實際 staged 85 套件重算；不可假定已解。若需要 VC v14，選定可合法再散布且版本相符的官方來源、app-local 或 installer 方案，記錄條款／版本／hash；不複製 System32。Microsoft 對 package 與單檔再散布都有授權條件，本次沒有核定新的來源或安裝 runtime。[Microsoft 再散布文件](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170)。

## 已知 BabelDOC 閉包與資產

沿用 [metadata 盤點](DEPENDENCIES.md)：Windows CPython 3.13 的 BabelDOC 0.6.3 必要閉包 **85 distributions，含 BabelDOC 本身**；PyMuPDF 1.26.7、bitstring 4.4.0 保留。這是 distribution 數，不是 native DLL 數。openai／peewee／tenacity／Pillow 仍為上游必要閉包，不能因 Rust 接管應用服務而假刪。ONNX 選現有 CPU profile；舊包有 DirectML 不代表 BabelDOC Windows PDF 路徑使用 GPU。

新 builder 需獨立 Worker lock／wheel hashes、非 editable 組包、dist-info metadata／license／資料；不能從 root 完整 uv 環境隨意刪包後稱已驗證最小 closure。

10/30 更正先前靜態資產計數：BabelDOC import 尾端會剔除四個未被家族引用的字型，實際官方 manifest 為 fonts 34、CMap 146、tokenizer 1、model 1，**182/182、352,451,566 bytes**。不是缺少四項必要資源。本輪真實 Rust CLI warmup／generate／fresh-home restore 均成功，ZIP 與還原的每檔 SHA3 全相符；詳見 [資產實證](ENGINE_ASSET_INVENTORY.json) 及 [修正原因](DEPENDENCIES.md)。

正式包仍須保留模型／字型／CMap 個別授權及來源。Pillow 12.3.0 的先前安全限制仍未由本設計修補；未重新 audit，也不以 Rust GUI 宣稱已解。

## Poppler 比較：已有工具不等於可直接出包

10 月 27 日本機 `pdftoppm -v` 為 **26.07.0**，位於 Codex 管理 runtime 的 `dependencies/native/poppler/Library/bin/`；相鄰 manifest 為 `poppler=26.07.0=h6618ce5_3`、win32、x64。當次官方 stable 為 **26.10.0，2026-10-04**，本機工具未升級。[Poppler 官方來源](https://poppler.freedesktop.org/)。

以既有 `llvm-objdump --private-headers`，從 pdftoppm 沿同一 bin 內的 `DLL Name` 遞迴盤點，結果 **33 個 PE、57,306,928 bytes**（約 54.7 MiB），含 11 個 CRT API shim；扣除 shim 為 **22 個 PE、57,030,592 bytes**：

```text
pdftoppm.exe, poppler.dll, lcms2.dll, freetype.dll, zlib.dll,
jpeg8.dll, libpng16.dll, openjp2.dll, tiff.dll, deflate.dll,
Lerc.dll, liblzma.dll, zstd.dll, libcurl.dll, libssh2.dll,
libcrypto-3-x64.dll, psl-5.dll, icuuc78.dll, icudt78.dll,
MSVCP140.dll, VCRUNTIME140.dll, VCRUNTIME140_1.dll
```

例如 `poppler → libcurl → psl → ICU` 帶來 33,126,192-byte 的 `icudt78.dll`。未在 bin 內解析到的 imports 為常見 Windows 名稱：ADVAPI32、bcrypt、CRYPT32、IPHLPAPI、KERNEL32、Secur32、SHELL32、USER32、WLDAP32、WS2_32。這是有界靜態盤點，不涵蓋全部動態 LoadLibrary、編碼資料或字型，不能作乾淨機通過結論。

該 runtime 根目錄深度 5 的 license/copying/notice 檔名查找無結果，不推論其他位置也沒有；只表示目前目錄不足以成為本產品再散布來源。Poppler 上游包含 GPLv2 COPYING，所附 binary 需對應來源／建置／相依授權與必要 poppler-data；subprocess 不免除其自身義務。[上游維護者 COPYING](https://raw.githubusercontent.com/tsdgeos/poppler_mirror/master/COPYING)。

若未來改選 Poppler，應從官方簽章 source release 或完整 provenance 的 Windows publisher 建立自己的固定分發，不直接拷貝 Codex-managed runtime。Rust 控制原生 Poppler 可做到無 Python 預覽，但本次維持它為開發驗證工具，不作 GUI 隱性 fallback。

## Manifest 與驗收

正式 native manifest 需有元件版本／CPU／OS、固定來源 URL、archive digest、實際交付檔 hash／bytes、API feature、build args、source revision／patches、licenses／notices、normal／delay imports、外部 OS 分類、attestation／簽章驗證與功能證據。Cargo checksum、Python wheel hash、BabelDOC asset SHA3 各自保留。

下例是**目前已知資料加未驗證值**；不是可出包 manifest。builder 必須拒絕必要欄位為 null／分類未解／未驗證狀態：

```json
{
  "schema_version": 1,
  "target": "x86_64-pc-windows-gnu",
  "component": "pdfium",
  "version": "158.0.8125.0",
  "publisher": "bblanchon/pdfium-binaries",
  "release_tag": "chromium/8125",
  "archive_url": "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8125/pdfium-win-x64.tgz",
  "publisher_archive_sha256": "9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b",
  "downloaded_archive_sha256": null,
  "crate": {"name": "pdfium-render", "version": "0.9.4", "api_feature": "pdfium_7881"},
  "files": [{"path": "native/pdfium/pdfium.dll", "sha256": null, "bytes": null}],
  "licenses": [],
  "normal_imports": [],
  "delay_imports": [],
  "verification": "publisher-digest-only"
}
```

hash 不是 ABI／功能／版本安全的替代品。下一步依序：固定下載與逐檔核驗、GNU loader／render smoke、獨立 Worker staging／實際 `_pth` 載入、完整離線資產、實際 EXE／DLL／PYD 閉包、CLI／GUI／翻譯／預覽／取消退出、逐檔 manifest 與 notices／對應來源，最後才是獨立乾淨 Windows。

乾淨機需無 Python／Rust／開發工具，另驗無預裝 VC runtime；涵蓋非來源 CWD、中文／空格路徑、唯讀安裝目錄、離線首次啟動及不同顯示驅動。當前沒有 VM／Sandbox 通過證據，亦沒有新正式包大小或完整再散布驗收。舊候選與已通過翻譯結果見 [RELEASE_REPORT](RELEASE_REPORT.md)、[TEST_REPORT](TEST_REPORT.md)，不挪作新 native GUI 驗收。
