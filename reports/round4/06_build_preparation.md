# Windows 發行流程修復與候選建置準備

實際稽核發現第三輪只讓 `exe-build.yml` 呼叫共用建置腳本；主要 `release.yml`、`fork-build.yml`、`rebuild.yml` 仍使用未鎖定的 `uv pip install`，再裝／更新 BabelDOC[directml]。本輪將這三個入口接到同一個 builder，保留原 DirectML 選項與原離線資產階段；新增四入口契約測試。rebuild 的可選版本參數改為對 lock 的期望值檢查，版本不符會失敗，不再偷偷換掉安全 pin。沒有執行 GitHub 遠端工作流程或發佈。

DirectML 透過相容 optional extra 納入 uv.lock；原有版本沒有被升級，僅增加 onnxruntime-directml 的 Python 分支。BabelDOC 同時要求 CPU／DirectML distribution，兩個 wheel 共用 onnxruntime/；乾淨 `uv sync` 曾實測載入 CPU 1.31.0 而沒有 DML provider。因此 builder 從鎖定安裝讀取精確 DirectML 版本，再以 uv export 的完整 constraints 正常重裝該 wheel，保持舊發行流程「DirectML 最後安裝」的順序，並強制檢查 DmlExecutionProvider 與 uv pip check。沒有修改第三方原始碼、沒有忽略相依限制。實際建置與 PDF 結果另在最終報告確認。

依 [Python 官方 3.13.16 發行頁](https://www.python.org/downloads/release/python-31316/) 改用同一 3.13 系列維護版，沒有更新系統 Python。下載的 embedded runtime python.exe／python313.dll Authenticode 均 Valid，簽署者為 Python Software Foundation。ZIP SHA256：`97dae5274cc54867065e8d5a3226e48c35017ed332a0fdb0e27d5b5821961297`。保留 PyStand 1.1.4 並補入其原始 MIT 授權，Python LICENSE 保留於 runtime。

新增 installed-package staging：排除套件 tests、bytecode、環境 hook、direct_url，保留必要 Python 執行檔與 DLL、套件程式及資源、dist-info 授權。這些 Python 檔是執行期必要內容，不是把 repository 原始碼與測試資料夾混入交付。新增無金鑰範本、繁體中文說明、第三方通知與逐檔 SHA256 manifest；若發現根層 config／輸出／憑證檔則拒絕建立 manifest。舊包不覆寫。

Ruff 及 PowerShell AST 語法檢查通過，10 份 workflow YAML 可解析。初次完整測試 245 passed（後續補入入口／範本與設定拒絕契約），定向檢查逐項通過；最終完整數量在新包驗證後重跑。

本機將建立 `dist/release/`，使用獨立 no-dev、no-editable 環境。此候選採明確首次下載模式，不複製開發者快取；大型模型／字型由 BabelDOC 官方機制首次下載與 hash 驗證。離線完整資產 ZIP 仍由 CI 原階段支援，本機不預先宣稱乾淨離線機通過。
