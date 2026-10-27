# Reqlivo Demo

Coleção sintética usada para validar o MVP do [Reqlivo](../../README.md).

- Formato: OpenCollection 1.0.0 + extensões `reqlivo`.
- Requisições: `Echo`, `Status 404 esperado` e `Token secreto do processo`.
- Pastas: `Básicos` (auth Bearer herdada) e `Segredos`.
- Ambientes: `dev` (valores sintéticos) e `seguro` (segredo `apiToken` por referência).
- Estado local (cache, histórico, logs) fica em `.reqlivo/` e não deve ser versionado.
- Segredos são referências a variáveis de ambiente, nunca valores neste diretório.

## Executar

Em um terminal, a partir da raiz do repositório:

```bash
python3 examples/servidor_demo.py
```

Em outro terminal:

```bash
./target/debug/reqlivo --collection examples/demo validate
./target/debug/reqlivo --collection examples/demo run --env dev
REQLIVO_DEMO_TOKEN=valor-sintetico ./target/debug/reqlivo \
  --collection examples/demo run --env seguro
```

A execução em `dev` usa apenas valores sintéticos. Em `seguro`, a requisição
`Token secreto do processo` lê `REQLIVO_DEMO_TOKEN` do processo; sem a variável,
ela termina com erro de variável. O valor real do segredo não aparece no
relatório, no histórico, nos logs ou nas exportações padrão.

Para abrir a interface:

```bash
npm ci
npm run desktop:dev -- -- --collection examples/demo
```

Acesse `http://127.0.0.1:18787`.

## Exportar

```bash
./target/debug/reqlivo --collection examples/demo export curl req_4a2b3c4d5e6f7081
./target/debug/reqlivo --collection examples/demo export postman --output /tmp/demo-postman.json
./target/debug/reqlivo --collection examples/demo export opencollection --output /tmp/demo-oc.yml
./target/debug/reqlivo --collection examples/demo export project --output /tmp/demo-copia
```

Templates de variáveis são preservados; credenciais literais e segredos
referenciados não são materializados. No cURL exportado, substitua `{{baseUrl}}` e
`{{token}}` antes de executar.
