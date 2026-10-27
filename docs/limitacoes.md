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
- A UI opera por IPC local Tauri. Não existe servidor HTTP da interface. O harness de validação em `examples/ui_harness.rs` é exclusivo de teste, não é distribuído e só abre listener quando executado explicitamente.
- Não há criptografia em repouso para `.reqlivo/`; o diretório deve ser protegido pelo sistema operacional e ignorado pelo Git.
- A exportação de projeto é sanitizada e reescrita; não é um espelho byte a byte. Para preservar comentários e formatação, use o diretório original.
- Arquivos extras do projeto (README, notas) são copiados sem inspeção de segredos; apenas os campos modelados são sanitizados.

## Persistência e edição

- Não há watcher nativo de arquivos. A UI consulta manifesto, pastas e requisições enquanto está visível e ao receber foco, com reconciliação por hash/geração; uma alteração pode demorar até o próximo ciclo/foco.
- Rascunhos são protegidos até 2 s após a última edição (limite de 5 s em digitação contínua). Encerramento abrupto antes da gravação pode perder a última tecla; a UI não promete o contrário.
- Rascunhos são texto local sem criptografia, podem conter credenciais literais digitadas e usam `0600` no Unix. Segredos resolvidos de `provider: env` não são gravados, mas o arquivo deve ser protegido pelo sistema operacional e ignorado pelo Git.
- Conflitos de revisão são verificados na escrita, sob lock cooperativo em `.reqlivo/lock`. Um lock obsoleto é recuperado por idade (60 s) ou PID morto no Linux. Editores externos que ignoram o lock continuam sujeitos à janela residual entre verificação e substituição; não há transação com eles.
- A lixeira restaura requisições e pastas vazias, mas não faz exclusão permanente nem expiração automática. Pastas não vazias continuam exigindo mover os itens primeiro; exclusão recursiva está fora desta release.
- Rollback de migração é bloqueado se algum arquivo tocado foi editado depois da aplicação. Não há botão de forçar nesta release; a recuperação manual preserva o backup. Backups legados da v0.2.0 não têm rollback automático seguro.
- Não há merge automático entre rascunho e disco. Aplicar uma edição sobre nova base exige comparação e confirmação explícitas.
- YAML inválido não substitui o último snapshot válido; o erro permanece visível até a correção do arquivo. YAML inválido na primeira abertura ainda é listado como arquivo não suportado.
- O snapshot usado por um envio/runner é imutável; alterações externas passam a valer na próxima execução. A comparação de conflito lê o disco explicitamente, sem alterar esse snapshot.

## Interface e acessibilidade

- A interface é uma SPA vanilla, sem framework, com foco no fluxo principal.
- A árvore, os diálogos e os atalhos principais são acessíveis por teclado. O harness headless audita foco inicial, Tab, Escape e anúncios `aria-live`; a matriz D-01 a D-11, em WebView real no Linux, cobre foco inicial, Tab e Escape. Não há auditoria com leitor de tela real, conformidade WCAG completa nem cobertura nativa de seleção de pasta, importação/exportação por diálogo e fechamento com rascunho.
- Tema escuro fixo; temas avançados estão fora do MVP.
- Sem i18n: textos em português.
- Respostas grandes são limitadas/truncadas; não há streaming visual incremental.
- Upload multipart lê arquivos locais no momento da execução, sem barra de progresso.

## Runner e relatórios

- O runner executa sequencialmente; não há paralelismo configurável.
- Filtros são por tags, IDs de requisição e uma pasta; não há expressão booleana.
- O histórico mantém as últimas execuções em `.reqlivo/history/runs.jsonl` e os 30 relatórios mais recentes em memória no aplicativo.
- Cancelamento interrompe transporte/scripts em andamento, mas não desfaz efeitos colaterais já enviados ao servidor de destino.
- O envio interativo só pode ser cancelado com o ID gerado pelo cliente; a UI já faz isso.

## Fora do MVP

- Git integrado, sync, colaboração, contas, nuvem e telemetria.
- Mock server, plugins de terceiros, IA e temas avançados.
- OAuth2 completo, certificados de cliente e proxy.
- OpenAPI completo e geração de código.
- Execução de scripts de terceiros importados.

## Decisões técnicas relevantes

- **Workspace Rust**: biblioteca/CLI e aplicativo Tauri 2. HTML/CSS/JavaScript são empacotados na WebView. O roteador Axum é usado em memória para preservar os contratos; não abre sockets. Linux usa WebKitGTK 4.1 e Windows usa WebView2.
- **OpenCollection** como fonte de verdade, com `extensions.reqlivo` para IDs, settings, segredos e metadados do Reqlivo.
- **QuickJS** (`rquickjs`) em vez de Node: sandbox menor, timeout por interrupção e superfície controlada.
- **Sem conta** por princípio: toda a operação essencial é local e não depende de rede externa.
- **IPC local**: a WebView não faz chamadas diretas às APIs de destino.

## Distribuição desktop

- Linux x86_64 e Windows x86_64 são os alvos iniciais. Sem macOS ou ARM.
- Instaladores não possuem assinatura de código; atualização automática não está implementada.
- Build automatizado e a matriz Linux em WebView real não substituem a validação manual no Windows, a inspeção visual do instalador nem a auditoria de acessibilidade. Consulte [desktop.md](desktop.md).

## Próximos marcos sugeridos

1. Validação manual da 0.3.0 no Windows e dos itens nativos restantes no Linux; a matriz automatizada Linux em WebView real já cobriu 11 fluxos. Faltam instalação limpa e auditoria com leitor de tela/WCAG.
2. Benchmark de reconciliação com 1.000 requisições executado (~12–13 ms por `changes`); decidir se um watcher nativo é necessário. O polling atual atende ao prazo de três segundos nesta medição.
3. Streaming de resposta e upload.
4. OAuth2, certificados e proxy.
5. Expansão de importação/interoperabilidade e tradução de scripts.
