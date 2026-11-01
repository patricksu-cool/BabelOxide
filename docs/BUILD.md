# 建置與平台交付

版本由根目錄 Cargo workspace 讀取。Rust／開發 Python 使用 lockfile；Worker 分別使用 Windows 85 個、macOS 84 個固定 wheel（差異只有 Windows 專用 colorama），BabelDOC **0.6.4**、Pillow **13.0.0**。Mac 的 ONNX Runtime 需要 **macOS 14+、Apple Silicon arm64**。

## GitHub Actions

[build.yml](../.github/workflows/build.yml) 接受手動、來源 push 與 PR 觸發。`windows-latest` 建 Windows x64，`macos-latest` 先確認為原生 arm64 才建 Mac。僅需 `contents: read`；不使用 Secrets、不 Push、不簽章、不建立 Release。官方 checkout v6、setup-python v6、cache v5、upload-artifact v6 的 `action.yml` 已核對 `using: node24`。

兩平台執行 fmt、嚴格 Clippy、Rust／Worker／Builder 測試、Release build、原生 import 與 PDFium、CLI／IPC 本機 Mock、套件閉包與 manifest 檢查。GUI `--version` 僅是版本 smoke，Actions 沒有宣稱通過桌面互動。

輸出完整 ZIP：`BabelOxide-v<workspace-version>-windows-x64`／`BabelOxide-v<workspace-version>-macos-arm64`，Artifacts 保留 **7 天**。只有所有步驟成功才上傳；缺失 wheel、錯誤架構或不可攜的 dylib/rpath 都會使 job 失敗，不使用 continue-on-error。

## 本機

保留原有 Rust／uv，Python 依賴只放專案 `.venv`。一般檢查：

```text
uv sync --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
uv run python -m pytest python-worker/tests -q
uv run python script/build_hybrid_windows.py --self-test
uv run python -m unittest discover -s script -p test_build_portable.py
cargo build --release --workspace --locked
```

`script/ci.py --platform windows-x64 --check` 與 `--build` 是 Actions 的相同入口；Mac 改用 `macos-arm64`。本機已有名稱不同但版本相符的 Rust toolchain 時，可指定 `--rust-toolchain <existing-name>`；不會替換或安裝本機工具鏈。CI 的 Rust 固定版本、Python runtime、PDFium 與 bootstrap uv 的公開來源／SHA256／大小在 [build-inputs.lock.json](../resources/build-inputs.lock.json)。

完整包 builder（先編譯 Rust，輸出目錄須不存在）：

```text
uv run python script/build_portable.py --platform windows-x64 --output dist/BabelOxide-windows-test --allow-unresolved-native --zip
uv run python script/build_portable.py --platform macos-arm64 --output dist/BabelOxide-macos-test --zip
```

Windows 的旗標明示需要官方 VC++ x64 Runtime；CI 只容許已知 VC++ Runtime DLL，其他未解析 import 一律失敗。不從 System32 複製檔案。

## 打包邊界

- Python 來自固定官方 embedded archive（Windows）或 Astral standalone archive（Mac）。Mac 移除 pristine archive 中的 pip／安裝 wrapper 後才安裝固定 engine closure，保留 stdlib 與其必要資源；沒有 venv 複製或全域 Python 依賴。
- BabelDOC 透過隔離 Worker 下載公開資產，逐項按 182 個 size／SHA3 pin 核對。CI 可重建 ZIP；ZIP 時戳不同不影響資產身分，產物另記實際 archive SHA256。
- Windows 保留 PE imports、原生 probes、完整 licenses／provenance／RELEASE_FILES。Mac `.app` 保留內部相對 runtime symlink，查核 Mach-O arm64、dylib 與 rpath，移動完整包後再執行 native／CLI smoke；不修改全域 Library 搜尋路徑。
- 只散布固定執行期、模型／字型種子、空 Key 範本與程式；不帶入個人 config、cache、測試 PDF 或翻譯結果。Manifest 同時檢查檔案集合、雜湊與 symlink 目標。
- Mac 未簽章／未公證，Gatekeeper 可能阻擋；不移除 quarantine 或停用系統保護。沒有 Intel、CUDA、DirectML 或必然 CoreML 加速的承諾。

兩平台實際驗證狀態與尚存限制以 [STATUS](STATUS.md) 為準；本機 Windows 成功不代表 Mac 已原生通過。
