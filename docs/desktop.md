---
title: Desktop e releases
---

# Desktop Linux e Windows

**Implementado:** aplicativo híbrido Tauri 2, mantendo HTML/CSS/JavaScript na WebView e o núcleo Rust compartilhado com a CLI. A interface anterior pelo navegador e o comando `serve` foram removidos. Os alvos iniciais são Linux x86_64 e Windows x86_64.

## Instalação

Baixe os artefatos em [GitHub Releases](https://github.com/Andre-Ricardo/reqlivo/releases), quando uma versão desktop estiver publicada:

| Sistema | Artefato | Dependência de execução |
|---|---|---|
| Linux Debian/Ubuntu | `.deb` | WebKitGTK 4.1; instalado pelo gerenciador de pacotes |
| Linux | `.AppImage` | Sistema compatível com Ubuntu 22.04 ou posterior; pode exigir FUSE |
| Windows 10/11 | instalador NSIS `.exe` | WebView2, instalado pelo instalador quando necessário |

O arquivo `SHA256SUMS` acompanha a release. Não há assinatura de código nem atualização automática nesta etapa.

## Desenvolvimento

Requisitos: Rust estável recente, Node.js 22 com npm e as [dependências oficiais do Tauri](https://v2.tauri.app/start/prerequisites/). A CLI do núcleo pode ser compilada sem dependências gráficas.

Ubuntu/Debian, com privilégios administrativos:

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl wget file libssl-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf xdg-utils
```

No Windows, instale Visual Studio Build Tools com a carga de trabalho C++ para desktop e o SDK Windows, Rust com toolchain MSVC, Node.js 22 e WebView2. Execute os comandos abaixo no diretório do repositório:

```bash
npm ci
npm run desktop:dev
```

Para abrir uma coleção existente:

```bash
npm run desktop:dev -- -- --collection examples/demo
```

Sem argumento, o aplicativo reabre a última pasta escolhida em **Abrir coleção**. Se ela não estiver disponível, usa uma coleção inicial em `app_data_dir()/colecao`. O caminho selecionado fica em `app_data_dir()/last-collection.txt`. Uma pasta inválida escolhida pelo diálogo mantém a coleção anterior aberta.

Use **Abrir coleção** para trocar a pasta. O aplicativo pede confirmação antes de descartar um rascunho ou fechar uma janela com alterações. Para criar uma coleção em outra pasta, use `reqlivo --collection minha-api init --name "Minha API"` e selecione a pasta no aplicativo. Exportações de conteúdo usam diálogo nativo de salvamento.

## Arquitetura e permissões

- `src-tauri/`: janela, diálogos nativos, seleção da coleção e empacotamento.
- `src/assets/`: interface empacotada por `frontendDist`, sem servidor de desenvolvimento frontend.
- `src/desktop.rs`: ponte testável que executa operações `/api/*` em memória por `Router::oneshot`.
- `src/server.rs`: contratos de CRUD, execução, cancelamento, importação e exportação preservados; nenhum listener ou asset HTTP.
- O comando Tauri `api_request` aceita somente caminhos internos e GET/POST/PUT/DELETE. Não é um proxy HTTP. APIs de destino continuam no transporte Rust.
- A capability é restrita à janela local `main`, sem origens remotas nem plugins de shell/filesystem. Diálogos são chamados pelo Rust. A CSP permite IPC e assets locais.
- A troca de coleção reinicia cookies e cancela operações do estado anterior. A UI exige concluir/cancelar execuções antes da troca.

## Build e verificações

```bash
cargo fmt --all -- --check
cargo test --locked
cargo test --workspace --locked
python3 scripts/check-version.py
```

`cargo test --locked` testa somente o núcleo e a ponte IPC, sem GTK/WebKit. O comando com `--workspace` também compila o desktop e exige as dependências gráficas.

```bash
# Linux
npm run desktop:build -- --bundles deb,appimage -- --locked
# Windows
npm run desktop:build -- --bundles nsis -- --locked
```

Instaladores ficam em `target/release/bundle/`. O AppImage preserva caminhos relativos de `--collection` usando o diretório original `OWD` fornecido pelo runtime, mesmo quando o launcher altera o diretório de trabalho. O build não exige um servidor Node.js e o aplicativo instalado não depende de Node.js.

## Releases no GitHub

[Workflow desktop](../.github/workflows/desktop.yml):

1. PRs e pushes para `main` executam testes do workspace e geram instaladores Linux/Windows como artifacts da execução. O disparo manual faz o mesmo, sem publicar release.
2. Uma tag `v*` verifica que as versões em `Cargo.toml`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `package.json` e `package-lock.json` coincidem com a tag.
3. Somente após os dois builds passarem, o job com `contents: write` publica uma release com os três instaladores e checksums SHA-256. Uma reexecução substitui os assets da mesma tag.
4. Usa o `GITHUB_TOKEN` automático; não exige token pessoal ou certificado em secrets.

Para publicar a versão preparada, depois de integrar e validar o código:

```bash
git tag v0.2.0
git push origin v0.2.0
```

Para uma versão posterior, atualize os quatro manifestos e gere o lock npm com `npm install --package-lock-only`. Confira a consistência antes de criar a tag. O lock Cargo deve acompanhar as mudanças de versão dos pacotes Rust.

## Validação e lacunas

Os testes da ponte IPC verificam criação/persistência/reabertura/exclusão, erros estruturados e rejeição de caminhos externos. Os testes HTTP existentes continuam verificando o transporte e o cancelamento. O CI compila e empacota o desktop em Linux e Windows, inicia o AppImage no Linux e aguarda até trinta segundos por uma janela visível; no Windows, verifica o executável após dez segundos. No Linux, usa D-Bus e Openbox em uma sessão X11 isolada no Xvfb, exige janela visível e rejeita captura uniforme e registra a imagem como artifact `desktop-smoke-linux`.

**Observado em execução nesta feature:** 62 testes locais aprovados; `cargo check -p reqlivo-desktop --locked` e build debug aprovados. Os builds Linux e Windows geraram os três instaladores e abriram os executáveis. O `.deb` produzido pelo GitHub foi extraído e executado localmente: a coleção sintética apareceu na árvore e no seletor de ambientes, confirmando o carregamento inicial por IPC.

![Aplicativo Tauri no Linux com a coleção sintética](assets/reqlivo-desktop-linux.png)

Validação interativa ainda necessária em cada plataforma: primeiro uso, seleção/cancelamento de pasta, editar/salvar/enviar, cancelar execução, importar/exportar e fechar janela com rascunho. Build automatizado não equivale a teste visual ou execução de instalador. A execução local do desktop depende da instalação de GTK/WebKit no ambiente de desenvolvimento.

## Referências consultadas

Documentação local: [uso](uso.md), [MVP](mvp.md), [Tauri](tauri.md), [bibliotecas Rust](bibliotecas-rust.md). Consulta Context7: `/websites/tauri_app`, tópicos de comandos/estado/IPC e distribuição GitHub.

Documentação oficial: [comandos Rust](https://v2.tauri.app/develop/calling-rust/), [configuração](https://v2.tauri.app/reference/config/), [capabilities](https://v2.tauri.app/security/capabilities/), [diálogos](https://v2.tauri.app/plugin/dialog/), [GitHub Actions](https://v2.tauri.app/distribute/pipelines/github/).
