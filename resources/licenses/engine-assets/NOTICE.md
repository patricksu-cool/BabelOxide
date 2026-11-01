# BabelDOC 引擎資產 notices

資產內容固定於 `resources/engine-assets.lock.json`：34 字型、146 CMap、1 ONNX 版面模型、1 tokenizer cache。這些是引擎必要資源，不是可任意刪除的測試 cache。

- 字型取自 [BabelDOC-Assets](https://github.com/funstory-ai/BabelDOC-Assets)；`font-notices.json` 從逐檔 SHA3 已驗證的字型 name table 保留 copyright、trademark、manufacturer、designer、license 與 license URL 原文，包含 Noto／Source Han／Go Noto／Klee／WenKai／MaruBuri 的個別聲明。
- CMap 由 BabelDOC-Assets 提供，保留 [Adobe CMap resources](https://github.com/adobe-type-tools/cmap-resources) 的 upstream license 原文。未以本文件重新授權個別資產。
- `doclayout_yolo_docstructbench_imgsz1024.onnx` 取自 [DocLayout-YOLO ONNX 模型庫](https://huggingface.co/wybxc/DocLayout-YOLO-DocStructBench-onnx)，轉換版與 [基礎模型](https://huggingface.co/juliozhao/DocLayout-YOLO-DocStructBench) 的模型卡均宣告 Apache-2.0；附兩份模型卡與 Apache 授權原文。這與 [DocLayout-YOLO 程式碼](https://github.com/opendatalab/DocLayout-YOLO/blob/main/LICENSE) 的 AGPL 是不同來源告知，不據模型卡重授權程式碼。
- tokenizer cache 隨 BabelDOC／tiktoken 使用；tiktoken 的 MIT license 亦隨 Python distribution notices 保存。

這是個人使用版的來源與必要 notices 保存，不宣稱完成所有商業再散布或對應來源義務審查。
