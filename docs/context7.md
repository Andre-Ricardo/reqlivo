---
title: Atualizar referências com Context7
---

# Atualizar referências com Context7

O [catálogo](tecnologias.md) foi preparado com consultas de resolução e documentação no Context7, seguidas de conferência das versões nas fontes oficiais. Os IDs e tópicos usados estão no [snapshot](tecnologias-versoes.json).

## Procedimento de manutenção

1. Leia `Cargo.toml`, `Cargo.lock` e os arquivos envolvidos para identificar a tecnologia e sua função real. Resolva a versão da dependência direta, sem confundi-la com uma versão transitiva de mesmo nome.
2. No Context7, resolva a biblioteca pelo nome oficial e pelo tópico específico. Escolha o resultado oficial mais relevante; não escolha uma versão antiga apenas pelo número de exemplos.
3. Consulte a documentação com o ID retornado, mantendo cada consulta focada em um conceito. Para uma API específica da versão instalada, escolha a versão correspondente quando o catálogo a oferecer.
4. Confira a release estável no registro oficial. Uma página indexada no Context7 não comprova que aquela versão é a mais recente.
5. Atualize a data, as versões e as fontes do snapshot e das páginas. Preserve a distinção entre documentação corrente e dependências em uso.
6. Verifique os links locais e a publicação do site. Mudanças documentais não substituem testes de compatibilidade necessários para uma futura atualização de dependências.

## Critérios adotados nesta consulta

- Rust: canal estável publicado pelo projeto Rust; `stable` nos links da documentação.
- Node.js: última release corrente e última LTS, registradas separadamente no índice oficial.
- Crates: `max_stable_version` do crates.io, com conferência do campo `yanked`.
- Tauri: documentação Tauri 2 e release estável do crate `tauri`; CLI, plugins e APIs JavaScript têm versionamento próprio.
- Web: referências correntes MDN e compatibilidade por API.
- Python: documentação publicada de Python 3; páginas da branch principal do CPython podem incluir recursos ainda não estáveis.
- OpenCollection: versão da especificação, separada da versão de ferramentas e bibliotecas auxiliares.
- GitHub Pages: versões suportadas pelo serviço, que podem diferir das releases mais novas do gerador upstream.

## Limites da consulta

As bibliotecas centrais têm tópicos efetivamente consultados via Context7. As bibliotecas auxiliares têm documentação oficial corrente e fixada catalogadas, com versões verificadas no crates.io; não houve uma consulta Context7 individual de cada crate auxiliar.

Os guias locais são sínteses de orientação, não cópias integrais dos manuais. Context7 pode retornar conteúdo histórico, de branches de desenvolvimento ou com defasagem de indexação. Se houver erro de conexão, registre e repita a consulta; se houver limite de quota, informe a limitação e autentique o CLI ou configure `CONTEXT7_API_KEY` antes de continuar. Nunca inclua credenciais nas consultas nem nos arquivos versionados.

[Voltar ao catálogo](tecnologias.md)
