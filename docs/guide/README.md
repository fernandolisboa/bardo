# Bardo user guide

The guide Bardo shows on its Guide screen (F1), in two languages, also published at [fernandolisboa.github.io/bardo](https://fernandolisboa.github.io/bardo/). O guia que o Bardo mostra na tela Guia (F1), em dois idiomas, também publicado em [fernandolisboa.github.io/bardo](https://fernandolisboa.github.io/bardo/).

| English | Português |
| --- | --- |
| [What Bardo is](en-US/what-bardo-is.md) | [O que é o Bardo](pt-BR/what-bardo-is.md) |
| [Your API keys](en-US/api-keys.md) | [Suas chaves de API](pt-BR/api-keys.md) |
| [Your first video](en-US/first-video.md) | [Seu primeiro vídeo](pt-BR/first-video.md) |
| [Niche research](en-US/niche-research.md) | [Pesquisa de nichos](pt-BR/niche-research.md) |
| [Themes and ranking](en-US/themes-ranking.md) | [Temas e ranqueamento](pt-BR/themes-ranking.md) |
| [Performance and metrics](en-US/performance-metrics.md) | [Desempenho e métricas](pt-BR/performance-metrics.md) |
| [Projects and stages](en-US/projects.md) | [Projetos e etapas](pt-BR/projects.md) |
| [Script](en-US/script.md) | [Roteiro](pt-BR/script.md) |
| [Narration](en-US/narration.md) | [Narração](pt-BR/narration.md) |
| [Scenes and images](en-US/scenes.md) | [Cenas e imagens](pt-BR/scenes.md) |
| [Clips](en-US/clips.md) | [Clipes](pt-BR/clips.md) |
| [Personas](en-US/personas.md) | [Personas](pt-BR/personas.md) |
| [Templates](en-US/templates.md) | [Modelos](pt-BR/templates.md) |
| [The editor](en-US/editor.md) | [O editor](pt-BR/editor.md) |
| [Review and render](en-US/render.md) | [Revisão e render](pt-BR/render.md) |
| [Channels](en-US/channels.md) | [Canais](pt-BR/channels.md) |
| [Network accounts](en-US/network-accounts.md) | [Contas de rede](pt-BR/network-accounts.md) |
| [Network app credentials](en-US/app-credentials.md) | [Credenciais de app das redes](pt-BR/app-credentials.md) |
| [Connecting YouTube](en-US/connect-youtube.md) | [Conectar o YouTube](pt-BR/connect-youtube.md) |
| [Connecting Instagram](en-US/connect-instagram.md) | [Conectar o Instagram](pt-BR/connect-instagram.md) |
| [Connecting TikTok](en-US/connect-tiktok.md) | [Conectar o TikTok](pt-BR/connect-tiktok.md) |
| [Uploading and scheduling](en-US/uploading.md) | [Envio e agendamento](pt-BR/uploading.md) |
| [Exporting](en-US/exporting.md) | [Exportação](pt-BR/exporting.md) |
| [Missed posts](en-US/missed-posts.md) | [Posts perdidos](pt-BR/missed-posts.md) |
| [Costs and budgets](en-US/costs.md) | [Custos e orçamentos](pt-BR/costs.md) |
| [Syncing metrics](en-US/metrics-sync.md) | [Sincronizar métricas](pt-BR/metrics-sync.md) |
| [Jobs](en-US/jobs.md) | [Tarefas](pt-BR/jobs.md) |
| [Settings](en-US/settings.md) | [Configurações](pt-BR/settings.md) |
| [Where your data and keys live](en-US/data-and-keys.md) | [Onde ficam seus dados e chaves](pt-BR/data-and-keys.md) |
| [Troubleshooting](en-US/troubleshooting.md) | [Solução de problemas](pt-BR/troubleshooting.md) |
| [Glossary](en-US/glossary.md) | [Glossário](pt-BR/glossary.md) |
| [Keyboard shortcuts](en-US/shortcuts.md) | [Atalhos de teclado](pt-BR/shortcuts.md) |

## Writing a page

- One Markdown file per page, with the same file name in `en-US/` and `pt-BR/`. en-US sets the structure; pt-BR is written in natural Portuguese, not word for word.
- Front matter: `id` (the file name), `title`, `group` (`getting-started`, `strategy`, `production`, `editing`, `publishing`, `costs` or `reference`), the `place` the page explains, if any (`research`, `projects/script`, `settings/networks`), and the `tour` that shows it, if any (`welcome`, `research`, `themes`, `performance`, `projects`, `script`, `narration`, `scenes`, `clips`, `personas`, `templates`, `editor`, `render`, `channels`, `accounts`, `networks`, `publish`, `missed`, `costs`, `jobs`, `keys`, `appearance`, `metrics-sync`).
- A `# Title` equal to the front matter's title, an optional introduction, then sections: each `## Heading` right after an `<a id="section-id"></a>` line. Both languages have the same sections, in the same order, with the same ids.
- Links: another page as `page.md#section` (the app also reads `bardo:guide/page#section`, but only the first works on GitHub), a section of this page as `#section`, a place in Bardo as `bardo:go/<place>`, a tour as `bardo:tour/<tour>`. Anything else must be an `https://` address.
- Add a new page to `GUIDE_PAGES` in `crates/app/src/guide.rs`, which embeds it in the app. The tests in that file fail on a missing translation, a different section or a broken link.
- Every place (each screen, project stage, Settings tab and the Jobs panel) has a page whose `place` is it, and a tour. A new place without both fails the coverage test in the same file.

## The website

The `Guide site` workflow builds the site from these files on every pull request that touches them, and publishes it from main. To build it locally, with [mdBook](https://rust-lang.github.io/mdBook/) at the version the workflow pins:

```sh
cargo run -p bardo-app --example guide_site -- target/guide-site
mdbook build target/guide-site/en-US
mdbook build target/guide-site/pt-BR
# open target/guide-site/site/index.html
```

The sidebar is the Guide screen's contents, `bardo:go/` links lead to the page that explains the place, and `bardo:tour/` links keep their text with an "in the app" note. A broken link fails the build.
