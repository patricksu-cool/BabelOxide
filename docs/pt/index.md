<div align="center">

<img src="./docs/images/banner.png" width="320px"  alt="banner"/>

<h2 id="título">PDFMathTranslate</h2>

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

Tradução de artigos científicos em PDF e comparação bilíngue. Baseado em [BabelDOC](https://github.com/funstory-ai/BabelDOC). Além disso, este projeto também é a implementação de referência oficial para chamar o BabelDOC para realizar a tradução de PDF.

- 📊 Preserve fórmulas, gráficos, sumário e anotações _([prévia](#preview))_.
- 🌐 Suporta [múltiplos idiomas](https://pdf2zh-next.com/supported_languages.html) e diversos [serviços de tradução](https://pdf2zh-next.com/advanced/Documentation-of-Translation-Services.html).
- 🤖 Oferece [ferramenta de linha de comando](https://pdf2zh-next.com/getting-started/USAGE_commandline.html), [interface de usuário interativa](https://pdf2zh-next.com/getting-started/USAGE_webui.html) e [Docker](https://pdf2zh-next.com/getting-started/INSTALLATION_docker.html)

<!-- Feel free to provide feedback in [GitHub Issues](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/issues) or [Telegram Group](https://t.me/+Z9_SgnxmsmA5NzBl). -->

> [!WARNING]
>
> Este projeto é fornecido "como está" sob a licença [AGPL v3](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/blob/main/LICENSE), e não são fornecidas garantias para a qualidade e desempenho do programa. **Todo o risco da qualidade e desempenho do programa é suportado por você.** Se o programa for considerado defeituoso, você será responsável por todos os custos necessários de serviço, reparo ou correção.
>
> Devido à energia limitada dos mantenedores, não fornecemos qualquer forma de assistência de uso ou resolução de problemas. Questões relacionadas serão fechadas diretamente! (Pull requests para melhorar a documentação do projeto são bem-vindos; bugs ou questões amigáveis que seguem o modelo de problema não são afetados por isso)


Para obter detalhes sobre como contribuir, consulte o [Guia de Contribuição](https://pdf2zh-next.com/community/Contribution-Guide.html).

<h2 id="preview">Pré-visualização</h2>

<div align="center">
<!-- <img src="./docs/images/preview.gif" width="80%"  alt="preview"/> -->
<img src="https://s.immersivetranslate.com/assets/r2-uploads/images/babeldoc-preview.png" width="80%"/>
</div>

<h2 id="demo">Serviço Online 🌟</h2>

Você pode experimentar nossa aplicação usando qualquer um dos seguintes serviços:

- [Immersive Translate - BabelDOC](https://app.immersivetranslate.com/babel-doc/) Cota de uso gratuito disponível; consulte a seção de Perguntas frequentes na página para obter detalhes.

<h2 id="instalacao">Instalação e Uso</h2>

### Instalação

1. [**Windows EXE**](https://pdf2zh-next.com/getting-started/INSTALLATION_winexe.html) <small>Recomendado para Windows</small>
2. [**Docker**](https://pdf2zh-next.com/getting-started/INSTALLATION_docker.html) <small>Recomendado para Linux</small>
3. [**uv** (um gerenciador de pacotes Python)](https://pdf2zh-next.com/getting-started/INSTALLATION_uv.html) <small>Recomendado para macOS</small>

---

### Uso

1. [Usando **WebUI**](https://pdf2zh-next.com/getting-started/USAGE_webui.html)
2. [Usando **Plugin do Zotero**](https://github.com/guaguastandup/zotero-pdf2zh) (Programa de terceiros)
3. [Usando **Linha de comando**](https://pdf2zh-next.com/getting-started/USAGE_commandline.html)

Para diferentes casos de uso, fornecemos métodos distintos para usar nosso programa. Confira [esta página](./getting-started/getting-started.md) para mais informações.

<h2 id="uso">Opções Avançadas</h2>

Para explicações detalhadas, consulte nosso documento sobre [Uso Avançado](https://pdf2zh-next.com/advanced/advanced.html) para obter uma lista completa de cada opção.

<h2 id="desenvolvimento-secundario">Desenvolvimento Secundário (APIs)</h2>

<!-- <!-- For downstream applications, please refer to our document about [API Details](./docs/APIS.md) for futher information about: -->

- [API Python](advanced/API/python.md), como usar o programa em outros programas Python
<!-- - [HTTP API](./docs/APIS.md#api-http), how to communicate with a server with the program installed -->

<h2 id="código-do-idioma">Código do Idioma</h2>

Se você não sabe qual código usar para traduzir para o idioma que precisa, consulte [esta documentação](https://pdf2zh-next.com/advanced/Language-Codes.html)

<h2 id="agradecimentos">Agradecimentos</h2>

- [Immersive Translation](https://immersivetranslate.com) patrocina códigos de resgate de assinatura Pro mensal para colaboradores ativos deste projeto, veja os detalhes em: [CONTRIBUTOR_REWARD.md](https://github.com/funstory-ai/BabelDOC/blob/main/docs/CONTRIBUTOR_REWARD.md)

- [SiliconFlow](https://siliconflow.cn) fornece um serviço de tradução gratuito para este projeto, alimentado por grandes modelos de linguagem (LLMs).

- Versão 1.x: [Byaidu/PDFMathTranslate](https://github.com/Byaidu/PDFMathTranslate)


- backend: [BabelDOC](https://github.com/funstory-ai/BabelDOC)

- Biblioteca PDF: [PyMuPDF](https://github.com/pymupdf/PyMuPDF)

- Análise de PDF: [Pdfminer.six](https://github.com/pdfminer/pdfminer.six)

- Visualização de PDF: [Gradio PDF](https://github.com/freddyaboulton/gradio-pdf)

- Análise de Layout: [DocLayout-YOLO](https://github.com/opendatalab/DocLayout-YOLO)

- Padrões PDF: [PDF Explained](https://zxyle.github.io/PDF-Explained/), [PDF Cheat Sheets](https://pdfa.org/resource/pdf-cheat-sheets/)

- Fonte Multilíngue: veja [BabelDOC-Assets](https://github.com/funstory-ai/BabelDOC-Assets)

- [Asynchronize](https://github.com/multimeric/Asynchronize/tree/master?tab=readme-ov-file)

- [Registro rico com multiprocessamento](https://github.com/SebastianGrans/Rich-multiprocess-logging/tree/main)

- Documentação i18n usando [Weblate](https://hosted.weblate.org/projects/pdfmathtranslate-next/)


<h2 id="conduta">Antes de enviar seu código</h2>

Agradecemos a participação ativa dos colaboradores para tornar o pdf2zh melhor. Antes de estar pronto para enviar seu código, consulte nosso [Código de Conduta](https://pdf2zh-next.com/community/CODE_OF_CONDUCT.html) e [Guia de Contribuição](https://pdf2zh-next.com/community/Contribution-Guide.html).

<h2 id="contribuidores">Colaboradores</h2>

<!-- <a href="https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/graphs/contributors">
  <img src="https://opencollective.com/PDFMathTranslate/contributors.svg?width=890&button=false" />
</a> -->

<!-- ![Alt](https://repobeats.axiom.co/api/embed/45529651750579e099960950f757449a410477ad.svg "Repobeats analytics image") -->

<h2 id="histórico_de_estrelas">Star History</h2>

<a href="https://star-history.com/#PDFMathTranslate-next/PDFMathTranslate-next&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=PDFMathTranslate-next/PDFMathTranslate-next&type=Date"/>
 </picture>
</a>

<div align="right"> 
<h6><small>Parte do conteúdo desta página foi traduzida pelo GPT e pode conter erros.</small></h6>