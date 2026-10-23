# GUI 介面資源與資料分離

將 18,261 bytes CSS、9,567 bytes 預覽 JavaScript 抽至 `ui/resources/gui.css` 與 `preview.js`，搬移時逐字相等。`gui_translation.yaml` 實際搬到 `ui/resources/translation.yaml`，SHA-256 維持 `fa8358264c12255f949067d2b01c36007eeec8b604e00f705c6c850210f0808f`；語言與頁碼對照移至 `ui/languages.py`。GUI 原名稱保留，資源從套件檔案位置解析，與工作目錄無關。

兩項資源／事件測試在搬移前後通過；連同檔案、設定與既有 GUI 測試共 **51 passed、67 warnings**。實際 `uv build --wheel` 成功，zip 內容包含三份資源且 bytes 與來源完全相同。尚未在此 Commit 修改 JavaScript 的原有語法錯誤，以便單獨比較修正行為。
