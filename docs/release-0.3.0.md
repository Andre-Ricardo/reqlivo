---
title: Notas da versão 0.3.0
---

# Reqlivo 0.3.0 — persistência e edição recuperável

**Estado: publicada em 2026-10-27 na tag `v0.3.0`.** A [release no GitHub](https://github.com/Andre-Ricardo/reqlivo/releases/tag/v0.3.0) contém `.deb`, `.AppImage`, instalador NSIS `.exe` e `SHA256SUMS`. O workflow `Desktop e releases` ([execução](https://github.com/Andre-Ricardo/reqlivo/actions/runs/40544605429)) compilou e testou Linux e Windows, executou o smoke de abertura do AppImage e do executável Windows e só então publicou os artefatos. A matriz completa está em [resultado-validacao.md](proxima-release/resultado-validacao.md); a matriz automatizada Linux em WebView real passou 11/11, enquanto a matriz manual no Windows, a instalação limpa e a auditoria com leitor de tela continuam pendentes.

## Artefatos publicados

| Arquivo | Plataforma | Tamanho |
|---|---|---:|
| `Reqlivo_0.3.0_amd64.deb` | Debian/Ubuntu x86_64 | 7.260.776 bytes |
| `Reqlivo_0.3.0_amd64.AppImage` | Linux x86_64 | 85.178.872 bytes |
| `Reqlivo_0.3.0_x64-setup.exe` | Windows x86_64 (NSIS) | 5.257.446 bytes |
| `SHA256SUMS` | checksums SHA-256 dos três instaladores | 279 bytes |

Os artefatos foram gerados pelo workflow a partir da tag `v0.3.0`. O smoke remoto abriu o AppImage em Xvfb/Openbox e confirmou a janela do executável Windows; a instalação manual em máquinas limpas ainda não foi executada.

## Funcionalidades entregues

- **Rascunhos locais** em `.reqlivo/drafts/`, protegidos até 2 s após a última edição (limite de 5 s durante digitação contínua), com geração, revisão base e estados visíveis. O YAML canônico só muda ao salvar.
- **Recuperação na reabertura** para rascunho atual, divergente, órfão, corrompido ou de versão futura. Rascunhos órfãos podem gerar uma nova requisição com ID novo, sem reutilizar identidade silenciosamente.
- **Guardião de navegação** para troca de requisição/coleção, recarga, criação e fechamento: salvar, manter rascunho, descartar ou cancelar.
- **Detecção de alterações externas** por hash/geração, com consulta enquanto a janela está visível e ao receber foco. A interface avisa, preserva o rascunho e exige decisão antes de recarregar. Criação, remoção, movimento e rename atômico externos convergem para a árvore.
- **Comparação com o disco real**: o diálogo de conflito lê a versão atual do arquivo, sem usar o snapshot em memória do runner; “aplicar” realinha a revisão antes de gravar.
- **YAML inválido não substitui o último snapshot válido**: o caminho e o erro são exibidos, o editor continua utilizável e a reconciliação volta a funcionar após a correção.
- **Conflitos estáveis** de revisão e geração retornam HTTP 409 e não sobrescrevem trabalho (`revision_conflict`, `generation_conflict`, `draft_generation` e correlatos). Uma falha de manifesto não deixa alterações parciais em memória.
- **Lixeira recuperável** com entrada versionada, payload original, ID lógico, origem, pai, data e hash. A restauração preserva bytes/ID, permite novo destino e rejeita destino ocupado, ID ativo, hash divergente, symlink e entrada legada ilegível.
- **Migração com inspeção somente-leitura**, plano persistido, confirmação explícita, backup completo, hashes anteriores/posteriores, journal de estados e validação do resultado.
- **Rollback verificável** que restaura os bytes exatos dos arquivos tocados, mantém arquivos independentes intactos e bloqueia a operação quando houve edição posterior.
- **Recuperação após rollback** no desktop: leituras continuam, escritas/execuções ficam bloqueadas e a coleção só volta ao normal após nova inspeção e confirmação.
- **Coordenação cooperativa** entre processos por `.reqlivo/lock`, com recuperação de lock obsoleto por idade (60 s) ou PID morto no Linux.
- **Proteção de segredos**: segredo resolvido de `provider: env` não entra no rascunho; lixeira e estado interno não entram na exportação de projeto.

## CLI

```bash
reqlivo --collection minha-api migration inspect
reqlivo --collection minha-api migration apply <plan-id> --revision <revisao>
reqlivo --collection minha-api migration rollback <plan-id> --revision <revisao>
reqlivo --collection minha-api migration list
```

Comandos comuns (`run`, `send`, `validate`, edição) que encontram migração pendente retornam erro acionável e **não** alteram arquivos. A inspeção também não escreve no projeto; ela apenas prepara o plano em `.reqlivo/backups/`.

## Compatibilidade e atualização

- A versão do aplicativo passa de `0.2.0` para `0.3.0` em `Cargo.toml`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `package.json`, `package-lock.json` e nas entradas dos pacotes próprios em `Cargo.lock`.
- O formato canônico continua **OpenCollection 1.0.0**; rascunhos, lixeira e backups têm versões internas próprias e ficam em `.reqlivo/`, fora da exportação.
- Coleções `0.2.0` com `schemaVersion` anterior exigem inspeção/confirmação. O `init` de novas coleções continua sem confirmação de migração.
- Entradas de lixeira e backups antigos aparecem como legados; quando não há metadados suficientes, a recuperação manual é indicada em vez de adivinhar origem ou reescrever bytes.
- A atualização não altera dependências indiscriminadamente.

## Validação executada

Validação local (Linux x86_64) e remota (Linux/Windows):

- `cargo fmt --all -- --check`, `cargo test --locked` (107 testes), `cargo test --workspace --locked` (107 testes), `cargo check --manifest-path src-tauri/Cargo.toml`, `node --check src/assets/app.js`, `node --check scripts/ui-acceptance.mjs`, `node --check scripts/desktop-acceptance.mjs`, `python3 -m py_compile scripts/x11-dialog-ok.py` e `scripts/check-version.py` passaram.
- O workflow remoto `Desktop e releases` passou em `ubuntu-22.04` e `windows-latest` no push para `main` ([pré-validação](https://github.com/Andre-Ricardo/reqlivo/actions/runs/40542757635)) e novamente na tag `v0.3.0` ([publicação](https://github.com/Andre-Ricardo/reqlivo/actions/runs/40544605429)), incluindo o job que publicou os instaladores e o `SHA256SUMS`.
- `examples/demo validate` passou; a demonstração CLI passou nos ambientes `dev` e `seguro` com servidor sintético em loopback.
- O cenário CLI de inspeção, aplicação, bloqueio após rollback, nova inspeção e reaplicação passou em cópia temporária da demonstração.
- Onze fluxos de interface passaram em Chrome headless com dados sintéticos: reabertura de rascunho, guardião, falha/prazo de autosave, criação/movimento/rename/remoção externos, comparação de conflito, YAML inválido, salvar sem recarga, cursor antigo, ID duplicado, manifesto alterado durante edição e foco/teclado. O harness é exclusivo de teste e não cria listener no produto.
- Uma matriz adicional de 11 casos (D-01 a D-11) passou no **aplicativo desktop real** (Tauri/WebKitGTK) no Linux, via `tauri-driver` e WebKitWebDriver, com binário de release, Xvfb e servidor sintético. Ela cobre abertura/envio, autosave e reinício, guardião, alteração externa/comparação, YAML inválido, lixeira, manifesto, cursor antigo, foco/Tab/Escape, cancelamento e migração/rollback.
- O benchmark de 1.000 requisições foi executado em três rodadas: `changes` ~12–13 ms, abertura/recarga ~137–143 ms, snapshot/tree <1 ms e RSS máximo ~9 MB.

Evidências locais (fora deste repositório): [checks](../../evidencias/reqlivo-v0.3.0/checks.txt), [UI headless](../../evidencias/reqlivo-v0.3.0/ui/ui-acceptance.txt), [capturas](../../evidencias/reqlivo-v0.3.0/ui/), [desktop real](../../evidencias/reqlivo-v0.3.0/desktop/desktop-acceptance.txt), [capturas desktop](../../evidencias/reqlivo-v0.3.0/desktop/), [benchmark](../../evidencias/reqlivo-v0.3.0/bench-reconciliacao.md), [demonstração CLI](../../evidencias/reqlivo-v0.3.0/demo-cli.txt), [migração/rollback](../../evidencias/reqlivo-v0.3.0/migracao-cli.txt) e [release](../../evidencias/reqlivo-v0.3.0/release.txt).

## Limitações restantes

- A matriz **manual** de desktop no Windows, a auditoria com leitor de tela e os itens nativos não automatizados **não foram executados**. No Linux, a matriz automatizada em WebView real passou 11/11; ela não substitui a validação manual de acessibilidade, instalador ou Windows.
- Os builds Tauri finais, os três instaladores e o `SHA256SUMS` foram gerados e o smoke automatizado passou em Linux e Windows. A instalação manual dos artefatos publicados em máquinas limpas ainda não foi executada.
- A falha de disco durante o autosave foi exercitada como falha de transporte no harness, não como indisponibilidade física do disco.
- A detecção externa é por consulta ao focar/ficar visível, não por watcher nativo contínuo. O benchmark de 1.000 requisições mediu `changes` em ~12–13 ms; o prazo de três segundos ainda depende do ciclo de consulta, do hardware e do tamanho da coleção.
- Rascunhos são texto local sem criptografia e podem conter credenciais literais digitadas pelo usuário.
- Não há merge automático, exclusão permanente da lixeira, exclusão recursiva de pasta nem rollback forçado.
- Permanecem fora do escopo: streaming/upload progressivo, OAuth2/certificados/proxy, novos protocolos/importadores, tradução de scripts Postman, Git/nuvem/contas/plugins/temas/i18n/updater/assinatura e reescrita de framework da UI.
