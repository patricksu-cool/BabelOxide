<div align="center">

<img src="./docs/images/banner.png" width="320px"  alt="banner"/>

<h2 id="제목">PDFMathTranslate</h2>

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

PDF 과학 논문 번역 및 이중 언어 비교. [BabelDOC](https://github.com/funstory-ai/BabelDOC) 을 기반으로 합니다. 또한 이 프로젝트는 BabelDOC 을 호출하여 PDF 번역을 수행하기 위한 공식 참조 구현체이기도 합니다.

- 📊 수식, 차트, 목차 및 주석을 보존합니다 _([미리보기](#preview))_.
- 🌐 [다양한 언어](https://pdf2zh-next.com/supported_languages.html) 를 지원하며, 다양한 [번역 서비스](https://pdf2zh-next.com/advanced/Documentation-of-Translation-Services.html) 를 제공합니다.
- 🤖 [명령줄 도구](https://pdf2zh-next.com/getting-started/USAGE_commandline.html), [대화형 사용자 인터페이스](https://pdf2zh-next.com/getting-started/USAGE_webui.html) 및 [Docker](https://pdf2zh-next.com/getting-started/INSTALLATION_docker.html) 를 제공합니다.

<!-- Feel free to provide feedback in [GitHub Issues](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/issues) or [Telegram Group](https://t.me/+Z9_SgnxmsmA5NzBl). -->

> [!WARNING]
>
> 이 프로젝트는 [AGPL v3](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/blob/main/LICENSE) 라이선스에 따라 "있는 그대로" 제공되며, 프로그램의 품질과 성능에 대한 어떠한 보증도 제공되지 않습니다. **프로그램의 품질과 성능에 대한 모든 위험은 귀하가 부담합니다.** 프로그램에 결함이 발견될 경우, 귀하는 모든 필요한 서비스, 수리 또는 수정 비용을 부담해야 합니다.
>
> 유지 관리자의 제한된 에너지로 인해, 우리는 어떠한 형태의 사용 지원이나 문제 해결도 제공하지 않습니다. 관련된 이슈는 직접적으로 닫힐 것입니다! (프로젝트 문서 개선을 위한 Pull Request 는 환영합니다; 이슈 템플릿을 따르는 버그 리포트나 우호적인 이슈는 이에 영향을 받지 않습니다)


자세한 기여 방법은 [기여 가이드](https://pdf2zh-next.com/community/Contribution-Guide.html) 를 참고하세요.

<h2 id="preview">미리보기</h2>

<div align="center">
<!-- <img src="./docs/images/preview.gif" width="80%"  alt="preview"/> -->
<img src="https://s.immersivetranslate.com/assets/r2-uploads/images/babeldoc-preview.png" width="80%"/>
</div>

<h2 id="demo">온라인 서비스 🌟</h2>

다음 서비스 중 하나를 사용하여 저희 애플리케이션을 시험해 볼 수 있습니다:

- [Immersive Translate - BabelDOC](https://app.immersivetranslate.com/babel-doc/) 무료 사용 할당량이 제공됩니다. 자세한 내용은 페이지의 FAQ 섹션을 참조하세요.

<h2 id="설치">설치 및 사용법</h2>

### 설치

1. [**Windows EXE**](https://pdf2zh-next.com/getting-started/INSTALLATION_winexe.html) <small>Windows 에 권장</small>
2. [**Docker**](https://pdf2zh-next.com/getting-started/INSTALLATION_docker.html) <small>Linux 에 권장</small>
3. [**uv** (Python 패키지 관리자)](https://pdf2zh-next.com/getting-started/INSTALLATION_uv.html) <small>macOS 에 권장</small>

---

### 사용법

1. [**WebUI** 사용](https://pdf2zh-next.com/getting-started/USAGE_webui.html)
2. [**Zotero Plugin** 사용](https://github.com/guaguastandup/zotero-pdf2zh) (서드파티 프로그램)
3. [**Commandline** 사용](https://pdf2zh-next.com/getting-started/USAGE_commandline.html)

다양한 사용 사례에 맞춰, 저희 프로그램을 사용할 수 있는 여러 방법을 제공합니다. 자세한 내용은 [이 페이지](./getting-started/getting-started.md) 를 확인하세요.

<h2 id="usage">고급 옵션</h2>

자세한 설명은 각 옵션의 전체 목록을 확인할 수 있는 [고급 사용법](https://pdf2zh-next.com/advanced/advanced.html) 문서를 참조하세요.

<h2 id="downstream">2 차 개발 (API)</h2>

<!-- <!-- For downstream applications, please refer to our document about [API Details](./docs/APIS.md) for futher information about: -->

- [Python API](advanced/API/python.md), 다른 Python 프로그램에서 이 프로그램을 사용하는 방법
<!-- - [HTTP API](./docs/APIS.md#api-http), how to communicate with a server with the program installed -->

<h2 id="langcode">언어 코드</h2>

필요한 언어로 번역하기 위해 어떤 코드를 사용해야 할지 모르겠다면, [이 문서](https://pdf2zh-next.com/advanced/Language-Codes.html) 를 확인하세요.

<h2 id="acknowledgement">감사의 말</h2>

- [Immersive Translation](https://immersivetranslate.com) 은 이 프로젝트의 활발한 기여자를 위해 월간 Pro 멤버십 교환 코드를 후원합니다. 자세한 내용은 [CONTRIBUTOR_REWARD.md](https://github.com/funstory-ai/BabelDOC/blob/main/docs/CONTRIBUTOR_REWARD.md) 에서 확인하세요.

- [SiliconFlow](https://siliconflow.cn) 는 이 프로젝트를 위해 대규모 언어 모델 (LLM) 로 구동되는 무료 번역 서비스를 제공합니다.

- 1.x 버전: [Byaidu/PDFMathTranslate](https://github.com/Byaidu/PDFMathTranslate)


- 백엔드: [BabelDOC](https://github.com/funstory-ai/BabelDOC)

- PDF 라이브러리: [PyMuPDF](https://github.com/pymupdf/PyMuPDF)

- PDF 파싱: [Pdfminer.six](https://github.com/pdfminer/pdfminer.six)

- PDF 미리보기: [Gradio PDF](https://github.com/freddyaboulton/gradio-pdf)

- 레이아웃 파싱: [DocLayout-YOLO](https://github.com/opendatalab/DocLayout-YOLO)

- PDF 표준: [PDF Explained](https://zxyle.github.io/PDF-Explained/), [PDF Cheat Sheets](https://pdfa.org/resource/pdf-cheat-sheets/)

- 다국어 글꼴: [BabelDOC-Assets](https://github.com/funstory-ai/BabelDOC-Assets) 참조

- [Asynchronize](https://github.com/multimeric/Asynchronize/tree/master?tab=readme-ov-file)

- [Rich logging with multiprocessing](https://github.com/SebastianGrans/Rich-multiprocess-logging/tree/main)

- [Weblate](https://hosted.weblate.org/projects/pdfmathtranslate-next/) 를 사용한 문서 i18n


<h2 id="conduct">코드 제출 전에</h2>

pdf2zh 를 더 나은 방향으로 발전시키기 위해 기여자의 적극적인 참여를 환영합니다. 코드를 제출할 준비가 되셨다면, [행동 강령](https://pdf2zh-next.com/community/CODE_OF_CONDUCT.html) 과 [기여 가이드](https://pdf2zh-next.com/community/Contribution-Guide.html) 를 참고해 주시기 바랍니다.

<h2 id="contrib">기여자</h2>

<!-- <a href="https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/graphs/contributors">
  <img src="https://opencollective.com/PDFMathTranslate/contributors.svg?width=890&button=false" />
</a> -->

<!-- ![Alt](https://repobeats.axiom.co/api/embed/45529651750579e099960950f757449a410477ad.svg "Repobeats analytics image") -->

<h2 id="스타_히스토리">Star History</h2>

<a href="https://star-history.com/#PDFMathTranslate-next/PDFMathTranslate-next&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date"/>
 </picture>
</a>

<div align="right"> 
<h6><small>이 페이지의 일부 내용은 GPT 에 의해 번역되었으며 오류가 포함될 수 있습니다.</small></h6>