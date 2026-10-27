# Reqlivo

[![CI](https://github.com/Andre-Ricardo/reqlivo/actions/workflows/ci.yml/badge.svg)](https://github.com/Andre-Ricardo/reqlivo/actions/workflows/ci.yml)
[![Licença MIT](https://img.shields.io/badge/licen%C3%A7a-MIT-blue.svg)](LICENSE)

Cliente de APIs **local-first**, sem conta e baseado em arquivos. O Reqlivo lê e grava uma coleção em **OpenCollection 1.0.0** com extensões `extensions.reqlivo`, executa HTTP e compartilha o mesmo modelo entre a CLI e a interface desktop Tauri.

[Documentação](https://andre-ricardo.github.io/reqlivo/) · [Contribuir](CONTRIBUTING.md) · [Reportar problema](https://github.com/Andre-Ricardo/reqlivo/issues)

> Estado atual: **MVP funcional com persistência e edição recuperável (0.3.0)**. O escopo implementado e as lacunas conhecidas estão em [docs/mvp.md](docs/mvp.md) e [docs/limitacoes.md](docs/limitacoes.md). A validação da 0.3.0 está em [docs/proxima-release/resultado-validacao.md](docs/proxima-release/resultado-validacao.md); a release está preparada, mas não foi publicada nem validada manualmente em Windows.

## O que está pronto

- Projeto local com coleção, pastas, requisições, ambientes e configurações.
- HTTP com métodos, query/path params, headers, corpos JSON/texto/XML/SPARQL, formulário e multipart.
- Metadados de resposta: status, headers, corpo, tempo, tamanho, URL final, redirecionamento e truncamento.
- Variáveis com precedência explícita: runtime → requisição → pasta → ambiente (pai→filho) → coleção.
- Autenticação Basic, Bearer e API key (header/query), com herança de pasta e coleção.
- Scripts síncronos de pré-requisição, pós-resposta e testes, com limite de memória/pilha e timeout.
- Asserções declarativas (`runtime.assertions`) além dos testes em JavaScript.
- Runner local com relatório, filtros, hash determinístico, cancelamento e histórico.
- Importação/exportação de cURL, Postman, OpenCollection e projeto.
- Rascunhos locais com autosave, recuperação de órfãos e guarda de navegação.
- Reconciliação de alterações externas por hash, com recarga assistida e conflito de revisão.
- Lixeira recuperável com restauração por ID, escolha de destino e proteção contra sobrescrita.
- Migrações com inspeção somente-leitura, backup verificável, rollback e tela de recuperação.
- Aplicativo Tauri 2 para Linux e Windows, com IPC local, atalhos e foco visível.
- Segredos por referência (`provider: env`) e redaction de logs, relatórios e exportações.
- Modo offline real: bloqueia destinos externos e redirecionamentos para fora da rede local.

## Requisitos

- Rust estável recente (edição 2024; o crate declara `rust-version = "1.85"`).
- Desktop: WebKitGTK 4.1 no Linux e WebView2 no Windows. Node.js 22 e npm são usados apenas no desenvolvimento/build.
- [Instaladores Linux e Windows](https://github.com/Andre-Ricardo/reqlivo/releases) e [guia desktop/releases](docs/desktop.md).
- Python 3 apenas para o servidor sintético de demonstração/testes manuais.

```bash
git clone https://github.com/Andre-Ricardo/reqlivo.git
cd reqlivo
cargo build --release
cargo test
```

## Início rápido

Crie uma coleção local:

```bash
cargo run -- init --collection minha-api --name "Minha API"
npm ci
npm run desktop:dev -- -- --collection minha-api
```

O aplicativo abre uma janela nativa. No primeiro uso sem argumentos, cria uma coleção nos dados locais do aplicativo. Use **Abrir coleção** para selecionar outra pasta. O comando `serve` foi removido.

### Demonstração incluída

O diretório [`examples/demo`](examples/demo) contém uma coleção pronta, com pasta, auth herdada, scripts, asserções e um segredo referenciado.

1. Inicie o servidor sintético, em outro terminal:

   ```bash
   python3 examples/servidor_demo.py
   ```

2. Execute a coleção:

   ```bash
   cargo run -- --collection examples/demo validate
   cargo run -- --collection examples/demo run --env dev
   ```

3. Para exercitar o segredo referenciado:

   ```bash
   REQLIVO_DEMO_TOKEN=valor-sintetico \
     cargo run -- --collection examples/demo run --env seguro
   ```

4. Para abrir a demonstração na interface:

   ```bash
   npm run desktop:dev -- -- --collection examples/demo
   ```

O valor do segredo nunca é gravado no projeto, no relatório, no log ou na exportação padrão. Ele aparece apenas como `••••••`/`<segredo>`.

## CLI

Todos os comandos aceitam `--collection <diretório>` e `--json` quando fazem sentido.

| Comando | Uso |
|---|---|
| `init` | cria uma coleção local. |
| `run` | executa a coleção ou filtros (`--env`, `--tag`, `--request`, `--folder`). |
| `send` | envia uma requisição salva. |
| `import` | importa cURL, Postman ou OpenCollection. |
| `export` | exporta cURL, Postman, OpenCollection ou o projeto. |
| `validate` | valida arquivos e compatibilidade. |
| `requests` | lista requisições na ordem de execução. |
| `env list/show` | inspeciona ambientes sem revelar segredos. |
| `history` | lê o histórico local de execuções. |
| `migration inspect` | prepara um plano de migração sem alterar arquivos. |
| `migration apply` | aplica um plano confirmado, com backup e hashes. |
| `migration rollback` | reverte uma migração sem edições posteriores. |
| `migration list` | lista planos, aplicações e rollbacks. |

Exemplos:

```bash
# Lista e executa por tag
cargo run -- --collection examples/demo requests
cargo run -- --collection examples/demo run --tag smoke --json

# Envia uma requisição específica
cargo run -- --collection examples/demo send req_4a2b3c4d5e6f7081 --env dev

# Importa um comando cURL ou uma coleção Postman
cargo run -- --collection minha-api import curl "curl -X POST http://127.0.0.1:18080/echo -d '{\"ok\":true}'"
cargo run -- --collection minha-api import postman ./collection.json

# Exporta um comando cURL, sem credenciais literais (templates são preservados)
cargo run -- --collection examples/demo export curl req_4a2b3c4d5e6f7081 --output /tmp/echo.sh

# Exporta o projeto sanitizado (o destino não pode existir; comentários podem ser normalizados)
cargo run -- --collection examples/demo export project --output /tmp/demo-copia

# Inspeciona, aplica e reverte uma migração de schema
cargo run -- --collection minha-api migration inspect
cargo run -- --collection minha-api migration apply <plan-id> --revision <revisao>
cargo run -- --collection minha-api migration rollback <plan-id> --revision <revisao>
```

Código de saída de `run`: `0` quando não há falhas/erros/cancelamentos; `1` caso contrário. `validate` também retorna `1` quando encontra problemas ou arquivos não suportados.

## Interface desktop

- Árvore navegável por teclado (`↑`/`↓`, `Home`/`End`, `←`/`→` em pastas).
- Editor de parâmetros, headers, corpo, auth, scripts e asserções.
- Autosave de rascunho com estados visíveis e recuperação na reabertura.
- Aviso de alteração externa, comparação e recarga sem sobrescrever edição.
- Lixeira com restauração e escolha de destino.
- Diálogo de migração com inspeção, confirmação, backup e rollback.
- Painel de resposta com corpo bruto, headers, testes e logs.
- Runner com filtros, ambiente, relatório e cancelamento.
- Editor de ambientes, segredos por referência e configurações da coleção.
- Importação/exportação e validação pela própria UI.

Atalhos principais:

| Atalho | Ação |
|---|---|
| `Ctrl/Cmd+S` | salvar requisição |
| `Ctrl/Cmd+Enter` | enviar requisição |
| `Ctrl/Cmd+K` | filtrar a árvore |
| `Alt+N` | nova requisição |
| `Alt+F` | nova pasta |
| `Alt+R` | abrir runner |
| `?` | ajuda e atalhos |

## Estrutura do código

| Caminho | Responsabilidade |
|---|---|
| `src/model.rs` | modelo canônico OpenCollection + extensões Reqlivo. |
| `src/store.rs` | persistência, árvore, revisões, ambientes e delegação da lixeira. |
| `src/persist.rs` | hashes, escrita atômica, permissões, caminhos seguros e lock entre processos. |
| `src/drafts.rs` | rascunhos versionados, gerações, órfãos e adoção. |
| `src/trash.rs` | índice da lixeira, restauração, reconciliação e entradas legadas. |
| `src/migration.rs` | planos, inspeção somente-leitura, aplicação, rollback e recuperação. |
| `src/variables.rs` | resolução de variáveis, precedência e segredos. |
| `src/auth.rs` | auth efetiva e herança. |
| `src/body.rs` | URL, params, headers, auth e corpos preparados. |
| `src/transport.rs` | HTTP, metadados, limites, redirects e offline. |
| `src/scripts.rs` | sandbox QuickJS, `rq.*`, `test`/`expect`, timeout e logs. |
| `src/assertions.rs` | operadores declarativos. |
| `src/runner.rs` | execução, status, relatório, hash e redaction. |
| `src/import_export.rs` | cURL, Postman, OpenCollection e projeto. |
| `src/server.rs` | operações do núcleo com roteamento em memória, sem listener. |
| `src/desktop.rs` | adaptador IPC testável sem WebView. |
| `src-tauri/` | janela, diálogos, estado e instaladores Tauri 2. |
| `src/main.rs` | CLI clap, relatórios e códigos de saída. |
| `src/assets/` | HTML, CSS e JavaScript da interface. |
| `tests/integration.rs` | testes ponta a ponta com servidor HTTP sintético. |

## Documentação

- [Desktop, pré-requisitos e releases](docs/desktop.md)
- [Notas da versão 0.3.0](docs/release-0.3.0.md)
- [Validação da 0.3.0](docs/proxima-release/resultado-validacao.md)

- [Tecnologias e documentação oficial atual](docs/tecnologias.md)
- [Formato da coleção](docs/formato.md)
- [Uso da CLI e da interface](docs/uso.md)
- [Escopo e critérios do MVP](docs/mvp.md)
- [Limitações e próximos passos](docs/limitacoes.md)

Inspirado no [Insomnia](https://insomnia.rest/) e no [Bruno](https://www.usebruno.com/), o Reqlivo é um projeto independente, sem afiliação com esses produtos.

Consulte também o [guia de contribuição](CONTRIBUTING.md).

## Segurança

- A interface só acessa o núcleo via IPC Tauri. Nenhuma porta é aberta para a UI; APIs de destino são chamadas pelo Rust.
- Credenciais devem ser variáveis ou segredos referenciados; a UI avisa sobre valores literais.
- Rascunhos são texto local sem criptografia e podem conter credenciais literais digitadas; no Unix, os arquivos usam permissão `0600`. Segredos resolvidos de `provider: env` não são gravados.
- Logs, mensagens de teste, URLs e headers conhecidos como sensíveis são redigidos.
- Exportações padrão omitem auth/cabeçalhos/parâmetros literais sensíveis e variáveis marcadas como segredo.
- Segredos usam `provider: env`; `system` e `file` ainda não são implementados.
- `REQLIVO_OFFLINE=1` ou a configuração da coleção ativam o modo offline, inclusive para redirecionamentos.

## Licença

[MIT](LICENSE) — permite usar, modificar e distribuir o projeto, inclusive comercialmente, mantendo o aviso de copyright e a licença.
