---
title: Tauri
---

# Tauri

**Integrado ao aplicativo desktop.** O workspace contém `src-tauri`, configuração Tauri 2, capability da janela local e comandos IPC. Consulte [desktop e releases](desktop.md) para arquitetura, pré-requisitos e validação.

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

## Integração atual

A interface usa `window.__TAURI__.core.invoke` e assets empacotados. O núcleo Rust mantém transporte, scripts, persistência e runner. Não há servidor HTTP da interface. A capability permite apenas a janela local; seleção de pasta, confirmações e salvamento usam diálogos Rust. Linux e Windows são os alvos de build.

## Consulta Context7

ID resolvido: `/tauri-apps/tauri-docs`. Tópicos: modelo de processos e capabilities. O retorno incluiu também textos históricos de beta e migração; eles não foram usados como instrução da versão estável atual.

[Voltar ao catálogo](tecnologias.md)
