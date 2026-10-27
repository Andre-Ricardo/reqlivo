---
title: Escopo do MVP
---

# Escopo do MVP

Este documento registra o que foi implementado, como validar e o que ainda não está coberto, conectando os critérios de aceite ao código atual.

## Tese entregue

O Reqlivo MVP demonstra que é possível ter um cliente HTTP útil, local e sem conta, com:

- projeto em arquivo legível e versionável;
- CLI e interface desktop sobre o mesmo modelo Rust;
- execução HTTP previsível e observável;
- ambientes, variáveis, auth e scripts;
- runner com relatório e hash determinístico;
- importação/exportação com relatório de perdas;
- segredos por referência, redaction, offline real e acessibilidade de teclado no fluxo principal.

O produto **não** tenta ser um substituto completo de Insomnia/Bruno nesta fase.

## Dentro do MVP implementado

| Área | Entrega |
|---|---|
| Projeto | OpenCollection 1.0.0, manifesto, pastas, requisições, tags, docs e settings. |
| HTTP | Métodos, query/path, headers, JSON/texto/XML/SPARQL, form-urlencoded e multipart. |
| Resposta | Status, headers, corpo, tempo, tamanho, URL final, redirects e truncamento. |
| Ambientes | Herança `extends`, ambiente padrão, precedência e edição pela UI/CLI. |
| Auth | Basic, Bearer, API key (header/query) e herança de pasta/coleção. |
| Scripts | Pré, pós e testes em QuickJS, com timeout, limites e `rq.*`. |
| Testes | `test`/`expect` e asserções declarativas. |
| Runner | Filtros, status, relatório, logs, histórico, cancelamento e hash determinístico. |
| Importação | cURL, Postman v2.1 (subconjunto), OpenCollection (arquivo/diretório). |
| Exportação | cURL, Postman, OpenCollection bundled e projeto sanitizado. |
| Segurança | Segredo por `provider: env`, redaction em logs/relatórios/exportações, IPC local. |
| Persistência | Rascunhos versionados, reconciliação de alterações externas, lixeira restaurável e migração com rollback. |
| Offline | Bloqueio de destinos externos e de redirects externos. |
| Acessibilidade | Foco visível, navegação por teclado na árvore, atalhos e diálogos nativos. |

## Fora do MVP

- gRPC, WebSocket, SSE, Socket.IO e MCP.
- OAuth2 completo, certificados/client certificates e proxy configurável.
- Git integrado, sync, colaboração, contas e nuvem.
- Mock server, OpenAPI/Swagger/HAR/WSDL completos.
- Plugins de terceiros, IA e temas avançados.
- Insomnia v4/v5 como formato de importação nativo.
- Execução de scripts `pm.*` do Postman.
- Streaming de resposta ou de upload.

## Critérios de aceite

Legenda: **Entregue** = há implementação e teste automatizado ou validação manual registrada; **Parcial** = o fluxo principal funciona, mas falta uma parte observável; **Fora** = não pertence ao MVP.

| Critério | Status | Evidência/observação |
|---|---|---|
| AC-01 — primeiro uso sem conta | Parcial | Coleção inicial e IPC implementados; validação interativa nas plataformas ainda pendente. |
| AC-02 — HTTP 200 | Entregue | `http_success_metadata_assertions_and_scripts`. |
| AC-03 — HTTP 404 | Entregue | `no_tests_is_executed_not_passed_and_404_is_preserved`. |
| AC-04 — erro de rede | Entregue | `connection_error_is_classified_without_panicking`. |
| AC-05 — variável ausente | Entregue | Resolver retorna `Variable` com nome/escopo; UI mostra o erro. |
| AC-06 — precedência | Entregue | `variable_precedence_request_then_folder_then_environment`. |
| AC-07 — herança de auth | Entregue | `auth_inherits_from_folder_and_redacts_secret_values`. |
| AC-08 — segredo não aparece | Entregue | Teste de auth verifica ausência no JSON; logs/exportações redigem. |
| AC-09 — script com timeout | Entregue | `script_timeout_is_enforced` (loop infinito interrompido). |
| AC-10 — teste falhando | Entregue | Status `failed`, esperado/obtido no relatório. |
| AC-11 — 404 sem teste | Entregue | Status `executed`, HTTP 404 visível. |
| AC-12 — runner determinístico | Entregue | `deterministic_hash_is_stable_across_runs`. |
| AC-13 — persistência/reabertura | Entregue | Store lê/grava YAML; testes de importação/projeto reabrem a coleção. |
| AC-14 — rascunho não salvo | Parcial | Autosave, recuperação, órfãos e guardião implementados e testados no núcleo/IPC e em fluxos headless; validação no desktop real pendente. |
| AC-15 — edição externa | Parcial | Hash/geração, recarga assistida, conflito 409 e comparação com o disco; a UI consulta ao focar/ficar visível, sem watcher nativo. Validação desktop pendente. |
| AC-16 — importação com perdas | Entregue | Relatório de contagem/avisos/perdas; scripts Postman não executam. |
| AC-17 — exportação segura | Entregue | Auth/headers/query/variáveis sensíveis literais omitidos; projeto sanitizado; conteúdo da lixeira não é exportado. |
| AC-18 — modo offline | Entregue | Bloqueio antes da conexão e em redirects; testes de offline. |
| AC-19 — migração | Parcial | Inspeção, confirmação, backup, hashes, rollback e recuperação implementados/testados no núcleo, CLI e IPC; validação visual e Windows pendentes. |
| AC-20 — acessibilidade | Entregue | Fluxo principal por teclado; foco visível; árvore com setas/Home/End; diálogos auditados em headless com Tab, Escape e `aria-live`, sem leitor de tela real. |
| AC-21 — resposta grande | Parcial | Limite configurável e prévia binária; sem streaming. |
| AC-22 — CLI consistente | Entregue | Mesmo modelo; `run`/`send` retornam códigos coerentes. |

## Verificação automatizada

```bash
cargo test
```

A suíte do núcleo cobre:

- metadados HTTP, 404 “executado”, erro de conexão;
- precedência de variáveis, auth herdada e redaction de segredo;
- hash determinístico e timeout de script;
- bloqueio offline e redirect externo;
- importação/exportação cURL, Postman, OpenCollection e projeto;
- sanitização de credenciais na exportação;
- ambiente padrão em `execute_single`;
- API do servidor, incluindo validação e cancelamento de envio com ID do cliente;
- rascunhos: roundtrip sem tocar no YAML, gerações, conflitos, isolamento por coleção, órfãos, corrupção/versão futura e segredo resolvido ausente;
- alterações externas: detecção por geração, conflito de recarga e revisão esperada;
- lixeira: bytes/ID, pasta vazia, destino ocupado, hash/symlink, legado, ID duplicado e restauração interrompida;
- migração: inspeção sem escrita, reuso de plano, backup/hashes, falha/interrupção, schema futuro, rollback e bloqueio de edições;
- interface headless (teste, não produto): reabertura de rascunho, guardião, autosave e falha, criação/movimento/rename/remoção externos, comparação e aplicação, YAML inválido, salvar sem recarga, cursor antigo, ID duplicado, manifesto alterado e foco/teclado.

## Validação manual da demonstração

1. Suba `examples/servidor_demo.py`.
2. Rode `reqlivo --collection examples/demo validate`.
3. Rode `run --env dev` e confira 3 requisições, 3 passagens e hash determinístico.
4. Rode `run --env seguro` com `REQLIVO_DEMO_TOKEN` definido e confira que o valor não aparece.
5. Abra `npm run desktop:dev -- -- --collection examples/demo` e execute (roteiro pendente de validação interativa do desktop):
   - abrir uma requisição;
   - enviar e ver status/tempo/tamanho;
   - abrir testes e logs;
   - abrir ambientes e conferir o badge do segredo;
   - abrir configurações e o runner;
   - usar **Validar** e conferir “Coleção válida”;
   - cancelar um envio lento (por exemplo, alterar a URL para `{{baseUrl}}/delay`);
   - navegar pela árvore apenas com teclado.

## Próximos passos sugeridos

1. Executar a matriz manual de rascunhos, alterações externas, lixeira e migração no desktop Linux e Windows.
2. Medir reconciliação com 1.000 requisições e decidir se um watcher nativo é necessário; hoje a UI consulta ao focar/ficar visível.
3. Adicionar streaming/limite progressivo para respostas grandes (AC-21).
4. Ampliar importação (OpenAPI/HAR) e tradução de scripts Postman.
5. Adicionar OAuth2, certificados e proxy como evolução pós-MVP.
6. Publicar a 0.3.0 somente após CI Linux/Windows, instaladores, checksums e smoke aprovados.
