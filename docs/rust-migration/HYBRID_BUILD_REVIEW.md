# Windows builder 審查與整合紀錄

日期：2026-10-30。Astra 唯讀 source 審查提出 HYB01–04；其 turn 因用量上限中斷，沒有最終全量簽核。整合者依具體發現完成修正與測試，以下不把整合者的後續工作歸為 Astra 已複審。

| ID | 具體問題 | 修正／oracle |
| --- | --- | --- |
| HYB01 | optional asset notices 未進 output overlap guard，工作目錄可能落入輸入而被再複製 | 所有 optional notice／registry／uv 輸入均納入，建立目錄前拒絕重疊；合成測試驗沒有產生輸出 |
| HYB02 | `--write-lock` 直接截斷寫入可跟隨 link | 檢查所有祖先 reparse points、同目錄暫存檔 flush/fsync/replace；失敗保留舊檔與 mocked reparse sentinel 測試通過 |
| HYB03 | manifest 依 basename 排除所有 `RELEASE_FILES.json`，漏掉巢狀同名檔 | 只排除根自身 manifest；巢狀同名檔測試納入 hash |
| HYB04 | 安裝後讀 live Cargo.lock／Python inventory，可能與已交付快照不同 | 全程使用 stage provenance，捕獲的 requirements／inventory／uv.lock 須一致；live source 不存在時仍以 snapshot 成功，snapshot lock 改動則拒絕 |

本輪 builder **24 個測試通過**，另 Ruff 通過。新增 oracle 包括 CRLF 授權檔僅在正規 LF digest 符合已記錄來源時接受、`.crate` archive 先按 Cargo.lock SHA256 核對後只讀授權文字且不 extract source、錯 digest 拒絕；空的已知引擎 cache 才能移除，非空 cache 保留；Worker profile 拒絕未知套件、過時基準、錯平台／名稱／版本、credential URL、缺 hash／reason／size，捕獲的 profile 變動也停止組包。這些實測不等於 archive／runtime 數位簽章或完整法律審核。

整合發現並保留的失敗：長 work 名稱讓 28 個 ONNX 參考資料路徑達 260–268 字元；縮短 fresh work 名稱後繼續。Git CRLF 授權文字與舊 LF hash 不同，只接受經相同 LF digest 證明的換行轉換。現有 Cargo cache 缺 `.cargo-checksum.json`，改從 hash 已驗的 `.crate` 原文取 notice；未驗證快取不作來源。上述均未刪除或改動舊 `dist/release`。

完整 PE 盤點已到達 380 個檔案。Microsoft PDH 為 OS 元件；ONNXRuntime 尚缺 MSVCP140_1，預設 build 正確拒絕發佈。另有明確 diagnostic 模式供本機查核，保留缺項及 `release_ready=false`。正式包還需 runtime來源／載入路徑、asset notices、缺少的 crate license texts、對應來源、乾淨機與實際 PDF／GUI／取消驗收；不能用合成測試清空這些門檻。

後續修正：inventory／uv.lock 在本機為 CRLF、Git blob 為 LF，raw digest 會讓跨 checkout 的同內容 lock 假 stale。owned requirements 現使用明列的 LF 文字 fingerprint，release manifest 與 BUILD_INFO 仍 hash 真正原始 bytes；新增 captured-input LF／CRLF 成功、內容變更拒絕 oracle，**25 tests**／Ruff 通過。v3 已另建立，原始 13,042 records 全驗證；在非來源 CWD 的中文空格路徑測試複本完成 107-request PDF 且八頁零差異。Astra 尚未對此後續變更作最終複審。
