---
title: Rust e Cargo
---

# Rust e Cargo

O núcleo, a CLI, o transporte HTTP e o servidor local são implementados em Rust. O projeto declara edição **2024** e `rust-version = "1.85"` em `Cargo.toml`; esse campo representa o mínimo declarado, não a versão estável mais recente nem uma garantia de teste nesse mínimo.

## Documentação oficial atual

| Referência | Uso |
|---|---|
| [The Rust Programming Language](https://doc.rust-lang.org/stable/book/) | Ownership, borrowing, tipos, tratamento de erros e concorrência. |
| [Rust Reference](https://doc.rust-lang.org/stable/reference/) | Semântica da linguagem e regras da edição. |
| [Biblioteca padrão](https://doc.rust-lang.org/stable/std/) | Arquivos, caminhos, coleções, tempo e sincronização. |
| [Cargo Book](https://doc.rust-lang.org/stable/cargo/) | Manifesto, dependências, lockfile e publicação. |
| [Edition Guide](https://doc.rust-lang.org/stable/edition-guide/) | Diferenças e migração entre edições. |

Os caminhos `stable` acompanham a versão estável oficial. Para reproduzir uma API antiga, use a documentação daquela versão, em vez de presumir que `stable` corresponde ao compilador local. A referência Rust consultada pelo Context7 explica essa distinção entre documentação corrente e versões arquivadas.

## Onde estudar no Reqlivo

- `src/model.rs`: tipos e contratos serializáveis.
- `src/store.rs`: persistência, revisões e arquivos.
- `src/runner.rs` e `src/transport.rs`: execução assíncrona.
- `src/main.rs`: CLI e códigos de saída.
- `tests/integration.rs`: cenários com servidor HTTP local.

Para o ciclo de desenvolvimento e os comandos verificados, consulte [Como contribuir](https://github.com/Andre-Ricardo/reqlivo/blob/main/CONTRIBUTING.md). Para as APIs de terceiros, consulte [Bibliotecas Rust](bibliotecas-rust.md), que separa a versão fixada no lockfile da mais recente.

## Consulta Context7

ID resolvido: `/rust-lang/reference`. Tópico consultado: referência estável e edição 2024. O índice do Context7 pode acompanhar uma branch do upstream; os links `stable` acima são a referência para a edição publicada da documentação.

[Voltar ao catálogo](tecnologias.md)
