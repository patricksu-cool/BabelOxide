# 獨立格式整理

使用現有 Ruff 設定，執行 ruff check --select I --fix .、ruff format .、ruff check .。lint 通過，未放寬規則。測試中 0.0.0.0 是 mocked launcher 的輸入，集中為常數並以單點 noqa:S104 註明，不產生對外綁定。完整 format check 與受保護 AST 在最終回歸確認。
