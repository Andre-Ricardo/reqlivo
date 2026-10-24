---
title: Tecnologias e documentação
---

# Tecnologias e documentação

Referências oficiais consultadas em **2026-10-24**, com apoio do **Context7**. Este catálogo traz guias locais de orientação e links para a documentação upstream, sem reproduzir integralmente os manuais de terceiros.

**Observado estaticamente:** o inventário do projeto vem de `Cargo.toml`, `Cargo.lock`, código e configuração do site. **Documentação oficial:** os tópicos das bibliotecas centrais foram consultados no Context7; os números de versões foram conferidos nos registros oficiais. O Context7 não foi usado como registro de releases.

## Guias por tecnologia

| Guia | Papel no projeto | Documentação atual |
|---|---|---|
| [Rust e Cargo](rust.md) | Núcleo, CLI e servidor | Rust estável **1.99.1** |
| [Bibliotecas Rust](bibliotecas-rust.md) | HTTP, runtime, scripts, contratos e CLI | Links `latest` e versões fixadas abaixo |
| [Web, Python e formatos](web-e-formatos.md) | Interface, demonstração, coleção e site | Referências correntes oficiais |
| [Node.js](nodejs.md) | Referência solicitada; não utilizado pelo MVP | Current **v26.12.1**; LTS **v24.22.1** |
| [Tauri](tauri.md) | Referência solicitada; não integrado | Crate `tauri` estável **2.12.5**, documentação Tauri 2 |
| [Atualização com Context7](context7.md) | Procedimento de manutenção das referências | IDs e consultas rastreáveis |

## O que significa “mais recente”

As versões acima e abaixo são uma fotografia da consulta. Para Rust foi usado o canal `stable`; para Node.js foram registrados Current e LTS separadamente; para crates foi usada a última versão estável informada pelo crates.io, conferindo a ausência de retirada (`yanked`). Prereleases não são o alvo deste catálogo.

Os links `latest`, `stable` e de documentação corrente acompanham atualizações upstream. O conteúdo indexado no Context7 pode ter defasagem ou incluir páginas históricas; por isso os guias apontam também para os sites oficiais. Uma versão nova pode não corresponder à API implementada no Reqlivo.

Esta atualização é documental. `Cargo.toml`, `Cargo.lock` e o comportamento do aplicativo permanecem nas versões do projeto. Edição Rust 2024 e mínimo declarado 1.85 são informações diferentes da última versão estável do compilador.

## Dependências diretas

Todas as **22 dependências diretas** estão listadas: 20 de produção e 2 de testes. A versão fixada corresponde à dependência direta resolvida pelo pacote Reqlivo, mesmo quando o lockfile contém outra versão do mesmo crate como dependência transitiva.

| Biblioteca | Fixada no projeto | Estável mais recente | Função | Documentação |
|---|---|---|---|---|
| `axum` | 0.8.9 | 0.8.9 | Servidor HTTP e API da interface (produção) | [fixada](https://docs.rs/axum/0.8.9/) · [mais recente](https://docs.rs/axum/latest/) |
| `base64` | 0.22.1 | 0.23.1 | Autenticação Basic e dados binários (produção) | [fixada](https://docs.rs/base64/0.22.1/) · [mais recente](https://docs.rs/base64/latest/) |
| `chrono` | 0.4.45 | 0.4.45 | Datas e horários (produção) | [fixada](https://docs.rs/chrono/0.4.45/) · [mais recente](https://docs.rs/chrono/latest/) |
| `clap` | 4.6.7 | 4.6.7 | Parser e ajuda da CLI (produção) | [fixada](https://docs.rs/clap/4.6.7/) · [mais recente](https://docs.rs/clap/latest/) |
| `hex` | 0.4.3 | 0.4.3 | Representação hexadecimal de hashes (produção) | [fixada](https://docs.rs/hex/0.4.3/) · [mais recente](https://docs.rs/hex/latest/) |
| `mime_guess` | 2.0.5 | 2.0.5 | Tipos MIME de arquivos (produção) | [fixada](https://docs.rs/mime_guess/2.0.5/) · [mais recente](https://docs.rs/mime_guess/latest/) |
| `reqwest` | 0.12.28 | 0.13.5 | Transporte HTTP e TLS (produção) | [fixada](https://docs.rs/reqwest/0.12.28/) · [mais recente](https://docs.rs/reqwest/latest/) |
| `rquickjs` | 0.14.0 | 0.14.0 | Execução embutida de JavaScript (produção) | [fixada](https://docs.rs/rquickjs/0.14.0/) · [mais recente](https://docs.rs/rquickjs/latest/) |
| `regex` | 1.13.1 | 1.13.1 | Expressões regulares, asserções e mascaramento (produção) | [fixada](https://docs.rs/regex/1.13.1/) · [mais recente](https://docs.rs/regex/latest/) |
| `roxmltree` | 0.20.0 | 0.21.2 | Análise e validação XML (produção) | [fixada](https://docs.rs/roxmltree/0.20.0/) · [mais recente](https://docs.rs/roxmltree/latest/) |
| `serde` | 1.0.229 | 1.0.229 | Serialização dos contratos (produção) | [fixada](https://docs.rs/serde/1.0.229/) · [mais recente](https://docs.rs/serde/latest/) |
| `serde_json` | 1.0.154 | 1.0.154 | JSON e relatórios (produção) | [fixada](https://docs.rs/serde_json/1.0.154/) · [mais recente](https://docs.rs/serde_json/latest/) |
| `serde_yaml` | 0.9.34+deprecated | 0.9.34+deprecated | Arquivos YAML da coleção (produção) | [fixada](https://docs.rs/serde_yaml/0.9.34+deprecated/) · [mais recente](https://docs.rs/serde_yaml/latest/) |
| `sha2` | 0.10.9 | 0.11.1 | Hashes e revisões (produção) | [fixada](https://docs.rs/sha2/0.10.9/) · [mais recente](https://docs.rs/sha2/latest/) |
| `shell-words` | 1.1.1 | 1.1.1 | Interpretação de argumentos cURL (produção) | [fixada](https://docs.rs/shell-words/1.1.1/) · [mais recente](https://docs.rs/shell-words/latest/) |
| `thiserror` | 2.0.21 | 2.0.21 | Tipos de erro (produção) | [fixada](https://docs.rs/thiserror/2.0.21/) · [mais recente](https://docs.rs/thiserror/latest/) |
| `tokio` | 1.54.0 | 1.54.1 | Runtime assíncrono e sincronização (produção) | [fixada](https://docs.rs/tokio/1.54.0/) · [mais recente](https://docs.rs/tokio/latest/) |
| `tokio-util` | 0.7.20 | 0.7.20 | Cancelamento de execuções (produção) | [fixada](https://docs.rs/tokio-util/0.7.20/) · [mais recente](https://docs.rs/tokio-util/latest/) |
| `url` | 2.5.8 | 2.5.8 | Análise e composição de URLs (produção) | [fixada](https://docs.rs/url/2.5.8/) · [mais recente](https://docs.rs/url/latest/) |
| `uuid` | 1.26.1 | 1.29.0 | Identificadores (produção) | [fixada](https://docs.rs/uuid/1.26.1/) · [mais recente](https://docs.rs/uuid/latest/) |
| `tempfile` | 3.27.0 | 3.27.0 | Diretórios temporários de testes (teste) | [fixada](https://docs.rs/tempfile/3.27.0/) · [mais recente](https://docs.rs/tempfile/latest/) |
| `tower` | 0.5.3 | 0.5.3 | Utilitários de serviço nos testes da API (teste) | [fixada](https://docs.rs/tower/0.5.3/) · [mais recente](https://docs.rs/tower/latest/) |

Dependências transitivas não são apresentadas como decisões diretas da arquitetura; suas versões completas continuam registradas em `Cargo.lock`.

### Diferenças encontradas

Há versões estáveis mais novas de algumas bibliotecas, incluindo Reqwest, base64, roxmltree, sha2, Tokio e uuid. Isso não implica compatibilidade automática. Consulte a documentação fixada para manutenção do código atual e a documentação corrente para planejar uma atualização.

`serde_yaml` permanece em `0.9.34+deprecated` no projeto e no registro consultado. Essa condição foi mantida explícita; uma documentação com URL `latest` não significa manutenção ativa.

## Fontes e rastreabilidade

- [Snapshot de versões e consultas Context7](tecnologias-versoes.json): versões, requisitos declarados, escopos, fontes e IDs resolvidos.
- [Canal estável do Rust](https://static.rust-lang.org/dist/channel-rust-stable.toml): versão e data de publicação.
- [Índice oficial de releases do Node.js](https://nodejs.org/dist/index.json): versão corrente e LTS.
- [crates.io](https://crates.io/): a URL de consulta de cada crate consta no snapshot.
- [Manifesto do Reqlivo](https://github.com/Andre-Ricardo/reqlivo/blob/main/Cargo.toml) e [lockfile](https://github.com/Andre-Ricardo/reqlivo/blob/main/Cargo.lock): versões efetivamente usadas.

[Voltar à documentação](index.md)
