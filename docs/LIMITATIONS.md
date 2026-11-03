# 使用與驗證限制

兩平台實際結果以 [STATUS](STATUS.md) 為準。版本 1.0.0 不表示已通過正式兩平台發行 gate；`BUILD_INFO.release_ready` 保持 false。

## 平台與交付

- Windows 已在此機測試完整 runtime；ONNX Runtime 的兩個 import 仍需官方 Microsoft VC++ v14 x64 Runtime（`msvcp140_1.dll`）。請按 [Microsoft 說明](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170) 安裝，不從 System32 複製 DLL。未驗乾淨 VM、不同實體磁碟與真實唯讀 ACL。
- macOS 只支援 Apple Silicon arm64、macOS 14+；使用者已確認 Actions 完整包可翻譯及預覽。本機沒有 macOS，新開發入口的原生執行／Finder 驗收仍待完成。CI 遇到缺失原生依賴會失敗；沒有 Intel 包。
- Mac 包未簽章／公證，Gatekeeper 可能阻擋；不移除 quarantine 或停用安全機制。CPU／CoreML 可用性需在 Mac 原生驗證，沒有 GPU 加速保證。
- Windows 的 Job Object 程序樹清理有受控測試；非 Windows 目前只 wait direct child，尚無同等 process-group／後代終止保證。Mac 使用 CLITranslator／Claude Code 等外部程序前須特別驗證取消與關閉。
- 125%／150%／200% 真實 OS DPI、完整鍵盤／螢幕閱讀器無障礙、所有顯示卡／驅動及超大型 PDF 長期資源測試未完成。egui 縮放與窄視窗布局的測試不等於上述認證。

## 翻譯服務與相容性

16 個原 provider 都有 Rust 路徑；OpenAICompatible、AliyunDashScope、DeepSeek、ModelScope、Zhipu、Gemini、Grok、Groq 使用 OpenAI 相容 route。這次使用本機固定回應／受控程序，沒有呼叫付費 API；不據此宣稱每家遠端 SDK 或翻譯品質均等價。

| 服務 | 現有本機證據與界線 |
| --- | --- |
| OpenAI／AzureOpenAI／SiliconFlow／Qwen-MT | 請求、選項、usage、分層重試與 typed error；Qwen-MT 僅 translate。未驗真 Azure deployment |
| SiliconFlowFree | 內層 429、外層 HTTPError 獨立預算與混合重置 |
| Ollama／Xinference | nonstream、動態預測、錯誤／模型 fallback；Xinference 不改成 100 輪 fallback |
| Google／Bing | loopback、sentinel／session refresh、cookie／predicate retry 與取消 |
| DeepL／Azure Text Translation | 分層 retry、deadline、語言與 wire fixtures；未完整重現 SDK pipeline |
| AnythingLLM／Dify | payload、解析、redirect cookie；非字串輸出明確 typed error |
| Tencent | 合成 TC3 簽章、loopback、設定及完整環境 ID／Key 覆蓋 |
| CLITranslator／Claude Code | 受控 child 的 stdin、輸出、postprocess、timeout、取消；未執行真 Claude |

HTTPX write／pool timer、企業 proxy／自訂 CA／環境處理、所有 requests redirect／cookie 角落及每個 Python wire／clock oracle 未全面重現。DeepL 初始 prepared request 不帶 session jar，redirect 才可用累積 cookies；Azure 七天 operation budget 限制 connect，read timeout 另為 300 秒。這些既有契約不由 UI 更名改寫。

Tencent 仍要求已設定靜態 ID／Key；完整 `TENCENTCLOUD_SECRET_ID`／`TENCENTCLOUD_SECRET_KEY` 環境對優先，部分環境對回到設定。環境憑證不序列化；不消費 session token。不支援 profile、CVM metadata、TKE OIDC／STS 與刷新；完整 TKE 指示會明確報錯，不假裝取得角色。

開始翻譯含健康檢查，可能計入服務用量。取消不能撤回遠端已受理請求；重試／回應遺失也不能保證絕不重複計費。每次 HTTP retry 並非重新取得 QPS token，QPS 與 BabelDOC pool／concurrency 各有界限。測試矩陣與 frozen oracle 只證明其明列範圍。

## 檔案、設定與 PDF

使用完整程式資料夾；缺 runtime／native／assets 時先核對 `RELEASE_FILES.json`，按 lock 重建，避免任意 pip install。完整包不依賴開發 `.venv`。預設設定、fallback、cache 與 IPC 詳見 [ARCHITECTURE](ARCHITECTURE.md)。

Cargo 只編譯 Rust；從原始碼執行請用根目錄的 `啟動開發版.cmd`／`啟動開發版.command` 準備及檢查完整 Runtime，或明確指定可信任的 `--runtime-dir`，詳見 [開發流程](BUILD.md#開發驗證)。這不會讓單獨的 `target/release` 資料夾變成可攜發行包。

Windows 超長 EXE 路徑受 COM／OLE 與原生套件限制；GUI 使用既有磁碟短檔名重啟，無短檔名時提示縮短路徑。Worker 的 Python 路徑達 260 字元時，只將 OpenCV 設為單執行緒；不修改系統路徑政策、ONNX 或 API 併行設定。

本次 ZIP 搬移測試中，Python 一般解壓因深層 ONNX 測試資料遇到 WinError 206；使用既有 7-Zip 完成解壓，再以長路徑 API 驗證全部 13,127 項雜湊。請使用支援長路徑的解壓工具及較短目的地，不修改系統政策或刪去未確認的引擎檔案。

已改動的 BabelDOC 模型／字型會保留並停止初始化；先備份，或使用新的空白 `--worker-home`。不要刪除不明 cache／模型。API Key 保存在本機設定，cache 含原文／譯文，均應當作私人資料。

預覽只載入包內 PDFium；缺檔、獨占鎖或 native 呼叫長時間阻塞可能影響操作。使用重新載入；原始 PDF 與舊成果不會因另存或重試被刻意刪除。固定 identity 回歸能檢查排版與串接，不能衡量中文翻譯正確率。

GUI 支援公開 HTTP(S) PDF 下載：標準連接埠、每次 DNS／redirect 公網檢查、100 MiB、總計 60 秒、連線 10 秒、最多五次轉向。拒絕本機／內網 URL；它與同權限 Worker 都不是惡意 PDF sandbox。只有自己有權且願意處理的文件才應送入引擎／翻譯服務。

官方術語 CSV 內容預覽及術語 ZIP 尚未移植；保留 CSV 路徑選取、單份術語檔另存與 PDF 批次 ZIP。第三方錯誤及未知進度可能保留原文；服務／語言／模型的全部組合未逐一驗證。

## 授權與發布

完整 AGPL、來源、修改告知與依賴 notices 位於 [LICENSE](../LICENSE)、[THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md)、GUI「關於 BabelOxide」及執行包 `licenses/`／`provenance/`。這些不是正式法務認證；公開散布或網路提供修改版功能前仍須符合對應原始碼及各元件授權要求。沒有自動 Push、簽章、公證或 Release。

`dispatch 0.2.0` 已按發布 crate 的 MIT 宣告補入 OSI 標準條款、原始 manifest 及可追溯告知，沒有虛構年份／著作權或冒充上游 LICENSE。Windows 套件及 Cargo.lock 全目標清單的缺文項目已歸零；objc2 的 Apple SDK 說明及 Mac standalone CPython／原生 wheel 內含程式庫的實際散布條件，仍需在 Mac 成品審查。原有已驗證 Windows 包保持不變；下一份完整包須由更新後的 builder 產生，才能帶入本輪新增的集中 notices。不得把文件已收集解讀為所有平台的正式再散布認證。

macOS 原始碼 `.command` 的 Windows Shell 驗證不代表 Finder 雙擊或原生工具鏈已通過；實際平台驗證結果見 [STATUS](STATUS.md)。來源審查 ZIP 不含第三方大型資源或私人資料，公開散布前仍須提供實際發行提交的相應來源及必要依賴來源。
