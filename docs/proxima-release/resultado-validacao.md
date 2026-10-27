---
title: Resultado de validação da 0.3.0
---

# Resultado de validação — Reqlivo 0.3.0

**Data da execução:** 2026-10-27 UTC. **Plataformas:** Linux x86_64 (Ubuntu 26.04.1, kernel 7.0.0-38-generic, AMD Ryzen 7 5825U, Rust/Cargo 1.97.1) local; workflow remoto em `ubuntu-22.04` e `windows-latest`. **Base:** checks locais no commit `949ef43`, com conteúdo idêntico ao commit `bbc4822` publicado na `main` e na tag `v0.3.0`. **Versão verificada:** `0.3.0`.

**Conclusão:** a automação de núcleo, CLI, IPC e API passou; 11 fluxos de interface foram exercitados em Chrome headless; uma matriz adicional de 11 fluxos foi executada no **aplicativo desktop real** (Tauri/WebKitGTK, WebDriver) no Linux; o benchmark de reconciliação com 1.000 requisições foi medido; e o workflow remoto compilou/testou Linux e Windows, executou o smoke de abertura e publicou os três instaladores com `SHA256SUMS`. A [release 0.3.0](https://github.com/Andre-Ricardo/reqlivo/releases/tag/v0.3.0) está publicada. A matriz **manual** de desktop no Windows, a auditoria com leitor de tela, a instalação manual em máquinas limpas e os itens nativos não automatizados (seleção de pasta, importação/exportação por diálogo e fechamento com rascunho) **não foram executados**. Este arquivo registra apenas o que foi realmente executado.

## Evidências

| Evidência | Conteúdo |
|---|---|
| [checks.txt](../../../evidencias/reqlivo-v0.3.0/checks.txt) | Versões, formatação, 107 testes em duas invocações, verificador de versão, `validate`, check Tauri, `node --check`, build do harness e 11/11 fluxos de UI. |
| [release v0.3.0](https://github.com/Andre-Ricardo/reqlivo/releases/tag/v0.3.0) | `.deb`, `.AppImage`, `.exe` e `SHA256SUMS` publicados; release verificada com `gh release view`. |
| [workflow de publicação](https://github.com/Andre-Ricardo/reqlivo/actions/runs/40544605429) | Builds Linux/Windows, smoke de abertura e job `Publicar release` aprovados na tag `v0.3.0`. |
| [pré-validação em main](https://github.com/Andre-Ricardo/reqlivo/actions/runs/40542757635) | Builds e smoke Linux/Windows aprovados antes da tag. |
| [ui-acceptance.txt](../../../evidencias/reqlivo-v0.3.0/ui/ui-acceptance.txt) e [capturas](../../../evidencias/reqlivo-v0.3.0/ui/) | Chrome headless: R-02, R-05, R-09, E-01, E-03, E-05, E-06, E-07, E-08, E-09 e G-05, sem falhas. |
| [desktop-acceptance.txt](../../../evidencias/reqlivo-v0.3.0/desktop/desktop-acceptance.txt), [JSON](../../../evidencias/reqlivo-v0.3.0/desktop/desktop-acceptance.json) e [capturas](../../../evidencias/reqlivo-v0.3.0/desktop/) | Aplicativo desktop real (Tauri/WebKitGTK) via `tauri-driver`: 11/11 casos (D-01 a D-11), sem falhas; binário de release com SHA-256 `a34ded160203dcd5a27cfadc57f19e5ee491b80e38eecff652f414cb52e98919`. |
| [bench-reconciliacao.md](../../../evidencias/reqlivo-v0.3.0/bench-reconciliacao.md), [run 1](../../../evidencias/reqlivo-v0.3.0/bench-reconciliacao-run-1.json), [run 2](../../../evidencias/reqlivo-v0.3.0/bench-reconciliacao-run-2.json) e [run 3](../../../evidencias/reqlivo-v0.3.0/bench-reconciliacao-run-3.json) | 1.000 requisições em três execuções: `open` ~137–143 ms, `changes` ~12–13 ms por chamada, snapshot/tree <1 ms e RSS máximo ~9 MB. |
| [demo-cli.txt](../../../evidencias/reqlivo-v0.3.0/demo-cli.txt) | Demonstração CLI nos ambientes `dev` e `seguro` com servidor sintético em loopback. |
| [migracao-cli.txt](../../../evidencias/reqlivo-v0.3.0/migracao-cli.txt) | Inspeção, aplicação, bloqueio após rollback, nova inspeção e reaplicação em cópia temporária. |
| [release.txt](../../../evidencias/reqlivo-v0.3.0/release.txt) | Tag, commit, URLs dos workflows, assets, tamanhos e checksums SHA-256. |

Os links para `evidencias/` apontam para o diretório de trabalho local, fora deste repositório; os links de release e workflows são públicos.

## Checks do projeto

| Check | Resultado | Status |
|---|---|---|
| `cargo fmt --all -- --check` | sem diferenças | aprovado |
| `cargo test --locked` | 107 testes, 5 suítes | aprovado |
| `cargo test --workspace --locked` | 107 testes, 6 suítes | aprovado |
| `cargo check --manifest-path src-tauri/Cargo.toml` | compilou | aprovado |
| `node --check src/assets/app.js` | sem erro de sintaxe | aprovado |
| `node --check scripts/ui-acceptance.mjs` | sem erro de sintaxe | aprovado |
| `python3 scripts/check-version.py` | versão consistente `0.3.0` | aprovado |
| `reqlivo --collection examples/demo validate` | coleção válida, 3 requisições | aprovado |
| `cargo build --example ui_harness` | harness de teste compilado | aprovado |
| `node scripts/ui-acceptance.mjs` | 11/11 fluxos headless | aprovado |
| `node --check scripts/desktop-acceptance.mjs` | sem erro de sintaxe | aprovado |
| `python3 -m py_compile scripts/x11-dialog-ok.py` | sem erro de sintaxe | aprovado |
| `node scripts/desktop-acceptance.mjs` (WebView real) | 11/11 casos D-01 a D-11 | aprovado |
| `cargo run --release --example bench_reconciliation -- 1000` | 3 execuções independentes concluídas | aprovado |
| Demonstração CLI `dev` e `seguro` | 3/3 passaram, hash determinístico | aprovado |
| Migração CLI em cópia temporária | aplicou, bloqueou após rollback e reaplicou | aprovado |
| Workflow `Desktop e releases` (push `main`) | Linux e Windows compilaram, testaram e abriram o aplicativo | aprovado |
| Workflow `Desktop e releases` (tag `v0.3.0`) | builds e smoke passaram; release publicou 3 instaladores e `SHA256SUMS` | aprovado |

Os testes automatizados indicados abaixo estão em `src/drafts.rs`, `src/trash.rs`, `src/migration.rs` e `tests/integration.rs`; todos fazem parte das 107 execuções acima. O harness de interface e o driver de desktop são exclusivos de teste: o produto continua usando IPC sem listener. Os fluxos headless e a matriz em WebView real não substituem a validação manual de acessibilidade, instalador ou Windows.

## Matriz desktop automatizada — Linux/WebView real

A matriz foi executada no binário de release `target/release/reqlivo-desktop` (SHA-256 `a34ded160203dcd5a27cfadc57f19e5ee491b80e38eecff652f414cb52e98919`), sem alterações de produto. O ambiente usou Ubuntu 26.04.1, Xvfb `:99` (1600×1000×24, sem gerenciador de janelas), `tauri-driver` 2.0.6 e WebKitWebDriver 2.52.6. As coleções foram cópias descartáveis de `examples/demo`, com servidor HTTP sintético em loopback. O script inicia e encerra o driver e o servidor, e usa X11/XTest apenas para clicar no “OK” dos diálogos nativos GTK; isso é ferramenta de teste, não uma auditoria de acessibilidade.

| ID | Caso executado na WebView real | Resultado |
|---|---|---|
| D-01 | Abrir a coleção e enviar HTTP 200 e 404 | aprovado |
| D-02 | Autosave, reinício do processo e recuperação do rascunho, com YAML canônico intacto | aprovado |
| D-03 | Guardião de navegação: Cancelar, Manter, Salvar e Descartar com confirmação nativa | aprovado |
| D-04 | Alteração externa: comparação, manter, aplicar e usar disco | aprovado |
| D-05 | YAML inválido, preservação do último snapshot e recuperação após correção | aprovado |
| D-06 | Excluir para a lixeira com confirmação nativa e restaurar os mesmos bytes no caminho original | aprovado |
| D-07 | Manifesto alterado externamente durante a edição de ambiente | aprovado |
| D-08 | Cursor de geração de outra coleção (simulado), sem recarga indevida | aprovado |
| D-09 | Foco, Tab e Escape na WebView real | aprovado |
| D-10 | Cancelar uma execução HTTP lenta e observar a resposta `cancelled` | aprovado |
| D-11 | Migração: confirmação nativa, aplicação, backup e rollback | aprovado |

Resultado final: **11/11**, sem falhas; tempos entre 1,8 s e 10,0 s por caso. A reprodução exige o binário de release, `tauri-driver`, `WebKitWebDriver`, Xvfb e `xwininfo`. Comando usado:

```bash
WEBKIT_WEBDRIVER=/caminho/WebKitWebDriver \
DESKTOP_WEBKIT_VERSION=2.52.6 \
node scripts/desktop-acceptance.mjs
```

O relatório completo, o JSON e as capturas estão em [desktop/](../../../evidencias/reqlivo-v0.3.0/desktop/). Cobertura não automatizada: seleção/cancelamento de pasta pelo diálogo nativo, importação/exportação por diálogo, fechamento da janela com rascunho, troca real entre coleções, leitor de tela/WCAG, matriz manual no Windows e instalação em máquina limpa.

## Grupo R — rascunhos

| ID | Automação executada | Status da automação | Validação desktop |
|---|---|---|---|
| R-01 | `draft_does_not_touch_canonical_file_and_roundtrips`, `drafts_api_roundtrips_without_touching_canonical_yaml` — YAML byte a byte intacto; rascunho com a edição. | aprovado | D-02: YAML canônico byte a byte intacto na WebView real |
| R-02 | `draft_survives_process_restart_without_sending_anything` (reabertura sem enviar nada) e UI R-02: rascunho restaurado pela lista, YAML canônico intacto. | aprovado | D-02 (WebView real; reinício do processo) |
| R-03 | `stale_generation_is_rejected_and_saved_generation_is_removed`, `discarded_generation_is_not_resurrected_by_a_late_write` — somente a geração salva é removida; geração posterior sobrevive. | aprovado | não executado |
| R-04 | `failed_canonical_save_keeps_the_draft_and_both_versions` (conflito de revisão preserva rascunho e disco) e UI R-09 (falha de transporte do rascunho simulada no harness). Falha de I/O de disco real não foi injetada. | aprovado (revisão; I/O simulado) | headless; desktop não executado |
| R-05 | UI R-05: Cancelar conserva contexto; Manter protege o rascunho; Salvar grava antes de navegar; Descartar remove o rascunho sem tocar no YAML. Atalho Escape coberto por G-05. | aprovado | D-03 (WebView real; descarte com confirmação nativa) |
| R-06 | `drafts_are_isolated_by_collection_and_orphans_remain` — duas coleções não misturam rascunhos. | aprovado | não executado |
| R-07 | `external_change_marks_draft_diverged_without_losing_content`, `corrupt_and_future_records_are_isolated_without_deleting_originals`, `orphan_is_adopted_with_new_id_and_old_draft_is_removed`. | aprovado | não executado |
| R-08 | `draft_never_contains_a_resolved_provider_secret` — valor de `provider: env` ausente; template preservado. | aprovado | não executado |
| R-09 | UI R-09: falha de proteção visível preserva a edição; após recuperação, digitação contínua grava no limite de ~5 s e novamente após a pausa. Tempo real; indisponibilidade de disco simulada no shim. | aprovado (transporte simulado) | headless; desktop não executado |

## Grupo E — alterações externas

| ID | Automação executada | Status da automação | Validação desktop |
|---|---|---|---|
| E-01 | `external_create_move_atomic_rename_and_remove_converge` e UI E-01: criação, rename atômico, movimento e remoção convergem sem perder a seleção. | aprovado | headless; desktop não executado |
| E-02 | Edição externa + rascunho divergente preservado + salvamento com revisão antiga rejeitado (409). | aprovado | não executado |
| E-03 | UI E-03: comparação lê a versão atual do disco (`GET /api/requests/{id}/disk`), “manter” não sobrescreve, “aplicar” realinha a revisão e grava, “usar disco” descarta o rascunho. | aprovado | D-04 (WebView real) |
| E-04 | `manifest_save_rejects_stale_expected_revision_and_preserves_form`; o estado em memória só muda após escrita verificada. | aprovado | não executado |
| E-05 | `invalid_yaml_keeps_last_valid_snapshot_and_recovers_after_fix` e UI E-05: YAML inválido preserva o último snapshot, mostra o caminho e volta a salvar após correção. | aprovado | D-05 (WebView real) |
| E-06 | `own_writes_and_draft_saves_do_not_request_reconciliation` e UI E-06: salvar pela UI não dispara recarga nem falso evento externo. | aprovado | headless; desktop não executado |
| E-07 | `stale_generation_from_another_collection_does_not_reconcile` e UI E-07: cursor de outra coleção apenas realinha a geração, sem recarregar nem mudar a árvore. Troca real entre coleções no desktop não executada. | aprovado (cursor antigo) | D-08 (cursor simulado na WebView real; troca real não) |
| E-08 | `rename_keeps_id_and_duplicate_id_is_rejected_without_losing_snapshot` e UI E-08: renomear mantém ID/seleção; ID duplicado mantém a árvore anterior e o editor utilizável. | aprovado | headless; desktop não executado |
| E-09 | `environment_save_conflicts_when_cli_changed_manifest_and_preserves_state` e UI E-09: manifesto alterado externamente, formulário preservado, 409 sem vazamento e salvamento explícito após recarregar. | aprovado | D-07 (WebView real) |
| E-10 | `runner_uses_the_initial_snapshot_while_the_file_changes`: execução em voo mantém a URL inicial; a seguinte usa o disco novo. | aprovado | não executado |

## Grupo L — lixeira

| ID | Automação executada | Status da automação | Validação desktop |
|---|---|---|---|
| L-01 | `request_is_restored_with_same_bytes_and_id`, `trash_api_deletes_reports_and_restores_with_content_revision`. | aprovado | D-06 (WebView real; exclusão com confirmação nativa e restauração byte a byte) |
| L-02 | `empty_folder_is_restored_and_non_empty_folder_is_rejected`, `empty_folder_roundtrips_with_same_id_and_bytes`. | aprovado | não executado |
| L-03 | `occupied_destination_does_not_overwrite`. | aprovado | não executado |
| L-04 | `duplicate_active_id_blocks_restore_and_keeps_entry`. | aprovado | não executado |
| L-05 | `interrupted_restore_is_reconciled_without_losing_bytes`. | aprovado | não executado |
| L-06 | `legacy_entry_can_be_restored_with_a_free_name`. | aprovado | não executado |
| L-07 | `tampered_and_symlinked_payloads_are_rejected`. | aprovado | não executado |
| L-08 | `project_export_and_trash_listing_do_not_leak_deleted_content` — listagem, log e projeto exportado sem o marcador; `.reqlivo/` ausente. | aprovado | não executado |
| L-09 | `second_restore_of_same_entry_reports_conflict`. | aprovado | não executado |

## Grupo M — migração

| ID | Automação executada | Status da automação | Validação desktop |
|---|---|---|---|
| M-01 | `inspection_does_not_change_project_and_apply_creates_backup`; inspeção CLI em cópia temporária. | aprovado | D-11 (WebView real; aplicação com confirmação nativa) |
| M-02 | `backup_originals_match_pre_migration_bytes`, `inspection_does_not_change_project_and_apply_creates_backup`; aplicação CLI. | aprovado | D-11 (WebView real; backup após aplicação) |
| M-03 | `interrupted_application_restores_originals_and_marks_failed`, `missing_proposed_bytes_fail_without_partial_success`. | aprovado | não executado |
| M-04 | `rollback_restores_original_bytes_and_blocks_later_edits`, `rollback_keeps_independent_files_and_restores_touched_bytes`, rollback CLI. | aprovado | D-11 (rollback na WebView real; bloqueio de edição posterior não exercitado) |
| M-05 | `rollback_restores_original_bytes_and_blocks_later_edits` — edição posterior bloqueia rollback. | aprovado | não executado |
| M-06 | `future_schema_is_rejected_without_touching_files`, `corrupt_and_legacy_backups_are_listed_without_writes`. | aprovado | não executado |
| M-07 | `migration_api_requires_confirmation_and_blocks_writes_after_rollback`; `validate` CLI falha após rollback e reaplicação funciona. | aprovado | D-11 (confirmação nativa e rollback; bloqueio de escrita posterior não exercitado) |
| M-08 | `validate` CLI retorna `1` com dica acionável; escrita bloqueada por `collection_unavailable`. | aprovado | não executado |
| M-09 | `corrupt_and_legacy_backups_are_listed_without_writes` — backup legado listado sem alteração. | aprovado | não executado |
| M-10 | `changed_file_between_inspection_and_apply_invalidates_plan`, `migration_api_requires_confirmation_and_blocks_writes_after_rollback` (revisão divergente). | aprovado | não executado |

## Critérios globais

| ID | Resultado | Status |
|---|---|---|
| G-01 | Linux local (107 testes, 11 fluxos headless e 11 casos na WebView real) e workflow automatizado Linux/Windows aprovados; matriz manual completa no Windows não executada. | parcial |
| G-02 | 107 testes, demonstração HTTP 200/404 e D-01/D-10 na WebView real; offline, timeout, cancelamento, runner e scripts continuam cobertos. | aprovado |
| G-03 | IPC sem listener em `tests/desktop.rs`, exercitado também nos 11 casos reais; o harness e o driver são ferramentas isoladas, não o produto; offline coberto; recuperação e reconciliação só leem o projeto local. | aprovado |
| G-04 | Rascunho não contém segredo resolvido; redaction/exportação e ausência do conteúdo da lixeira em listagem/log/projeto cobertas. | aprovado |
| G-05 | UI G-05: foco inicial no primeiro botão, Tab entre botões, Escape como Cancelar, `aria-live`/`role` e contexto preservado; D-09 exercitou foco, Tab e Escape na WebView real. Leitor de tela real e conformidade WCAG não executados. | parcial |
| G-06 | Workflow remoto executado na tag `v0.3.0`; `.deb`, `.AppImage`, `.exe` e `SHA256SUMS` publicados e verificados. A instalação manual em máquinas limpas segue pendente. | aprovado |
| G-07 | Guias, limitações e notas atualizados, incluindo rascunho em texto claro, harness headless, YAML inválido e coordenação entre processos; revisão editorial feita nesta execução. | aprovado |

## Pendências pós-publicação

1. Executar a matriz **manual** de desktop no Windows e os itens nativos restantes no Linux (seleção/cancelamento de pasta, importação/exportação por diálogo e fechamento com rascunho). A matriz automatizada Linux em WebView real cobre 11 fluxos, mas não substitui leitor de tela, menu nativo, inspeção visual do instalador nem a plataforma Windows.
2. O benchmark de 1.000 requisições foi executado (três execuções; `changes` ~12–13 ms). Decidir se um watcher nativo é necessário; o polling atual atende ao prazo de três segundos nesta medição.
3. Instalar manualmente `.deb`, `.AppImage` e o NSIS em máquinas limpas; o workflow já abriu o AppImage em Xvfb/Openbox e confirmou a janela do executável Windows, mas não testou a instalação em ambiente limpo.
4. Realizar auditoria com leitor de tela/WCAG; D-09 cobre apenas foco, Tab e Escape.
5. Corrigir qualquer falha encontrada e publicar uma versão de correção com novas evidências, sem converter cenário não executado em aprovação.

A tag `v0.3.0` foi criada em 2026-10-27 e a release foi publicada com os três instaladores e checksums. A matriz automatizada Linux em WebView real e o benchmark foram executados depois, sem retag. A matriz manual no Windows, o leitor de tela, a instalação limpa e os itens nativos restantes continuam sendo lacunas conhecidas, não bloqueios retroativos da publicação.
