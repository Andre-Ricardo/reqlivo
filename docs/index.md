---
title: Reqlivo
---

# Suas APIs. Seus arquivos.

O **Reqlivo** é um cliente de APIs open source que funciona localmente, sem criar conta. Organize requisições em arquivos, execute pela CLI ou pela interface web local e acompanhe os resultados dos testes.

Inspirado no [Insomnia](https://insomnia.rest/) e no [Bruno](https://www.usebruno.com/). Projeto independente, sem afiliação com esses produtos.

> **MVP funcional:** HTTP, ambientes, autenticação, scripts, testes e importação/exportação já estão disponíveis. Consulte as [limitações](limitacoes.md) para conhecer o escopo atual.

## Comece em poucos comandos

Instale uma versão estável recente do Rust (mínimo declarado: 1.85), clone e compile:

```bash
git clone https://github.com/Andre-Ricardo/reqlivo.git
cd reqlivo
cargo build --release --locked
./target/release/reqlivo --collection minha-api init --name "Minha API"
./target/release/reqlivo --collection minha-api serve
```

Abra **http://127.0.0.1:8787**. A interface é servida pelo próprio binário; não é necessário Node.js nem banco de dados.

## Explore a documentação

| Guia | O que você encontra |
|---|---|
| [Uso](uso.md) | Instalação, interface, CLI, ambientes e execução. |
| [Formato da coleção](formato.md) | YAML, variáveis, autenticação, scripts e segredos. |
| [Escopo do MVP](mvp.md) | Funcionalidades implementadas e critérios de aceite. |
| [Limitações](limitacoes.md) | Compatibilidade, restrições e próximos passos. |

## Experimente a coleção de demonstração

Em um terminal, na raiz do projeto:

```bash
python3 examples/servidor_demo.py
```

Em outro:

```bash
cargo run -- --collection examples/demo run --env dev
cargo run -- --collection examples/demo serve
```

A demonstração usa somente dados sintéticos e um servidor HTTP em loopback. O script registra requisições em `/tmp/reqlivo-demo.jsonl`; use `SYNTH_LOG` para escolher outro caminho cujo diretório já exista.

## Feito para trabalhar localmente

- Coleções em **OpenCollection 1.0.0**, com extensões do Reqlivo, prontas para versionar.
- HTTP com parâmetros, headers, vários tipos de corpo e autenticação Basic, Bearer ou API key.
- Ambientes e variáveis com herança, scripts síncronos e asserções declarativas.
- Importação/exportação de cURL, Postman e OpenCollection, com limites descritos nos guias.
- Referências de segredos por variáveis do processo e mascaramento de valores conhecidos.

## Participe

[Código-fonte e issues](https://github.com/Andre-Ricardo/reqlivo) · [Guia de contribuição](https://github.com/Andre-Ricardo/reqlivo/blob/main/CONTRIBUTING.md) · [Licença MIT](https://github.com/Andre-Ricardo/reqlivo/blob/main/LICENSE)

Envie suas contribuições por pull request. Alterações de outros colaboradores precisam da aprovação do mantenedor **@Andre-Ricardo**.
