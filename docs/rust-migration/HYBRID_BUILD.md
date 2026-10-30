# Windows x64 hybrid bundle builder

`script/build_hybrid_windows.py` 建立新的 Rust CLI／原生 GUI／BabelDOC Worker 候選目錄。它不呼叫 Cargo、不變更舊 `dist/release`、不刪除舊 Python 應用，也不從完整 `.venv` 複製 site-packages。建置機需要既有 Python 3.11+ 及 `uv.exe`；交付機使用包內 CPython。

已實際建立 `dist/hybrid-diagnostic-20261030`，完成 wheel 安裝、搬移、包內 import／離線資產還原／CLI／GUI PDF 測試。首版 manifest 列 13,027 檔、1,246,802,492 bytes（不含 manifest 自身）；它保留未解的 MSVCP140_1 與授權缺項，並非乾淨 Windows／VM 驗收通過。每個輸出的 `BUILD_INFO.json` 固定標為 `artifact_kind=hybrid-windows-x64-candidate`、`release_ready=false`。新版 Worker profile 改用 Pillow 13，另建新候選，不覆寫首版。

## 固定輸入

| 元件 | 選擇與驗證 |
| --- | --- |
| Rust | 呼叫端先提供 `pdf2zh.exe`、`pdf2zh-gui.exe`；預設從 `target/release` 讀取，檢查 PE32+ AMD64，交付檔逐一 SHA256。builder 不編譯、不下載工具鏈。 |
| CPython | 3.13.16、Windows x64 embedded。只接受舊 `dist/release/runtime` 中完整列於舊 `RELEASE_FILES.json` 的 34 個檔案，每檔 bytes／SHA256 均核對，拒絕額外檔案及連結。五個關鍵檔另有程式內固定 digest；來源的舊 `_pth` 也先驗證，才改寫新複本。 |
| Python engine | [85 個 distribution 基準盤點](PYTHON_DEPENDENCY_INVENTORY.json) 與 `uv.lock`，再套用 [Worker 專用安全更新](../../python-worker/wheel-overrides.json)：Pillow 12.3.0 → 13.0.0，其餘 84 項維持版本。獨立的 [requirements lock](../../python-worker/requirements-win64.lock.txt) 每項只選一個 Windows x64／CPython 3.13 wheel，固定 HTTPS URL、SHA256；不採 sdist。舊應用 `uv.lock`／`.venv` 保留供回歸。 |
| BabelDOC | 0.6.3。包內仍有 upstream 必需的 OpenAI SDK、peewee、tenacity、Pillow 等；ONNX 為 CPU profile，沒有沿用舊 DirectML profile。沒有 `pdf2zh_next`、Gradio、PyStand launcher 或 `.int`。 |
| Worker | 只複製 `python-worker/worker.py`，放到 `babeldoc/worker.py`；不複製開發測試或舊應用。 |
| PDFium | `bblanchon/pdfium-binaries` 的 `chromium/8125`、158.0.8125.0、x64、無 V8／XFA。archive publisher SHA256 為 `9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b`；DLL 為 7,586,816 bytes，SHA256 `4b0dec92555d69437bef0df37be630181ab3e83a6823ca264ede355a4f2ef994`。builder 查 DLL digest、VERSION、args.gn 及 notices，不自行下載或將 archive digest 當成簽章／ABI 證明。 |
| 離線資產 | [實際 runtime manifest](ENGINE_ASSET_INVENTORY.json)：字型 34、CMap 146、模型 1、tokenizer 1，共 182 項、352,451,566 bytes。ZIP 為 223,571,497 bytes，SHA256 `b32dfc00c5ade7b59094f82b774fef31b5b504e9c2aab783b842bf9a5792ba98`。 |

資產檔名中的 `b485a82328a20d1e4b5c37bb9c6472e7af9943333bdffd39a05b1f57d6d7a1da` 是 BabelDOC 資產列表的 tag，**不是 ZIP 的 SHA256**。舊 186 項的靜態數字包含 import 尾端被清理的四個未引用字型，不能作新的驗收標準。

## 指令

下例以目前工作目錄為專案根目錄。從其他 CWD 執行時，請使用 script 與 `--output` 的絕對路徑；預設來源均由 script 所在的專案根定位。

先作不含安裝或組包的檢查：

```powershell
& .\.venv\Scripts\python.exe -I -B .\script\build_hybrid_windows.py --self-test

& .\.venv\Scripts\python.exe -I -B .\script\build_hybrid_windows.py `
  --plan `
  --output .\dist\hybrid-candidate-20261030 `
  --bin-dir .\target\release `
  --cargo-registry-src C:\Users\user\.cargo\registry\src
```

主代理完成 Cargo release build 後，才執行真正建置。此指令會從固定 PyPI wheel URL 下載 wheel（精確 bytes 見 `--plan`），安裝到新的 staging，不安裝到 host Python／`.venv`：

```powershell
$hybridRevision = (& git rev-parse HEAD).Trim()
& .\.venv\Scripts\python.exe -I -B .\script\build_hybrid_windows.py `
  --output .\dist\hybrid-candidate-20261030 `
  --bin-dir .\target\release `
  --cargo-registry-src C:\Users\user\.cargo\registry\src `
  --source-commit $hybridRevision
```

`--output` 必須是專案內尚不存在的新目錄。與舊 `dist/release`、來源、`.git`、`.venv`、`target`、source tree 重疊時會在寫入／安裝前失敗。選用的 `--asset-notices`、`--cargo-registry-src` 也屬保護來源。輸出已存在時絕不覆寫；選另一個新目錄，不需刪除舊成品。

可選來源參數：

- `--runtime-source` 與 `--runtime-manifest`：預設為舊 embedded runtime 及舊完整 release file manifest，不接受整個舊 bundle 作為 runtime。
- `--pdfium-source`：預設 `.test-output/rust-migration/native/8125/extracted`；需要 `bin/pdfium.dll`、`VERSION`、`args.gn`、`LICENSE`、`licenses/`。
- `--assets-zip`：預設本輪產生的 `.test-output/rust-migration/phase6-assets-generate/archives/offline_assets_<tag>.zip`，內容與 SHA256 必須完全吻合。
- `--uv`：明確指定既有 `uv.exe`，否則在清理環境前只解析一次既有命令位置。
- `--asset-notices`：呼叫端提供經查核的字型／模型／CMap／tokenizer 授權與來源目錄。它不取代授權審查；builder 不到使用者資料夾猜測素材。
- `--cargo-registry-src`：明確指定既有 Cargo `registry/src`，唯讀收集對應 Cargo.lock 的 license 檔；未提供或 cache 不完整時，報告缺項，不自動 fetch。
- `--install-timeout`（預設 1800 秒）、`--smoke-timeout`（600 秒）：單一外部步驟的有界時間。逾時不發佈目錄，保留工作區與 log 供診斷。

只有經過 closure／版本／hash 來源審查，才重新產生 requirements：

```powershell
& .\.venv\Scripts\python.exe -I -B .\script\build_hybrid_windows.py --write-lock
```

這只更新上述 owned lock；不變更 `uv.lock`／Cargo.lock。基準版本、替換 wheel 的名稱／版本／平台／hash／bytes／官方 metadata URL 均檢查，profile digest 也寫入 lock。更新會拒絕連結，以同目錄暫存檔、flush／fsync、atomic replace 保留舊檔直到新內容寫完。平常組包若看到 lock 與 inventory／uv.lock／profile 不符，會停止，不自行更新；安裝後只讀已捕獲的 provenance 快照。

## Staging 與執行隔離

輸出同層建立唯一 `.w-<uuid>/stage`，成功時在同一個磁碟區把完整 stage rename 為新輸出。工作名稱刻意縮短：首輪帶完整 output 名稱時，28 個 ONNX 參考資料路徑達 260–268 字元而失敗；保留失敗 stage，不更改全域長路徑政策或刪除 package 資料。Windows destination 已存在時 rename 失敗。沒有覆寫／合併成品，也沒有遞迴刪除舊目錄。成功或失敗均保留 work 的 log／uv cache；失敗 stage 不作成功成品。

`uv` 的實際安裝參數包含：

```text
uv --no-config pip install --python <stage>/runtime/python/python.exe
  --target <stage>/runtime/python/site-packages
  --no-deps --no-index --require-hashes --only-binary :all: --link-mode copy
  -r <stage>/provenance/requirements-win64.lock.txt
```

所有 requirements 都是 exact direct URL，所以 `--no-index` 仍可下載已鎖 wheel，不會重新解依賴或讀私人 index。builder 的子程序環境只保留必要 Windows 系統路徑，HOME／USERPROFILE／TEMP／APPDATA／uv cache 指向新的 work；不繼承 API keys、proxy、PYTHONPATH、uv index／config 環境。PATH 僅 System32；不改系統 PATH，不讀或複製使用者應用設定、API credentials。`uv` 不讀專案／使用者設定，也不下載 Python。

新 `runtime/python/python313._pth` 精確內容為：

```text
python313.zip
.
site-packages
```

不啟用 `import site`；目前 engine wheel 閉包不需要 `.pth` 執行。安裝後發現任何 `.pth` hook 會停止，須先個別查核，不能概括啟用 site 或全域環境。實際 smoke 使用 staged embedded Python `-I -B`，確認 Python 3.13.16／64 bit、isolated／ignore_environment／no_user_site，所有 `sys.path` 位於該 runtime 內，並載入 BabelDOC high_level、PyMuPDF、ONNXRuntime、NumPy、SciPy、OpenCV、Rtree、Hyperscan、HarfBuzz、FreeType、cryptography。這是 import smoke，不是 PDF 翻譯或預覽驗收。

包內 Rust CLI 執行 `--restore-offline-assets ... --worker-home <stage>/resources/worker-home --json`。這條路徑使用真正 bridge／worker，不建立 provider 或讀 API key；builder 再驗證 cache 的每項 bytes／SHA3 與完整檔案集合。embedded probe 另重算 BabelDOC 的 active dependency／extras 閉包，逐項檢查 `Requires-Python`／`Requires-Dist` 與實裝版本，不能以 `--no-deps` 隱藏更新後的不相容。CLI／GUI 的 `--version` 也要成功。最後只移除新 stage 的 smoke bytecode，及經唯讀確認 schema 正確且零列的 BabelDOC 生成快取；非空或未知資料庫保留並停止組包。再次核對僅 182 項資產後產生 manifest。Rust Worker 以 `-I -B -u` 啟動。

## 輸出與授權證據

```text
pdf2zh.exe
pdf2zh-gui.exe
runtime/python/{python.exe, python313.dll, python313.zip, python313._pth, ...}
runtime/python/site-packages/                 # 精確 85 distributions + metadata/data
babeldoc/worker.py
native/pdfium/pdfium.dll
engine-assets/offline_assets_<tag>.zip
resources/worker-home/.cache/babeldoc/         # 182 項已還原、已驗 SHA3
licenses/{project,python-runtime,python,rust,pdfium,engine-assets}/
provenance/                                  # locks、inventory、builder、來源檔 hashes
THIRD_PARTY_COMPONENTS.json
NATIVE_DEPENDENCIES.json
BUILD_INFO.json
RELEASE_FILES.json
```

`engine-assets` notices 目錄只有明確提供時才存在。Python metadata、wheel 內 LICENSE／COPYING／NOTICE 等保留並收集；flatbuffers 與 PyMuPDF 的現有補充 license 另核對固定 SHA256。不複製與 engine 無關的 Gradio／Tencent／PyStand notices。Rust 清單涵蓋 Cargo.lock 全 target，可能大於 Windows 實際 link 的集合；優先按 Cargo.lock SHA256 核對既有 `.crate` archive，只讀 package metadata／notice 而不解出 source。舊 checksum cache 作已驗證 fallback。這不是 archive 簽章驗證或對應來源的完整交付。

`RELEASE_FILES.json` 記錄所有交付檔案的相對路徑、bytes、SHA256，僅排除 manifest 自身，避免自我 digest 循環。`BUILD_INFO` 不帶目前使用者名稱、API key 或環境內容；呼叫端的 `--source-commit` 只是其提供的來源 revision，不能從 EXE hash 推導其真實建置來源。可重現的是鎖定輸入與組包步驟；本工具不承諾 supplied Rust EXE 或 installer-generated metadata 的逐位元重建。

## Native／正式交付界限

在啟動 staged CLI 或 import smoke 前，builder 先讀取所有包內 EXE／DLL／PYD 的 AMD64 PE normal／delay imports，產生 `NATIVE_DEPENDENCIES.json`。名稱分為明列 Windows OS 元件、包內可選 DLL、未解析三類。API-set 與 Windows 元件不從開發機複製；**MSVCP140／MSVCP140_1／VCRUNTIME 等 VC redistributables 不列為 OS 元件**。一個同名檔案在包內只是 static candidate，不保證 native loader 會從正確的 module scope 找到它；動態 `LoadLibrary` 與顯示驅動亦未涵蓋。

舊候選曾依賴未內附的 `msvcp140_1.dll`，本 builder 不假定新閉包已解決。如果新 stage 仍有未解析 import，預設停止並保留報告。不要從 System32 複製 DLL；需選定符合授權、版本、架構的官方再散布來源，記錄 hash／條款後另接入。目前沒有自動取得新的 VC runtime 來源或 installer。

若只是要在已裝 runtime 的開發機診斷，可明確加 `--allow-unresolved-native`，在另一個新輸出名重跑。仍需通過 import／restore／version；缺項保留在 manifest、`checks.native_unresolved_count` 與 release review items，不可當作乾淨機可攜通過。此旗標不關閉 hash、路徑、閉包或 `.pth` 檢查。

正式交付仍需：個別 font／model／CMap／tokenizer notices 與來源、AGPL／其他相依的對應來源與義務核對、native DLL 實際載入證據、乾淨 Windows（無 Python／Rust／預裝 VC runtime）CLI／GUI／PDF／預覽／取消退出、唯讀安裝／離線首次啟動／不同顯示驅動。本機中文空格路徑的整包 CLI／GUI 翻譯與取消已另測；不能移作新 bundle 的 VM 驗收。Pillow 13 的固定 wheel、隔離 imports／依賴條件與完整 PDF 零差異證據見 [SECURITY_REPORT](SECURITY_REPORT.md)。

## 本輪實際檢查

- `--write-lock`：由目前 inventory／uv.lock 產生 85 個確切 wheel URL／SHA256，未安裝。
- `--self-test`：合成邊界測試涵蓋 wheel 平台／ABI 選擇、credential URL 拒絕、unsafe paths、輸出與所有來源重疊、copy hash mismatch 不改來源、資產 SHA3 tamper／missing、bytecode 清理限新 stage、manifest 完整性、子環境移除 secrets、normal／delay PE imports 與 missing CRT。lock atomic replace 失敗保留舊檔及 mocked reparse stat 也有測；這不是 Windows 原生 symlink 權限驗收。
- 唯讀 `--plan --bin-dir target/debug --output dist/hybrid-plan-only` 已通過：34 runtime files、85 wheels（214,954,906 bytes）、固定 PDFium DLL、ZIP SHA256／182 entries 均核對。此命令沒有建立 `dist/hybrid-plan-only`、安裝套件或啟動 GUI／Worker。release EXE 的 plan 須在主代理供應正式 build 後再跑。

首版的完整安裝／搬移／import／還原與 bundle CLI／GUI 證據已完成；其 EXE 來源為 `6187d9b`、Pillow 仍為 12.3.0。Pillow 13 profile、新版 HTTP transport 與 `-B` Worker 須透過新輸出另建，不改寫首版證據。合成測試現為 24 項通過、Ruff 通過；後續實際新包結果另記錄。

新版已建立於 `dist/hybrid-diagnostic-20261030-v2`，EXE 來源 `f17abb8`，Pillow 13.0.0；搬移後 active metadata closure／native imports／restore／versions 通過。manifest 為 13,042 檔、1,248,017,820 bytes（排除自身），SHA256 `3086c43405de7442f47c1cdad1b50515d5929f8f9adbc3e4758bf7d98b276094`。另對發布目錄重讀集合與逐檔 hash 全過、沒有 DB／loose bytecode，再以中文空格路徑測試複本執行 CLI／GUI。兩條入口的八頁 PDF 與 main 文字、座標、圖片、像素差均為零；Terra xhigh 實測 GUI 預覽與處理中關閉，fixture exit 0、特定 Worker 無殘留。見 [RELEASE_REPORT](RELEASE_REPORT.md)。這仍是帶兩個未解析 CRT import 及授權缺項的本機診斷包。
