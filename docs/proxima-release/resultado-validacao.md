---
title: Resultado de validação da 0.3.0
---

# Resultado de validação — Reqlivo 0.3.0

**Data da execução:** 2026-10-27 UTC. **Plataformas:** Linux x86_64 (kernel 7.0.0-38-generic) local, Rust/Cargo 1.97.1; workflow remoto em `ubuntu-22.04` e `windows-latest`. **Base:** commit `bbc4822` na `main`, tag `v0.3.0`. **Versão verificada:** `0.3.0`.

**Conclusão:** a automação de núcleo, CLI, IPC e API passou; 11 fluxos de interface foram exercitados em Chrome headless; e o workflow remoto compilou/testou Linux e Windows, executou o smoke de abertura e publicou os três instaladores com `SHA256SUMS`. A [release 0.3.0](https://github.com/Andre-Ricardo/reqlivo/releases/tag/v0.3.0) está publicada. A matriz manual de desktop, a auditoria com leitor de tela, o benchmark de 1.000 requisições e a instalação manual em máquinas limpas **não foram executados**. Este arquivo registra apenas o que foi realmente executado.

## Evidências

| Evidência | Conteúdo |
|---|---|
| [checks.txt](../../../evidencias/reqlivo-v0.3.0/checks.txt) | Versões, formatação, 107 testes em duas invocações, verificador de versão, `validate`, check Tauri, `node --check`, build do harness e 11/11 fluxos de UI. |
| [release v0.3.0](https://github.com/Andre-Ricardo/reqlivo/releases/tag/v0.3.0) | `.deb`, `.AppImage`, `.exe` e `SHA256SUMS` publicados; release verificada com `gh release view`. |
| [workflow de publicação](https://github.com/Andre-Ricardo/reqlivo/actions/runs/40544605429) | Builds Linux/Windows, smoke de abertura e job `Publicar release` aprovados na tag `v0.3.0`. |
| [pré-validação em main](https://github.com/Andre-Ricardo/reqlivo/actions/runs/40542757635) | Builds e smoke Linux/Windows aprovados antes da tag. |
| [ui-acceptance.txt](../../../evidencias/reqlivo-v0.3.0/ui/ui-acceptance.txt) e [capturas](../../../evidencias/reqlivo-v0.3.0/ui/) | Chrome headless: R-02, R-05, R-09, E-01, E-03, E-05, E-06, E-07, E-08, E-09 e G-05, sem falhas. |
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
| Demonstração CLI `dev` e `seguro` | 3/3 passaram, hash determinístico | aprovado |
| Migração CLI em cópia temporária | aplicou, bloqueou após rollback e reaplicou | aprovado |
| Workflow `Desktop e releases` (push `main`) | Linux e Windows compilaram, testaram e abriram o aplicativo | aprovado |
| Workflow `Desktop e releases` (tag `v0.3.0`) | builds e smoke passaram; release publicou 3 instaladores e `SHA256SUMS` | aprovado |

Os testes automatizados indicados abaixo estão em `src/drafts.rs`, `src/trash.rs`, `src/migration.rs` e `tests/integration.rs`; todos fazem parte das 107 execuções acima. O harness de interface é exclusivo de teste: o produto continua usando IPC sem listener e os fluxos headless não substituem a validação manual do aplicativo desktop.

## Grupo R — rascunhos

| ID | Automação executada | Status da automação | Validação desktop |
|---|---|---|---|
| R-01 | `draft_does_not_touch_canonical_file_and_roundtrips`, `drafts_api_roundtrips_without_touching_canonical_yaml` — YAML byte a byte intacto; rascunho com a edição. | aprovado | não executado |
| R-02 | `draft_survives_process_restart_without_sending_anything` (reabertura sem enviar nada) e UI R-02: rascunho restaurado pela lista, YAML canônico intacto. | aprovado | headless; desktop não executado |
| R-03 | `stale_generation_is_rejected_and_saved_generation_is_removed`, `discarded_generation_is_not_resurrected_by_a_late_write` — somente a geração salva é removida; geração posterior sobrevive. | aprovado | não executado |
| R-04 | `failed_canonical_save_keeps_the_draft_and_both_versions` (conflito de revisão preserva rascunho e disco) e UI R-09 (falha de transporte do rascunho simulada no harness). Falha de I/O de disco real não foi injetada. | aprovado (revisão; I/O simulado) | headless; desktop não executado |
| R-05 | UI R-05: Cancelar conserva contexto; Manter protege o rascunho; Salvar grava antes de navegar; Descartar remove o rascunho sem tocar no YAML. Atalho Escape coberto por G-05. | aprovado | headless; desktop não executado |
| R-06 | `drafts_are_isolated_by_collection_and_orphans_remain` — duas coleções não misturam rascunhos. | aprovado | não executado |
| R-07 | `external_change_marks_draft_diverged_without_losing_content`, `corrupt_and_future_records_are_isolated_without_deleting_originals`, `orphan_is_adopted_with_new_id_and_old_draft_is_removed`. | aprovado | não executado |
| R-08 | `draft_never_contains_a_resolved_provider_secret` — valor de `provider: env` ausente; template preservado. | aprovado | não executado |
| R-09 | UI R-09: falha de proteção visível preserva a edição; após recuperação, digitação contínua grava no limite de ~5 s e novamente após a pausa. Tempo real; indisponibilidade de disco simulada no shim. | aprovado (transporte simulado) | headless; desktop não executado |

## Grupo E — alterações externas

| ID | Automação executada | Status da automação | Validação desktop |
|---|---|---|---|
| E-01 | `external_create_move_atomic_rename_and_remove_converge` e UI E-01: criação, rename atômico, movimento e remoção convergem sem perder a seleção. | aprovado | headless; desktop não executado |
| E-02 | Edição externa + rascunho divergente preservado + salvamento com revisão antiga rejeitado (409). | aprovado | não executado |
| E-03 | UI E-03: comparação lê a versão atual do disco (`GET /api/requests/{id}/disk`), “manter” não sobrescreve, “aplicar” realinha a revisão e grava, “usar disco” descarta o rascunho. | aprovado | headless; desktop não executado |
| E-04 | `manifest_save_rejects_stale_expected_revision_and_preserves_form`; o estado em memória só muda após escrita verificada. | aprovado | não executado |
| E-05 | `invalid_yaml_keeps_last_valid_snapshot_and_recovers_after_fix` e UI E-05: YAML inválido preserva o último snapshot, mostra o caminho e volta a salvar após correção. | aprovado | headless; desktop não executado |
| E-06 | `own_writes_and_draft_saves_do_not_request_reconciliation` e UI E-06: salvar pela UI não dispara recarga nem falso evento externo. | aprovado | headless; desktop não executado |
| E-07 | `stale_generation_from_another_collection_does_not_reconcile` e UI E-07: cursor de outra coleção apenas realinha a geração, sem recarregar nem mudar a árvore. Troca real entre coleções no desktop não executada. | aprovado (cursor antigo) | headless; desktop não executado |
| E-08 | `rename_keeps_id_and_duplicate_id_is_rejected_without_losing_snapshot` e UI E-08: renomear mantém ID/seleção; ID duplicado mantém a árvore anterior e o editor utilizável. | aprovado | headless; desktop não executado |
| E-09 | `environment_save_conflicts_when_cli_changed_manifest_and_preserves_state` e UI E-09: manifesto alterado externamente, formulário preservado, 409 sem vazamento e salvamento explícito após recarregar. | aprovado | headless; desktop não executado |
| E-10 | `runner_uses_the_initial_snapshot_while_the_file_changes`: execução em voo mantém a URL inicial; a seguinte usa o disco novo. | aprovado | não executado |

## Grupo L — lixeira

| ID | Automação executada | Status da automação | Validação desktop |
|---|---|---|---|
| L-01 | `request_is_restored_with_same_bytes_and_id`, `trash_api_deletes_reports_and_restores_with_content_revision`. | aprovado | não executado |
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
| M-01 | `inspection_does_not_change_project_and_apply_creates_backup`; inspeção CLI em cópia temporária. | aprovado | não executado |
| M-02 | `backup_originals_match_pre_migration_bytes`, `inspection_does_not_change_project_and_apply_creates_backup`; aplicação CLI. | aprovado | não executado |
| M-03 | `interrupted_application_restores_originals_and_marks_failed`, `missing_proposed_bytes_fail_without_partial_success`. | aprovado | não executado |
| M-04 | `rollback_restores_original_bytes_and_blocks_later_edits`, `rollback_keeps_independent_files_and_restores_touched_bytes`, rollback CLI. | aprovado | não executado |
| M-05 | `rollback_restores_original_bytes_and_blocks_later_edits` — edição posterior bloqueia rollback. | aprovado | não executado |
| M-06 | `future_schema_is_rejected_without_touching_files`, `corrupt_and_legacy_backups_are_listed_without_writes`. | aprovado | não executado |
| M-07 | `migration_api_requires_confirmation_and_blocks_writes_after_rollback`; `validate` CLI falha após rollback e reaplicação funciona. | aprovado | não executado |
| M-08 | `validate` CLI retorna `1` com dica acionável; escrita bloqueada por `collection_unavailable`. | aprovado | não executado |
| M-09 | `corrupt_and_legacy_backups_are_listed_without_writes` — backup legado listado sem alteração. | aprovado | não executado |
| M-10 | `changed_file_between_inspection_and_apply_invalidates_plan`, `migration_api_requires_confirmation_and_blocks_writes_after_rollback` (revisão divergente). | aprovado | não executado |

## Critérios globais

| ID | Resultado | Status |
|---|---|---|
| G-01 | Linux local (107 testes e 11 fluxos headless) e workflow automatizado Linux/Windows aprovados; fluxo manual completo de desktop não. | parcial |
| G-02 | 107 testes mais demonstração HTTP 200/404; offline, timeout, cancelamento, runner e scripts continuam cobertos. | aprovado |
| G-03 | IPC sem listener em `tests/desktop.rs`; o harness de teste é um exemplo isolado, não o produto; offline coberto; recuperação e reconciliação só leem o projeto local. | aprovado |
| G-04 | Rascunho não contém segredo resolvido; redaction/exportação e ausência do conteúdo da lixeira em listagem/log/projeto cobertas. | aprovado |
| G-05 | UI G-05: foco inicial no primeiro botão, Tab entre botões, Escape como Cancelar, `aria-live`/`role` e contexto preservado. Desktop real e leitor de tela não executados. | parcial |
| G-06 | Workflow remoto executado na tag `v0.3.0`; `.deb`, `.AppImage`, `.exe` e `SHA256SUMS` publicados e verificados. A instalação manual em máquinas limpas segue pendente. | aprovado |
| G-07 | Guias, limitações e notas atualizados, incluindo rascunho em texto claro, harness headless, YAML inválido e coordenação entre processos; revisão editorial feita nesta execução. | aprovado |

## Pendências pós-publicação

1. Executar a matriz manual de desktop no Linux e no Windows. O harness headless cobre 11 fluxos, mas não substitui WebView, IPC real, leitor de tela, menu nativo nem abertura de instalador.
2. Medir a reconciliação com 1.000 requisições e registrar máquina, tempo e responsividade.
3. Instalar manualmente `.deb`, `.AppImage` e o NSIS em máquinas limpas; o workflow já abriu o AppImage em Xvfb/Openbox e confirmou a janela do executável Windows, mas não testou a instalação em ambiente limpo.
4. Corrigir qualquer falha encontrada e publicar uma versão de correção com novas evidências, sem converter cenário não executado em aprovação.

A tag `v0.3.0` foi criada em 2026-10-27 e a release foi publicada com os três instaladores e checksums. A matriz manual e o benchmark continuam sendo lacunas conhecidas, não bloqueios retroativos da publicação.
