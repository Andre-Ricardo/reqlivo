---
title: Limitações
---

# Limitações e decisões abertas

O Reqlivo está em nível **MVP funcional**. As limitações abaixo são intencionais nesta fase ou dependem de trabalho futuro; nenhuma delas deve ser interpretada como comportamento completo de Insomnia/Bruno.

## Protocolos e formatos

- Somente HTTP. gRPC, WebSocket, SSE, Socket.IO e MCP estão fora do MVP.
- OpenAPI, Swagger, HAR e WSDL não são importados.
- Insomnia v4/v5 não é um formato nativo de importação.
- Postman é suportado como subconjunto v2.1: scripts `pm.*` são preservados como dados, não executados; variáveis e recursos sem equivalente geram avisos/perdas.
- A exportação OpenCollection é “bundled” e destinada a importação; não substitui o diretório de projeto.
- Campos desconhecidos são preservados quando o parser os mantém em `extra`; uma edição pela UI pode não reproduzir comentários/ordem originais do YAML.

## Scripts e testes

- Scripts são síncronos. Promises, `async/await`, timers e módulos npm não são drenados/importados.
- O sandbox expõe apenas `rq.*`, `test`/`expect`, `rq.assert` e `console.*`.
- O timeout usa interrupção do runtime QuickJS. Um script que bloqueie o host fora do runtime (por exemplo, I/O bloqueante nativo) não é suportado.
- O relatório de teste é intencionalmente simples: nome, status, mensagem, esperado/obtido e origem.
- O hash determinístico cobre IDs, status, códigos de erro e nomes/status dos testes; não é uma reprodução byte a byte da resposta.

## Segurança e segredos

- Apenas `provider: env` está implementado. `system` e `file` existem no modelo, mas não resolvem.
- Valores literais em auth/headers/query são permitidos no projeto, mas a UI avisa e a exportação padrão os omite. Não há scanner de segredos em corpos livres.
- A redaction cobre nomes/valores conhecidos, variáveis secretas e padrões de logs/relatórios. Conteúdo arbitrário em corpo de resposta ou script só é mascarado quando contém um valor secreto conhecido.
- O servidor web escuta apenas em loopback e não tem autenticação. Não exponha a porta em redes públicas.
- Não há criptografia em repouso para `.reqlivo/`; o diretório deve ser protegido pelo sistema operacional e ignorado pelo Git.
- A exportação de projeto é sanitizada e reescrita; não é um espelho byte a byte. Para preservar comentários e formatação, use o diretório original.
- Arquivos extras do projeto (README, notas) são copiados sem inspeção de segredos; apenas os campos modelados são sanitizados.

## Persistência e edição

- Não há watcher de arquivos. A UI recarrega sob demanda e detecta conflito de revisão ao salvar; não pergunta automaticamente a cada mudança externa.
- Não há autosave de rascunho. Fechar a aba usa o aviso nativo do navegador; recarregar a página descarta o rascunho não salvo.
- A lixeira é um diretório em `.reqlivo/trash/`; não há UI de restauração.
- Migrações fazem backup e aviso, mas o rollback é manual.
- A edição simultânea por múltiplos processos não é coordenada além da revisão no momento do save.

## Interface e acessibilidade

- A interface é uma SPA vanilla, sem framework, com foco no fluxo principal.
- A árvore, os diálogos e os atalhos principais são acessíveis por teclado; não há auditoria com leitor de tela real nem conformidade WCAG completa.
- Tema escuro fixo; temas avançados estão fora do MVP.
- Sem i18n: textos em português.
- Respostas grandes são limitadas/truncadas; não há streaming visual incremental.
- Upload multipart lê arquivos locais no momento da execução, sem barra de progresso.

## Runner e relatórios

- O runner executa sequencialmente; não há paralelismo configurável.
- Filtros são por tags, IDs de requisição e uma pasta; não há expressão booleana.
- O histórico mantém as últimas execuções em `.reqlivo/history/runs.jsonl` e os 30 relatórios mais recentes em memória no servidor.
- Cancelamento interrompe transporte/scripts em andamento, mas não desfaz efeitos colaterais já enviados ao servidor de destino.
- O envio interativo só pode ser cancelado com o ID gerado pelo cliente; a UI já faz isso.

## Fora do MVP

- Git integrado, sync, colaboração, contas, nuvem e telemetria.
- Mock server, plugins de terceiros, IA e temas avançados.
- OAuth2 completo, certificados de cliente e proxy.
- OpenAPI completo e geração de código.
- Execução de scripts de terceiros importados.

## Decisões técnicas relevantes

- **Um crate Rust** com biblioteca + binário. A UI é embutida via `include_str!` e servida por Axum; não há Tauri nem dependência de Node/WebKit para rodar.
- **OpenCollection** como fonte de verdade, com `extensions.reqlivo` para IDs, settings, segredos e metadados do Reqlivo.
- **QuickJS** (`rquickjs`) em vez de Node: sandbox menor, timeout por interrupção e superfície controlada.
- **Sem conta** por princípio: toda a operação essencial é local e não depende de rede externa.
- **Loopback apenas**: reduz superfície de ataque e evita transformar o MVP em servidor remoto sem autenticação.

## Próximos marcos sugeridos

1. Draft/autosave e modal de descarte.
2. Watcher de arquivos e recarga assistida.
3. Rollback de migração e UI de lixeira.
4. Streaming de resposta e upload.
5. OAuth2, certificados e proxy.
6. Expansão de importação/interoperabilidade e tradução de scripts.
