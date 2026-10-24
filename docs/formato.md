---
title: Formato da coleção
---

# Formato da coleção

O Reqlivo usa **OpenCollection 1.0.0** como formato canônico. A coleção é um diretório legível, versionável e editável fora do aplicativo. Dados que não pertencem ao projeto ficam em `.reqlivo/`.

## Layout

```text
minha-api/
├── opencollection.yml
├── .gitignore
├── README.md
├── basicos/
│   ├── folder.yml
│   ├── echo.yml
│   └── status-404.yml
├── segredos/
│   ├── folder.yml
│   └── segredo.yml
└── .reqlivo/                 # cache, logs, lixeira, backups e imports
```

- `opencollection.yml` é o manifesto da coleção.
- Cada pasta contém um `folder.yml`.
- Cada requisição é um arquivo `.yml` com um `info.id` lógico em `extensions.reqlivo.id`.
- IDs são opacos e estáveis, com prefixos: `col_`, `fld_`, `req_`, `env_`, `run_` e `send_`.
- IDs usam 16 caracteres hexadecimais. Não dependa do nome do arquivo para identificar um item.

## Manifesto

Exemplo reduzido (baseado em [`examples/demo`](https://github.com/Andre-Ricardo/reqlivo/tree/main/examples/demo)):

```yaml
opencollection: 1.0.0
info:
  name: Reqlivo Demo
  summary: Coleção local criada pelo Reqlivo.
config:
  environments:
    - name: dev
      variables:
        - name: baseUrl
          value: http://127.0.0.1:18080
        - name: token
          value: token-demo
    - name: seguro
      extends: dev
      variables:
        - name: apiToken
          secret: true
extensions:
  reqlivo:
    schemaVersion: 1
    id: col_...
    settings:
      default_environment: dev
      default_timeout_ms: 30000
      follow_redirects: true
      max_redirects: 5
      offline_only: false
      allow_process_env: false
      max_response_bytes: 10485760
      script_timeout_ms: 2000
    secret_refs:
      - name: apiToken
        environment: seguro
        provider: env
        key: REQLIVO_DEMO_TOKEN
    environment_ids:
      dev: env_...
      seguro: env_...
```

### Configurações

| Campo | Efeito |
|---|---|
| `default_environment` | ambiente usado quando a CLI/UI não informa outro. |
| `default_timeout_ms` | timeout HTTP padrão; `0` desabilita o timeout do cliente. |
| `follow_redirects` / `max_redirects` | política de redirecionamento. |
| `offline_only` | restringe requisições e redirects a loopback/privado/link-local. |
| `allow_process_env` | permite resolver `{{VAR}}` diretamente do ambiente do processo. |
| `max_response_bytes` | limite de corpo lido; o restante é truncado. |
| `script_timeout_ms` | tempo máximo de cada script. |

`REQLIVO_OFFLINE=1` (ou `true`) força o modo offline mesmo que a coleção não o habilite.

## Pasta

```yaml
info:
  name: Básicos
  type: folder
  seq: 1
request:
  auth:
    type: bearer
    token: "{{token}}"
  headers:
    - name: X-Colecao
      value: reqlivo-demo
extensions:
  reqlivo:
    schemaVersion: 1
    id: fld_...
```

`request` é o bloco de padrões da pasta: headers, auth, variáveis, scripts e settings. Requisições com `auth: inherit` sobem da pasta folha até a raiz e depois para a coleção.

## Requisição

```yaml
info:
  name: Echo
  type: http
  seq: 1
  tags: [smoke, demo]
http:
  method: GET
  url: "{{baseUrl}}/echo?origem=reqlivo"
  headers:
    - name: Accept
      value: application/json
  params:
    - name: id
      value: "42"
      type: path
  auth: inherit
  body:
    type: json
    data: |
      {"exemplo": true}
runtime:
  scripts:
    - type: before-request
      code: |
        rq.env.set('nomeRuntime', 'demo');
    - type: tests
      code: |
        test('status 200', function () {
          expect(rq.response.status).toBe(200);
        });
  assertions:
    - expression: res.status
      operator: eq
      value: "200"
      description: status esperado
settings:
  timeout: 5000
extensions:
  reqlivo:
    schemaVersion: 1
    id: req_...
```

### Corpos

| `type` | Conteúdo |
|---|---|
| `json` | string JSON; validada antes do envio. |
| `text` | texto simples. |
| `xml` | XML; validado antes do envio. |
| `sparql` | consulta SPARQL. |
| `form-urlencoded` | lista de campos nome/valor. |
| `multipart-form` | lista de campos; `type: file` lê um caminho local. |

`body: null` (ou ausência de `http.body`) significa sem corpo.

### Parâmetros

- `type: query` acrescenta `nome=valor` à query string.
- `type: path` substitui `:nome` na URL antes da query.
- Valores aceitam `{{variavel}}`; a URL final não pode manter placeholders não resolvidos.

### Auth

| Forma | Campos |
|---|---|
| `inherit` | herda da pasta/coleção. |
| Basic | `type: basic`, `username`, `password`. |
| Bearer | `type: bearer`, `token`. |
| API key | `type: apikey`, `key`, `value`, `placement: header\|query`. |
| Sem auth | `auth: null` ou ausência. |

Valores literais funcionam, mas não são recomendados. A UI avisa e a exportação padrão os omite.

## Variáveis e precedência

A resolução aplica, do menor para o maior escopo:

```text
coleção → ambiente (pai→filho) → pasta (raiz→folha) → requisição → runtime (scripts)
```

Em outras palavras, o runtime vence a requisição, que vence a pasta, que vence o ambiente, que vence a coleção.

Uma variável pode ser:

```yaml
- name: baseUrl
  value: http://127.0.0.1:18080
  disabled: false
```

- `disabled: true` mantém a variável no arquivo, mas não a resolve.
- Variáveis de ambiente do processo só são consultadas quando `allow_process_env: true`, exceto segredos explicitamente referenciados.

## Segredos por referência

Um segredo **não tem valor no projeto**:

```yaml
config:
  environments:
    - name: seguro
      variables:
        - name: apiToken
          secret: true
extensions:
  reqlivo:
    secret_refs:
      - name: apiToken
        environment: seguro
        provider: env
        key: REQLIVO_DEMO_TOKEN
```

Regras:

- `provider: env` lê `key` do ambiente do processo.
- O valor nunca é serializado para a UI, relatório, histórico ou exportação.
- A UI mostra apenas `definido`/`não definido` e o valor efetivo como `••••••`.
- `provider: system` e `provider: file` estão declarados no modelo, mas ainda não são implementados no MVP.
- O valor real só existe em memória durante a resolução/execução.

## Scripts

Fases aceitas:

| `type` | Momento |
|---|---|
| `before-request` | antes de preparar/enviar; pode alterar headers, URL e corpo via patch. |
| `after-response` | após uma resposta HTTP. |
| `tests` | depois das asserções e do pós-resposta; usa `rq.response`. |

API disponível no sandbox:

- `rq.env.get/set/has/delete/all`
- `rq.request` e `setHeader/header/removeHeader`
- `rq.response.status`, `headers`, `body`, `json()`, `header(name)`
- `test(name, fn)` e `expect(...)` (`toBe`, `toEqual`, `toContain`, `toMatch`, `toBeTruthy`, `toBeFalsy`, `toBeNull`, `toBeDefined`, `toBeGreaterThan`, `toBeLessThan`, `toHaveLength`, `.not`)
- `rq.assert(condition, message)` e `console.log/info/warn/error`

Os scripts são **síncronos**; promises não são drenadas. O timeout interrompe loops infinitos. Logs e mensagens são redigidos antes de entrar no relatório.

## Asserções declarativas

```yaml
runtime:
  assertions:
    - expression: res.body.ok
      operator: eq
      value: "true"
```

Operadores: `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `contains`, `notContains`, `startsWith`, `endsWith`, `matches`, `in`, `notIn`, `isString`, `isNumber`, `isBoolean`, `isArray`, `isObject`, `isNull`, `exists`, `notExists`, `lengthEq`.

Uma requisição sem testes/asserções que recebe HTTP 404 termina como **executada**, não **passou**. Esse é o contrato para não confundir “não falhou” com “foi validada”.

## Revisões, lixeira e migração

- Cada arquivo tem uma revisão SHA-256 usada para detectar edição externa.
- Salvar com revisão divergente retorna conflito; a UI pede recarregar em vez de sobrescrever.
- Excluir move o item para `.reqlivo/trash/` em vez de apagar diretamente.
- Migrações de `schemaVersion` geram backup em `.reqlivo/backups/` e um aviso.
- `.reqlivo/` não entra na exportação de projeto.

## Compatibilidade

- O manifesto e os itens usam os campos canônicos do OpenCollection sempre que possível.
- Campos desconhecidos são preservados em blocos `extra` quando o parser consegue representá-los.
- Importações Postman preservam scripts não executáveis em `extensions.postmanScripts`, com relatório de perdas.
- A exportação OpenCollection é “bundled” (`bundled: true` + `items`) e deve ser importada com `reqlivo import opencollection`, não aberta diretamente como diretório.
