# Como contribuir

Contribuições são bem-vindas por issues e pull requests. O mantenedor é [@Andre-Ricardo](https://github.com/Andre-Ricardo).

## Fluxo de contribuição

1. Faça um fork e crie uma branch para sua alteração.
2. Descreva o problema e implemente uma mudança com escopo claro.
3. Atualize a documentação e inclua testes quando houver mudança de comportamento.
4. Execute `cargo fmt --all -- --check` e `cargo test --locked`.
5. Abra um PR para `main`, explicando o resultado e como verificá-lo.

Contribuições de outras pessoas precisam da aprovação de @Andre-Ricardo, responsável por todos os arquivos em `CODEOWNERS`. A branch `main` exige CI aprovada, resolução das conversas e nova revisão quando houver alterações após a aprovação. O proprietário mantém a exceção administrativa para administrar o projeto.

Use dados sintéticos em testes, exemplos e capturas. Não inclua tokens, arquivos `.env`, histórico local `.reqlivo/` nem dados pessoais. A documentação atual é escrita em português.

## Desenvolvimento

```bash
cargo build --locked
cargo test --locked
cargo run -- --collection examples/demo validate
```

Para testar a demonstração, inicie `python3 examples/servidor_demo.py` em outro terminal e execute `cargo run -- --collection examples/demo run --env dev`.

Consulte o [guia de uso](docs/uso.md), o [formato da coleção](docs/formato.md) e as [limitações](docs/limitacoes.md) antes de propor funcionalidades maiores.

## Licença das contribuições

Ao enviar uma contribuição, você concorda em disponibilizá-la sob a [licença MIT](LICENSE) do projeto.
