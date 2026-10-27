---
title: Web, Python e formatos
---

# Web, Python e formatos

## JavaScript, HTML e CSS

**Observado estaticamente:** a interface em `src/assets/` usa HTML, CSS e JavaScript sem framework. Os assets são empacotados pelo Tauri, conforme `src-tauri/tauri.conf.json`. A WebView e o runtime de scripts de requisição são contextos diferentes.

Referências correntes:

| Referência | Uso |
|---|---|
| [JavaScript](https://developer.mozilla.org/en-US/docs/Web/JavaScript) | Linguagem e biblioteca padrão do navegador. |
| [HTML](https://developer.mozilla.org/en-US/docs/Web/HTML) | Estrutura, formulários e elementos semânticos. |
| [CSS](https://developer.mozilla.org/en-US/docs/Web/CSS) | Estilos, layout e responsividade. |
| [DOM](https://developer.mozilla.org/en-US/docs/Web/API/Document_Object_Model) | Manipulação da interface. |
| [Fetch](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API/Using_Fetch) | Comunicação com a API local. |
| [AbortController](https://developer.mozilla.org/en-US/docs/Web/API/AbortController) | Cancelamento de operações do navegador. |

**Documentação oficial consultada:** a MDN descreve o uso de `AbortSignal` em `fetch` e a rejeição com `AbortError` quando a operação é interrompida. Isso não significa, por si só, que o servidor cancelou o trabalho; o cancelamento de uma execução Reqlivo tem contrato próprio na API.

Context7: `/mdn/content`, consulta sobre Fetch e AbortController. Essas referências web são atualizadas continuamente; não há uma única versão de pacote compartilhada por HTML, CSS e todas as APIs do navegador. Verifique a compatibilidade de cada API nos navegadores suportados.

## Python: servidor de demonstração

**Observado estaticamente:** `examples/servidor_demo.py` usa somente a biblioteca padrão Python: `http.server`, `base64`, `gzip`, `json`, `os` e `time`. A demonstração escuta em `127.0.0.1:18080`.

- [Documentação estável atual do Python 3](https://docs.python.org/3/).
- [http.server](https://docs.python.org/3/library/http.server.html).
- [Biblioteca padrão](https://docs.python.org/3/library/index.html).

**Documentação oficial consultada:** `ThreadingHTTPServer` trata conexões com threads; `BaseHTTPRequestHandler` delega métodos HTTP a handlers como `do_GET` e `do_POST`. Context7: `/python/cpython`.

O índice do repositório CPython pode incluir desenvolvimento da próxima versão. Para a documentação publicada, use o caminho oficial `/3/` acima. O script é uma ferramenta de demonstração local com dados sintéticos; Python não é requisito do binário Reqlivo.

## OpenCollection e YAML

**Observado estaticamente:** o Reqlivo declara OpenCollection **1.0.0**, usa arquivos YAML e registra seus dados específicos em `extensions.reqlivo`. O contrato implementado está em [Formato da coleção](formato.md).

**Documentação oficial consultada:** a especificação corrente encontrada no Context7 declara **1.0.0**, define `opencollection` como versão do formato e permite dados específicos de fornecedores em `extensions`. A existência de outros protocolos na especificação não implica suporte deles no Reqlivo.

- [Especificação OpenCollection](https://spec.opencollection.com/).
- [Schema OpenCollection](https://schema.opencollection.com/).
- [Repositório oficial](https://github.com/opencollection-dev/opencollection).
- [Especificação YAML 1.2.2](https://yaml.org/spec/1.2.2/).

Context7: `/opencollection-dev/opencollection`. O documento YAML é interpretado pelo parser efetivamente usado pelo projeto; a especificação de referência não equivale a uma declaração de conformidade completa do parser.

## Site de documentação e automação

**Observado estaticamente:** `docs/_config.yml` configura Jekyll e `jekyll-relative-links`; GitHub Pages publica `main:/docs`. `.github/workflows/ci.yml` usa GitHub Actions para validar o crate.

- [GitHub Pages](https://docs.github.com/en/pages).
- [Jekyll](https://jekyllrb.com/docs/).
- [jekyll-relative-links](https://github.com/benbalter/jekyll-relative-links).
- [Versões fornecidas pelo GitHub Pages](https://pages.github.com/versions/).
- [GitHub Actions](https://docs.github.com/en/actions).

O ambiente de Pages é gerenciado pelo GitHub. A versão mais recente do Jekyll upstream pode diferir da versão suportada pelo serviço; o projeto não fixa nem instala uma versão própria do gerador.

[Voltar ao catálogo](tecnologias.md)
