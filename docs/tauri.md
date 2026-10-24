---
title: Tauri
---

# Tauri

**Referência solicitada; não está integrado ao MVP.** O projeto atual serve sua interface pelo Axum em loopback. Não há diretório `src-tauri`, configuração Tauri nem dependência `tauri` no manifesto.

## Documentação oficial atual

| Referência | Uso |
|---|---|
| [Documentação Tauri 2](https://v2.tauri.app/) | Guia da linha atual de documentação. |
| [Pré-requisitos](https://v2.tauri.app/start/prerequisites/) | Dependências de desenvolvimento por sistema operacional. |
| [Modelo de processos](https://v2.tauri.app/concept/process-model/) | Core Rust, WebViews e comunicação. |
| [Capabilities](https://v2.tauri.app/security/capabilities/) | Escopo das permissões por janela e WebView. |
| [Configuração](https://v2.tauri.app/reference/config/) | Esquema da configuração do aplicativo. |
| [API Rust mais recente](https://docs.rs/tauri/latest/tauri/) | API da última versão publicada do crate. |

A versão exata mais recente do crate está no [catálogo](tecnologias.md). Os pacotes Rust, CLI, plugins e pacotes JavaScript têm versões próprias; o número do crate `tauri` não identifica automaticamente a versão de todos eles.

## Orientação para uma possível integração

O modelo de processos documentado separa o core das WebViews. O sistema de capabilities delimita o acesso do frontend às funcionalidades expostas. Uma futura integração do Reqlivo precisaria definir os comandos e permissões necessários, o empacotamento e a política de acesso local, além de testar cada plataforma suportada.

As referências acima são material de estudo. Elas não representam uma migração aprovada ou uma funcionalidade já implementada.

## Consulta Context7

ID resolvido: `/tauri-apps/tauri-docs`. Tópicos: modelo de processos e capabilities. O retorno incluiu também textos históricos de beta e migração; eles não foram usados como instrução da versão estável atual.

[Voltar ao catálogo](tecnologias.md)
