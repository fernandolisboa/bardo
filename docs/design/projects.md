# Projects: screens

The four screens the projects spec (`docs/spec/projects.md`) was designed with, in the Workspace layout and a dark theme. Labels are shown in pt-BR, as the user sees them, with the en-US label in parentheses. The Studio layout places the same parts in its top tabs and status bar.

## 1. Overview of a project

- **Title bar:** the Bardo mark, `/`, the project switcher button (the project's color swatch, its name, a chevron), a chip with the number of channels, and on the right the project's folder path.
- **Sidebar**, top to bottom: section *Projeto* (Project) with Visão geral (Overview), Ideias (Ideas), Vídeos (Videos), Mundo (World), Narradores (Narrators), Produtos (Products), Modelos (Templates), Canais e contas (Channels and accounts), Desempenho (Performance); a divider; Biblioteca global (Global library); then pinned at the bottom: Tarefas (Jobs), Custos (Costs, with the month's spending), Guia (Guide), Configurações (Settings).
- **Content:** heading *Visão geral* and one line on what it shows. *Continuar de onde parou* (Pick up where you left off): one card per video in progress, with the title, a chip with its channel and format, its stage and progress ("Clipes, 7 de 14"), and the next step in the accent color with its estimate ("Próximo passo: animar as 7 cenas sem clipe, cerca de US$ 1,40"), with a primary *Continuar* (Continue) button. A video waiting for nothing says so ("Nada a fazer até a publicação") with a secondary *Abrir* (Open). Below, two cards: *Ideias esperando você* (ideas waiting for you: themes without a ranking, approved without a video) and *Mundo do projeto* (counts of characters, places, objects, and linked copies).
- **Aside:** the project's channels (name, language, country, networks), the project's spending this month, and the default narrator with where it comes from ("Global, usado em 3 projetos").

## 2. The project switcher, open

A popover under the switcher button, over the dimmed screen: a search field; *Favoritos*, *Recentes* and *Todos* groups, each row with the project's color, name and on the right its number of channels and when it was last opened, the open project highlighted; a divider and *Sem projeto* (No project) with the number of loose videos; a divider and *Novo projeto…*, *Abrir projeto de uma pasta…*, *Importar pacote de projeto…*, *Projetos arquivados* (with a count); a divider and a group *Este projeto* (This project): *Mostrar pasta no Explorer*, *Configurações do projeto*, *Exportar projeto…*, *Arquivar projeto…* and *Apagar projeto…* in the danger color. Each row in Favorites and Recent has a star to add or remove the favorite.

## 3. World › Characters

- **Header:** heading *Mundo* and one line: only what the user picks enters a video. Buttons: *Da biblioteca global…* (from the global library), *Importar de outro projeto…* (import from another project) and the primary *Novo personagem* (new character).
- **Tabs:** Personagens, Lugares, Objetos (Characters, Places, Objects), each with its count.
- **Collection:** a grid of cards, each with the reference picture (or the initials on the item's color until one is drawn), the name, a short description and chips: *Compartilhável* (shareable), *Tem: <object>* (belongings), *Segue <project> · só leitura* (linked copy), *Cópia independente de <project>*, *Global · em N projetos*.
- **Inspector** of the selected character: picture, name, owner and how many videos use it; *Aparência fixa* (fixed appearance), *Sempre* (always), *Nunca* (never), *Pertences* (belongings); a checkbox *Compartilhável com outros projetos*; buttons *Exportar para outro projeto…*, *Tornar global…* and *Salvar como arquivo…*. A linked copy shows its source and *Desvincular* (unlink) instead, and its fields read-only.

## 4. New video: who appears

- **Header:** breadcrumb *Vídeos › Novo vídeo*, the approved theme's title, and where it comes from.
- **Choices in a row:** channel (only the project's), narrator, format, visual style, each showing when it follows the channel.
- **Section *Quem e o que aparece*** (who and what appears), with the rule "the scene planner only uses what is ticked here; it may create extras, never a fixed character" and a *Novo personagem…* button. A table: a checkbox, the picture, the name with its owner (this project, follows another project, global) and a text field *Só neste vídeo* (this video only) for the instruction, enabled when ticked.
- **Below:** the places and objects as toggle chips, then *Cancelar* and the primary *Criar vídeo*.
