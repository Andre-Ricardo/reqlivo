---
title: Bibliotecas Rust
---

# Bibliotecas Rust

O [inventário completo](tecnologias.md) contém todas as dependências diretas de produção e de testes, com links para a versão usada e para a documentação mais recente. Esta página orienta a leitura das bibliotecas centrais.

## Axum: protocolo interno

**No projeto:** `src/server.rs` define rotas, estado e respostas JSON. O adaptador IPC em `src/desktop.rs` executa essas rotas em memória, sem servidor HTTP.

**Documentação oficial consultada:** o extrator `State` lê o estado fornecido ao `Router`; deve preceder os extratores que consomem o corpo. Consulte [Router](https://docs.rs/axum/latest/axum/struct.Router.html) e [State](https://docs.rs/axum/latest/axum/extract/struct.State.html).

Context7: `/websites/rs_axum_axum`, consulta sobre `Router`, `State` e `Json`.

## Tokio e tokio-util: execução e cancelamento

**No projeto:** Tokio fornece o runtime e a sincronização; `CancellationToken` de tokio-util é usado no cancelamento dos envios e execuções.

**Documentação oficial consultada:** o cancelamento de uma tarefa assíncrona é cooperativo; `abort` sinaliza a interrupção e não garante que trabalho bloqueante já iniciado pare. Consulte [Tasks](https://docs.rs/tokio/latest/tokio/task/) e [CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html).

Context7: `/websites/rs_tokio_tokio`, consulta sobre runtime, tarefas e cancelamento.

## Reqwest: cliente HTTP

**No projeto:** `src/transport.rs` implementa transporte, redirecionamentos, limites e modo offline. As features efetivas estão em `Cargo.toml`.

**Documentação oficial consultada:** `ClientBuilder::timeout` estabelece o prazo total da requisição, incluindo leitura do corpo. Consulte [ClientBuilder](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html).

Context7: `/websites/rs_reqwest`, consulta sobre timeout, redirects e TLS. A documentação corrente pode apresentar nomes de features e métodos diferentes dos da série **0.12** usada pelo projeto; a coluna de documentação da versão fixada deve orientar alterações no código atual.

## rquickjs: scripts JavaScript

**No projeto:** `src/scripts.rs` cria o runtime e os contextos, define limites e expõe a API de scripts do Reqlivo.

**Documentação oficial consultada:** o runtime oferece limite de memória e callback de interrupção. Consulte [Runtime](https://docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html). A API disponível ao usuário do Reqlivo é um subconjunto definido em [Formato da coleção](formato.md#scripts).

Context7: `/delskayn/rquickjs`, consulta sobre runtime, contextos, memória e interrupção.

## Serde, JSON e YAML: persistência e contratos

**No projeto:** `src/model.rs` declara os contratos; `src/store.rs` lê e grava arquivos e `src/import_export.rs` transforma formatos.

**Documentação oficial consultada:** `default` preenche valores ausentes e `flatten` incorpora campos ou captura campos adicionais. `flatten` tem restrições com `deny_unknown_fields`; consulte [Atributos de campo](https://serde.rs/field-attrs.html).

Context7: `/websites/serde_rs`. Veja também [serde_json](https://docs.rs/serde_json/latest/serde_json/) e [serde_yaml](https://docs.rs/serde_yaml/latest/serde_yaml/). A versão fixada de `serde_yaml` tem o sufixo `+deprecated`; apontar para `latest` não remove essa condição. Uma substituição exigiria avaliação de compatibilidade própria.

## clap: CLI

**No projeto:** `src/main.rs` usa `Parser` e `Subcommand`; `--collection` e `--json` são argumentos globais.

**Documentação oficial consultada:** argumentos globais propagam-se aos subcomandos. Consulte o [tutorial derive](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) e [Arg::global](https://docs.rs/clap/latest/clap/builder/struct.Arg.html#method.global).

Context7: `/websites/rs_clap`, consulta sobre derive, subcomandos e argumentos globais.

[Voltar ao catálogo](tecnologias.md)
