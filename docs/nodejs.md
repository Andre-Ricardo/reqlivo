---
title: Node.js
---

# Node.js

**Referência solicitada; não é dependência do MVP.** O Reqlivo atual não possui `package.json`, pipeline npm nem servidor Node.js. A interface usa JavaScript no navegador e os scripts de requisição são executados por rquickjs. As APIs de Node.js não fazem parte de `rq.*`.

## Documentação oficial atual

| Referência | Uso |
|---|---|
| [API corrente](https://nodejs.org/api/) | Documentação da linha corrente do Node.js. |
| [API da última LTS](https://nodejs.org/docs/latest-v24.x/api/) | Consulte também a linha LTS identificada no catálogo; este link representa a linha 24. |
| [Versões publicadas](https://nodejs.org/en/about/previous-releases) | Verificação das linhas Current, LTS e suporte. |
| [ECMAScript modules](https://nodejs.org/api/esm.html) | Módulos ESM. |
| [CommonJS](https://nodejs.org/api/modules.html) | Módulos CommonJS. |
| [Processo e ambiente](https://nodejs.org/api/process.html) | `process` e `process.env`. |

O catálogo registra separadamente a versão corrente e a LTS consultadas. Esses canais podem estar em versões diferentes; a API corrente não garante compatibilidade com uma LTS anterior.

## Se for adotado futuramente

Uma adoção de Node.js deve definir sua finalidade, versão, manifesto, lockfile e comandos de build. O uso de uma ferramenta de frontend não implica que o produto precise executar um servidor Node.js. Essa integração ainda precisaria de implementação e validação próprias.

Na consulta oficial via Context7, `process.env` aparece nas variantes ESM e CommonJS. Essa informação serve para ferramentas Node.js; o acesso a segredos pelo Reqlivo continua seguindo seu [próprio contrato](formato.md).

## Consulta Context7

ID resolvido: `/websites/nodejs_api`. Tópico consultado: ESM, CommonJS e `process.env`.

[Voltar ao catálogo](tecnologias.md)
