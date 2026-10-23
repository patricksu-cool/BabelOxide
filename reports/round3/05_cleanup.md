# 全專案清理

盤點範圍包含根目錄、Python 套件、test／tests、GUI 資源、GitHub Actions、Docker／PyStand 建置、docs 所有語系、PO 檔、reports 與已追蹤的 .doctemp。刪除前逐檔清單：05_deletion_manifest.json。

實際刪除 55 檔、368,287 bytes：原始英文文件的 chunks 與 splitResult 中繼、10 份已被 advanced/API/python.md 取代且含未實作 HTTP API 的 APIS 草稿、三個舊版部署腳本、重複 Flake8 設定、已被原生預覽替代的 preview.js。逐一確認路徑位於工作區、是一般檔案且非 symlink 後才刪除；僅刪除確認為空的 .doctemp/chunks、.doctemp/splitResult 目錄。

保留 LICENSE、所有測試（含停用的歷史 provider 測試）、現用 EXE 工作流程、PyStand launcher、GUI 圖片／YAML／CSS、文件 PO、.doctemp/translated／terms／gui、所有歷史報告。沒有刪除 config、API Key、pdf2zh_files、Downloads、使用者 PDF 輸出、venv 或未能確認所有權的 --google 檔案。新增精確 ignore，避免重新追蹤可再生英文中繼。

Docker 修正：移除安裝後的 BabelDOC 更新／PyMuPDF 降級與 random.org cache-buster，改用 uv.lock；.dockerignore 排除設定、憑證、測試環境、報告與快取。容器預設同樣 loopback，公開容器 port 需使用者明確指定位址。未在本機安裝 Docker 或聲稱已建置映像。

移除未配置的 mkdocs-git-committers-plugin 與不再使用的 markdown-include，保留實際文件 plugins。pre-commit 對齊目前 Ruff，移除與 pyproject 不一致的額外 ignore。新增集中開發／測試／建置／限制文件與報告索引。

體積：基準 Git 追蹤內容 503 檔、17,757,228 bytes；清理後與最終體積另依相同排除規則計算，報告／瀏覽器證據增加也一併列入，不能只挑已刪除 bytes 宣稱總體積下降。

清理後初次驗證：224 passed（14.15 秒）、Ruff format 全部 313 檔通過、wheel 建立成功；發行環境含 134 個套件。Strict 暫時因新增 development.md 尚未提交而觸發 git-authors／git-revision metadata 警告，需提交新文件後重跑；沒有停用任何 Git／Strict plugin。

AST 防護腳本第一次引用上一輪報告中的過時 asynchronize.py 路徑而失敗；已改為遞迴比對實際 translator 與 utils/asynchronize 套件，23 檔均與 1345fc2 AST 一致，結果完整保存在 04_retry_ast.json。
