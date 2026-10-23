# 第八階段：Windows 鎖定建置

## 修改

`.github/workflows/exe-build.yml` 改用 `uv sync --locked --no-dev --no-editable`，明確指定專用環境；移除不必要的整份工作目錄複製及 activate 步驟。uv 快取同時依據 `pyproject.toml`／`uv.lock`，依賴檢查與離線資源產生指令失敗會回傳非零。離線資源仍使用同一個已鎖定環境執行，原 PyStand／embedded runtime 入口保留。

## 本機驗證

- 全新 `.test-output/round2-stage9-exe-env` 用相同 locked／no-dev／no-editable 選項建立，**136 個實際安裝套件**；`uv pip check` 通過。
- 本機建置 Python 為已安裝的 **3.13.16**，CI 固定版本仍為 **3.13.3**，沒有為此安裝另一個 Python。使用既有的 embedded **3.13.3**／PyStand 建立全新 `.test-output/round2/exe-stage9`。
- 用 `python -I` 從安裝位置載入拆分模組，確認不是從工作目錄或 editable path 載入；翻譯 YAML 路徑有效。首次未隔離 profile 的受限制執行因預設 SQLite cache 無法寫入失敗，之後設定隔離 USERPROFILE／TEMP／app 目錄重跑通過；沒有修改 cache 程式來掩蓋環境問題。
- 新 wheel 內 CSS、JavaScript、翻譯 YAML、configuration／usage 共五個檔案存在且與來源逐位元組相同。
- Workflow YAML 可解析，展開版本字串後 PowerShell 語法解析沒有錯誤。
- 新 EXE 從其他工作目錄執行 `--version`：**exit 0、2.9.0**。設定建立在 EXE 旁 `config/default/2.9.0.toml`；工作目錄沒有 config，隔離 profile 沒有發生舊設定遷移。

完整 PDF／GUI／錯誤流程留到第九階段，不能以版本指令代替。完整 GitHub Actions、重新下載 runtime、離線資源包產生／移除後重抓、跨電腦測試尚未執行。
