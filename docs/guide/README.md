# Bardo user guide

The guide Bardo shows on its Guide screen (F1), in two languages. O guia que o Bardo mostra na tela Guia (F1), em dois idiomas.

| English | Português |
| --- | --- |
| [What Bardo is](en-US/what-bardo-is.md) | [O que é o Bardo](pt-BR/what-bardo-is.md) |
| [Your API keys](en-US/api-keys.md) | [Suas chaves de API](pt-BR/api-keys.md) |
| [Your first video](en-US/first-video.md) | [Seu primeiro vídeo](pt-BR/first-video.md) |
| [Glossary](en-US/glossary.md) | [Glossário](pt-BR/glossary.md) |
| [Keyboard shortcuts](en-US/shortcuts.md) | [Atalhos de teclado](pt-BR/shortcuts.md) |

## Writing a page

- One Markdown file per page, with the same file name in `en-US/` and `pt-BR/`. en-US sets the structure; pt-BR is written in natural Portuguese, not word for word.
- Front matter: `id` (the file name), `title`, `group` (`getting-started`, `strategy`, `production`, `editing`, `publishing`, `costs` or `reference`) and, when the page explains one, the `place` (`research`, `projects/script`, `settings/keys`) and its `tour` (`welcome`).
- A `# Title` equal to the front matter's title, an optional introduction, then sections: each `## Heading` right after an `<a id="section-id"></a>` line. Both languages have the same sections, in the same order, with the same ids.
- Links: another page as `page.md#section`, a section of this page as `#section`, a place in Bardo as `bardo:go/<place>`, a tour as `bardo:tour/<tour>`. Anything else must be an `https://` address.
- Add a new page to `GUIDE_PAGES` in `crates/app/src/guide.rs`, which embeds it in the app. The tests in that file fail on a missing translation, a different section or a broken link.
