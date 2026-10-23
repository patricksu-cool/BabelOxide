<div align="center">

<img src="./docs/images/banner.png" width="320px"  alt="banner"/>

<h2 id="標題">PDFMathTranslate</h2>

<p>
  <!-- PyPI -->
  <a href="https://pypi.org/project/pdf2zh-next/">
    <img src="https://img.shields.io/pypi/v/pdf2zh-next"></a>
  <a href="https://pepy.tech/projects/pdf2zh-next">
    <img src="https://static.pepy.tech/badge/pdf2zh-next"></a>
  <a href="https://hub.docker.com/repository/docker/awwaawwa/pdfmathtranslate-next/tags">
    <img src="https://img.shields.io/docker/pulls/awwaawwa/pdfmathtranslate-next"></a>
  <!-- <a href="https://gitcode.com/PDFMathTranslate-next/PDFMathTranslate-next/overview">
    <img src="https://gitcode.com/PDFMathTranslate-next/PDFMathTranslate-next/star/badge.svg"></a> -->
  <!-- <a href="https://huggingface.co/spaces/reycn/PDFMathTranslate-Docker">
    <img src="https://img.shields.io/badge/%F0%9F%A4%97-Online%20Demo-FF9E0D"></a> -->
  <!-- <a href="https://www.modelscope.cn/studios/AI-ModelScope/PDFMathTranslate"> -->
    <!-- <img src="https://img.shields.io/badge/ModelScope-Demo-blue"></a> -->
  <!-- <a href="https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/pulls">
    <img src="https://img.shields.io/badge/contributions-welcome-green"></a> -->
  <a href="https://t.me/+Z9_SgnxmsmA5NzBl">
    <img src="https://img.shields.io/badge/Telegram-2CA5E0?style=flat-squeare&logo=telegram&logoColor=white"></a>
  <!-- License -->
  <a href="./LICENSE">
    <img src="https://img.shields.io/github/license/PDFMathTranslate-next/PDFMathTranslate-next"></a>
  <a href="https://hosted.weblate.org/engage/pdfmathtranslate-next/">
    <img src="https://hosted.weblate.org/widget/pdfmathtranslate-next/svg-badge.svg" alt="translation status" /></a>
    <a href="https://deepwiki.com/PDFMathTranslate-next/PDFMathTranslate-next"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"></a>
</p>

<a href="https://trendshift.io/repositories/12424" target="_blank"><img src="https://trendshift.io/api/badge/repositories/12424" alt="Byaidu%2FPDFMathTranslate | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>

</div>

PDF 科學論文翻譯與雙語對比。基於 [BabelDOC](https://github.com/funstory-ai/BabelDOC)。此外，此項目也是調用 BabelDOC 執行 PDF 翻譯的官方參考實現。

- 📊 保留公式、圖表、目錄和註釋 _([預覽](#預覽))_。
- 🌐 支持 [多種語言](https://pdf2zh-next.com/supported_languages.html)，以及多樣化的 [翻譯服務](https://pdf2zh-next.com/advanced/Documentation-of-Translation-Services.html)。
- 🤖 提供 [命令行工具](https://pdf2zh-next.com/getting-started/USAGE_commandline.html)、[互動式用戶界面](https://pdf2zh-next.com/getting-started/USAGE_webui.html) 和 [Docker](https://pdf2zh-next.com/getting-started/INSTALLATION_docker.html)

<!-- Feel free to provide feedback in [GitHub Issues](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/issues) or [Telegram Group](https://t.me/+Z9_SgnxmsmA5NzBl). -->

> [!WARNING]
>
> 本項目基於 [AGPL v3](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/blob/main/LICENSE) 許可證「按現狀」提供，對程序的質量和性能不作任何保證。**程序的質量和性能的全部風險由您承擔。** 如果程序被發現存在缺陷，您將承擔所有必要的服務、維修或更正費用。
>
> 由於維護者精力有限，我們不提供任何形式的使用協助或問題解答。相關問題將被直接關閉！（歡迎提交改進項目文檔的拉取請求；遵循問題模板的錯誤報告或友好問題不受此影響）


有關如何貢獻的詳細資訊，請參閱 [貢獻指南](https://pdf2zh-next.com/community/Contribution-Guide.html)。

<h2 id="預覽">預覽</h2>

<div align="center">
<!-- <img src="./docs/images/preview.gif" width="80%"  alt="preview"/> -->
<img src="https://s.immersivetranslate.com/assets/r2-uploads/images/babeldoc-preview.png" width="80%"/>
</div>

<h2 id="demo">在線服務 🌟</h2>

您可以通過以下任一服務試用我們的應用程序：

- [Immersive Translate - BabelDOC](https://app.immersivetranslate.com/babel-doc/) 提供免費使用額度；詳情請參閱頁面上的常見問題部分。

<h2 id="安裝">安裝與使用</h2>

### 如何安裝

1. [**Windows EXE**](https://pdf2zh-next.com/getting-started/INSTALLATION_winexe.html) <small>推薦用於 Windows</small>
2. [**Docker**](https://pdf2zh-next.com/getting-started/INSTALLATION_docker.html) <small>推薦用於 Linux</small>
3. [**uv** (a Python package manager)](https://pdf2zh-next.com/getting-started/INSTALLATION_uv.html) <small>推薦用於 macOS</small>

---

### 如何使用

1. [使用 **WebUI**](https://pdf2zh-next.com/getting-started/USAGE_webui.html)
2. [使用 **Zotero 插件**](https://github.com/guaguastandup/zotero-pdf2zh) (第三方程序)
3. [使用 **命令行**](https://pdf2zh-next.com/getting-started/USAGE_commandline.html)

針對不同的使用情境，我們提供了多種使用方式。請查看 [此頁面](./getting-started/getting-started.md) 以獲取更多資訊。

<h2 id="usage">高級選項</h2>

有關詳細說明，請參閱我們的 [高級用法](https://pdf2zh-next.com/advanced/advanced.html) 文檔，以獲取每個選項的完整列表。

<h2 id="downstream">二次開發 (APIs)</h2>

<!-- <!-- For downstream applications, please refer to our document about [API Details](./docs/APIS.md) for futher information about: -->

- [Python API](advanced/API/python.md)，如何在其他 Python 程式中使用本程式
<!-- - [HTTP API](./docs/APIS.md#api-http), how to communicate with a server with the program installed -->

<h2 id="langcode">語言代碼</h2>

如果您不知道應該使用什麼代碼來翻譯成您需要的語言，請查閱 [此文檔](https://pdf2zh-next.com/advanced/Language-Codes.html)

<h2 id="acknowledgement">致謝</h2>

- [Immersive Translation](https://immersivetranslate.com) 為本項目的活躍貢獻者提供每月 Pro 會員兌換碼贊助，詳情請見：[CONTRIBUTOR_REWARD.md](https://github.com/funstory-ai/BabelDOC/blob/main/docs/CONTRIBUTOR_REWARD.md)

- [SiliconFlow](https://siliconflow.cn) 為本項目提供免費的翻譯服務，由大型語言模型（LLMs）驅動。

- 1.x 版本：[Byaidu/PDFMathTranslate](https://github.com/Byaidu/PDFMathTranslate)


- 後端：[BabelDOC](https://github.com/funstory-ai/BabelDOC)

- PDF 庫：[PyMuPDF](https://github.com/pymupdf/PyMuPDF)

- PDF 解析：[Pdfminer.six](https://github.com/pdfminer/pdfminer.six)

- PDF 預覽：[Gradio PDF](https://github.com/freddyaboulton/gradio-pdf)

- 版面分析：[DocLayout-YOLO](https://github.com/opendatalab/DocLayout-YOLO)

- PDF 標準：[PDF Explained](https://zxyle.github.io/PDF-Explained/), [PDF Cheat Sheets](https://pdfa.org/resource/pdf-cheat-sheets/)

- 多語言字體：請參閱 [BabelDOC-Assets](https://github.com/funstory-ai/BabelDOC-Assets)

- [Asynchronize](https://github.com/multimeric/Asynchronize/tree/master?tab=readme-ov-file)

- [Rich logging with multiprocessing](https://github.com/SebastianGrans/Rich-multiprocess-logging/tree/main)

- 使用 [Weblate](https://hosted.weblate.org/projects/pdfmathtranslate-next/) 進行文檔國際化


<h2 id="conduct">提交代碼前</h2>

我們歡迎貢獻者的積極參與，讓 pdf2zh 變得更好。在您準備提交代碼之前，請參考我們的 [行為準則](https://pdf2zh-next.com/community/CODE_OF_CONDUCT.html) 和 [貢獻指南](https://pdf2zh-next.com/community/Contribution-Guide.html)。

<h2 id="contrib">貢獻者</h2>

<!-- <a href="https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/graphs/contributors">
  <img src="https://opencollective.com/PDFMathTranslate/contributors.svg?width=890&button=false" />
</a> -->

<!-- ![Alt](https://repobeats.axiom.co/api/embed/45529651750579e099960950f757449a410477ad.svg "Repobeats analytics image") -->

<h2 id="star_hist">Star History</h2>

<a href="https://star-history.com/#PDFMathTranslate-next/PDFMathTranslate-next&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date"/>
 </picture>
</a>

<div align="right"> 
<h6><small>Some content on this page has been translated by GPT and may contain errors.</small></h6>