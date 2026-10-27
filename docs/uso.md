---
title: Guia de uso
---

# Uso

Este guia cobre o fluxo diário da CLI e da interface desktop do Reqlivo. Para o formato dos arquivos, consulte [formato.md](formato.md).

## Instalação e build

```bash
cargo build --release
./target/release/reqlivo --help
```

Durante o desenvolvimento, substitua `reqlivo` por `cargo run --`.

## Criar uma coleção

```bash
reqlivo --collection minha-api init --name "Minha API"
```

O comando cria `opencollection.yml`, `README.md`, `.gitignore` e o ambiente `dev` com `baseUrl=http://127.0.0.1:18080`. O diretório de destino deve estar vazio ou não existir.

Abra a interface:

```bash
npm ci
npm run desktop:dev -- -- --collection minha-api
```

Consulte [desktop.md](desktop.md) para pré-requisitos e instaladores. **Abrir coleção** seleciona a pasta pelo diálogo nativo. Sem argumentos, o app reabre a última pasta selecionada ou cria uma coleção nos dados locais do aplicativo. Não há servidor da interface nem comando `serve`.

## Interface desktop

### Árvore e editor

1. Use a árvore para abrir uma pasta ou requisição.
2. Edite nome, método, URL e tags.
3. Percorra as abas **Params**, **Headers**, **Corpo**, **Auth**, **Scripts** e **Testes**.
4. Use **Salvar** (`Ctrl/Cmd+S`) para gravar no arquivo.
5. Use **Enviar** (`Ctrl/Cmd+Enter`) para executar o rascunho atual, mesmo antes de salvar.

A árvore é navegável por teclado. `↑`/`↓` percorre itens, `Home`/`End` vai ao início/fim, `←`/`→` recolhe/expande pastas e `Enter`/`Espaço` abre o item focado. O filtro da árvore fica em `Ctrl/Cmd+K`.

### Resposta

O painel mostra:

- status HTTP e texto;
- tempo e tamanho;
- URL final e indicador de redirecionamento;
- abas de corpo, headers, testes e logs;
- botão para corpo bruto quando o conteúdo é textual;
- erros tipados (`connect`, `timeout`, `offline`, `script`, etc.).

Respostas maiores que `max_response_bytes` são truncadas com aviso. Conteúdo binário é exibido como prévia/base64 limitada.

### Runner

1. Abra **Runner** (`Alt+R`).
2. Escolha ambiente, tags, pasta ou “somente a requisição atual”.
3. Inicie e acompanhe o relatório.
4. Cancele enquanto a execução estiver em andamento.

O resultado diferencia:

| Status | Significado |
|---|---|
| `executed` | houve resposta HTTP, mas não havia teste/asserção. |
| `passed` | havia testes/asserções e todos passaram. |
| `failed` | algum teste/asserção falhou. |
| `error` | falha de preparação, rede, script ou transporte. |
| `cancelled` | cancelado pelo usuário. |
| `skipped` | não selecionado pelo filtro. |

### Ambientes e segredos

Abra **Ambientes** no topo. É possível:

- criar, editar e excluir ambientes;
- definir herança com `extends`;
- marcar a variável como segredo;
- informar a chave de ambiente do processo (por exemplo, `REQLIVO_API_TOKEN`);
- definir o ambiente padrão.

A UI mostra apenas `definido`/`não definido` para a referência. O valor real nunca é enviado à WebView.

### Configurações

O botão de engrenagem abre as configurações da coleção: timeout, timeout de script, redirects, limite de resposta, ambiente padrão, modo offline e permissão para variáveis do processo.

O botão **Validar** executa a mesma verificação da CLI (`reqlivo validate`) sem enviar requisições e mostra problemas, avisos e arquivos não suportados.

## CLI

### Listar e validar

```bash
reqlivo --collection minha-api requests
reqlivo --collection minha-api validate
reqlivo --collection minha-api validate --json
```

`validate` verifica método/URL, JSON/XML inválidos, herança/ciclos de ambientes, referências de segredo e arquivos não suportados. Também há avisos de migração e compatibilidade.

### Executar

```bash
reqlivo --collection minha-api run --env dev
reqlivo --collection minha-api run --tag smoke
reqlivo --collection minha-api run --request req_... --json
reqlivo --collection minha-api run --folder fld_... --output /tmp/relatorio.json
```

`--request` e `--tag` são listas e podem ser repetidos (`--tag smoke --tag demo`); `--folder` aceita um ID. O ambiente padrão é usado quando `--env` é omitido.

O relatório humano mostra status, tempo e falhas. O JSON inclui `summary`, `requests`, `tests`, `logs`, `response`, `auth`, `effective_variables` e `deterministic_hash`.

### Enviar uma requisição

```bash
reqlivo --collection minha-api send req_... --env dev
reqlivo --collection minha-api send req_... --json
```

`send` usa o ambiente padrão quando `--env` não é informado. Em caso de falha, retorna código `1`.

### Importar

```bash
# cURL inline
reqlivo --collection minha-api import curl \
  "curl -X POST http://127.0.0.1:18080/echo -H 'X-Test: 1' -d '{\"a\":1}'"

# arquivo ou diretório
reqlivo --collection minha-api import curl ./requisicao.sh
reqlivo --collection minha-api import postman ./collection.json
reqlivo --collection minha-api import opencollection ./colecao-opencollection
```

- O original importado é preservado em `.reqlivo/imports/` quando aplicável.
- Postman tem relatório de perdas; scripts ficam em `extensions.postmanScripts` e **não são executados**.
- Use `--folder <fld_...>` para importar dentro de uma pasta.

### Exportar

```bash
reqlivo --collection minha-api export curl req_... --output /tmp/echo.sh
reqlivo --collection minha-api export postman --output /tmp/postman.json
reqlivo --collection minha-api export opencollection --output /tmp/col.yml
reqlivo --collection minha-api export project --output /tmp/minha-api-copia
```

Exportações padrão omitem:

- Authorization, cookies, tokens, chaves e headers sensíveis literais;
- auth literal (Basic/Bearer/API key);
- parâmetros de query sensíveis literais;
- variáveis marcadas como segredo.

Templates (`{{variavel}}`) e referências de segredo são preservados. No cURL, substitua os templates pelo valor do ambiente antes de executar ou use a UI/CLI para enviar a requisição. O relatório de exportação lista os avisos. A exportação de projeto grava uma cópia sanitizada, não um espelho byte a byte; arquivos extras (README, notas) são copiados sem análise, então não coloque segredos neles.

### Ambientes e histórico

```bash
reqlivo --collection minha-api env list
reqlivo --collection minha-api env show seguro
reqlivo --collection minha-api history --limit 10
```

`env show` mascara valores secretos. O histórico fica em `.reqlivo/history/runs.jsonl`; logs ficam em `.reqlivo/logs/events.jsonl`.

## Demonstração

Com o servidor sintético em execução:

```bash
# Em um terminal
python3 examples/servidor_demo.py

# Em outro
reqlivo --collection examples/demo run --env dev
REQLIVO_DEMO_TOKEN=valor-sintetico reqlivo --collection examples/demo run --env seguro
reqlivo --collection examples/demo export curl req_4a2b3c4d5e6f7081
```

Endpoints usados: `/echo`, `/status/404`. O servidor grava requisições em `$SYNTH_LOG` (padrão definido pelo próprio script).

## JSON e integração

Use `--json` para automação. Os corpos da ponte IPC usam `camelCase`; os relatórios do runner e os objetos de domínio usam `snake_case`, refletindo o modelo canônico.

## Offline

Ative pela coleção (Configurações → Modo offline) ou por:

```bash
REQLIVO_OFFLINE=1 reqlivo --collection examples/demo run --env dev
```

No modo offline:

- destinos externos são recusados antes de abrir conexão;
- `localhost`, loopback, IPs privados e link-local são permitidos;
- redirecionamentos para fora da rede local não são seguidos;
- não há chamada a telemetria, atualização ou nuvem.

## Erros e códigos de saída

| Código | Situação |
|---|---|
| `0` | comando concluído sem falhas/erros/cancelamentos. |
| `1` | falha de validação, execução, importação/exportação ou erro de rede. |
| `--json` | o erro vai para `stderr`; o resultado vai para `stdout`. |

Mensagens de erro incluem categoria, código e próximo passo quando disponível. Segredos conhecidos são redigidos.
