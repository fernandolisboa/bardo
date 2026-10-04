# Onboarding: guided tours, the Guide and the user guide

- Status: Approved 2026-10-04 (#105 to #112)
- Built so far: #105 (tour engine, welcome tour, Guide menu, keyboard shortcuts), #106 (Guide screen, Getting started and Reference pages, `docs/guide/`), #107 (Research, Themes and Performance tours, "Tour this screen", Strategy pages), #108 (Projects, Personas and Templates tours, Script, Narration, Scenes and Clips stage tours, "Tour this stage", Production pages), #109 (the editor's tour in two parts, the Render stage tour, the guide over the editor, Editing pages), #110 (Channels, Accounts, Settings › Networks, Publish stage and missed posts tours, Publishing pages with the network setup guides in both languages)

Bardo teaches itself in three ways, all from the same place in the navigation, **Guide**:

1. **Guided tours**: the window dims, one component at a time is lit, and a card beside it says what it is and how to use it.
2. **The Guide screen** (#106): the user guide inside the app, read like a help center.
3. **The documentation** (#106, #112): the same guide as Markdown in `docs/guide/` and as a site on GitHub Pages.

Tours run on the user's own data. There is no sample project: a step whose component is not on screen (no projects yet, a stage still locked) falls back as the step says (see [Missing components](#missing-components)).

## Guided tour

### What is on screen

- **Scrim**: the content area (everything under the title bar) dimmed by the theme's `scrim` color, drawn as four rectangles around the lit component so the component itself stays at full contrast. Each theme sets its own `scrim` in `crates/app/data/themes.json` (`#RRGGBBAA`); the high contrast themes dim harder.
- **Ring**: 2 px in the theme's focus color around the lit component, 4 px out, with the theme's corner radius (square in the terminal themes). 3 px in the high contrast themes.
- **Card**: at most 360 px wide, on the side of the lit component the layout leaves open (right of the Workspace sidebar, below the Studio top bar), flipped to the other side when there is no room, then tried on the other axis, and kept 12 px inside the window. It shows "n of N" and "Esc closes", a title, at most three sentences, and Skip tour, Learn more (when the step has a guide section), Back (not on the first step), Next (Finish on the last). The terminal themes draw it in their monospace font, like everything else.
- The light slides to the next component in 150 ms (ease out). A component inside a scroll (the Workspace sidebar, the Studio tabs) is scrolled into view first.

Every click outside the card is absorbed: a tour never changes data, and nothing under the scrim can be clicked by accident. Jobs keep running underneath.

### Keyboard

| Key | Does |
| --- | --- |
| → or Enter | Next (Enter presses the card button the keyboard is on) |
| ← | Back |
| Esc | Closes the tour and keeps the step for "Resume tour" |
| Tab / Shift+Tab | Moves between the card's buttons |

The card takes the keyboard when it appears and gives it back where it was when the last card closes.

### Ending a tour

- **Finish** (last step): the tour is done; the user stays where the tour left them.
- **Skip tour**: the tour ends for good (no "Resume tour"); the user goes back to the screen they were on when it started.
- **Esc**: the tour closes midway; the Guide menu offers **Resume tour**, which goes back to that step. If the tour's content changed in between, it starts over.

### The missed posts list comes first

A tour does not start while the missed posts list is open, and if the list opens during a tour, the tour waits behind it and comes back on the same step once the list closes.

The list's own tour is the one exception: it runs over the list (`TourPlace::Missed`). **Tour this list** in the list's title, or Shift+F1 while the list is up, starts it. Its **Learn more**, and F1, put the list off as **Decide later** does, so the guide shows. Started from its guide page with the list closed, it brings back the posts put off this session; with no missed posts, its cards show in the middle.

### Steps

A step names:

- its **anchor** (the component it lights), or none for a card in the middle of the window;
- optionally a **place** to open first (a screen, or a stage of the open video project), so its anchor is on screen;
- its title and body texts, `tour.<tour>.<step>.title` and `.body` in both locale files;
- the side its card prefers (or the side the layout leaves open);
- what to do when its anchor is missing.

#### Missing components

| When missing | Does |
| --- | --- |
| Skip | Passes over the step in the direction the user was going (forward, or back when Back led here; at the first step it turns forward) |
| Light the part | Lights the part that holds the component instead (the collection that holds an empty list's first row) |
| Center | Shows the card in the middle with nothing lit |

### Anchors

A **tour anchor** names a component a tour can light. Anchors live in `bardo_app::TourAnchor` (no GPUI), and the UI tags what it draws:

- **The layouts tag the parts**: each navigation group by pillar, each place (pinned ones included), the header, the stages, the toolbar, the collection, the inspector and the page content. Each layout tags them where it draws them, with the side it leaves open and the scroll that holds them.
- **Screens tag their own controls** with `kit::anchor(TourAnchor::Control(Control::…), element)`. A control is drawn by its screen in both layouts, so the test checks that its screen tags it rather than each layout.

A tag adds an invisible child that records the element's bounds while the window prepaints, into a per-window map rebuilt every frame. The spotlight is drawn deferred, above everything else (popovers and the missed posts list included), after the whole window has prepainted, so it reads the bounds of the same frame: resizing the window, or switching the layout, theme or language mid-tour, only moves the light.

A test checks that every anchor a tour step uses is tagged in every layout.

### Progress

Per profile and tour, `tour_progress` (migration 0035) keeps the **content version** the user saw, the state (offered, in progress, completed, dismissed), the last step and when it changed. Raising a tour's content version shows it as **New** in the Guide menu; it never restarts a tour on its own. Reading and saving it are best-effort: a failure is logged, the tour goes on, and progress that cannot be read counts as none.

## The welcome tour

The first time Bardo opens on a profile (and once for profiles that existed before tours), a card in the middle, without the scrim, asks **Take a 2-minute tour?** with **Start the tour**, **Not now** (asked again next time Bardo opens) and **Don't show again** (never asked again; the tour stays in the Guide menu).

Eight steps, each lighting the navigation, which every screen shows:

| # | Lights | Says |
| --- | --- | --- |
| 1 | Nothing (centered) | Welcome; how to move and leave |
| 2 | Strategy group | Research, themes, performance |
| 3 | Production group | Projects, personas, templates |
| 4 | Publishing group | Channels and their accounts |
| 5 | Settings | Bring your own API keys, kept in Windows Credential Manager |
| 6 | Jobs | Long work runs as jobs; keep working meanwhile |
| 7 | Costs | Spend and budgets |
| 8 | Guide | Where tours, shortcuts and the guide live |

## The Guide place

**Guide** is a pinned place next to Jobs, Costs and Settings: at the foot of the Workspace sidebar, between Jobs and Settings on the right of the Studio top bar. It opens a menu over the screen (the screen stays):

- **User guide** (F1): opens the Guide screen;
- **Welcome tour**, marked **New** until it is started or turned down, and again when its content changes;
- **Tour this screen**, when the current screen has a tour and something to show (below);
- **Tour this stage**, on a project whose open stage has a tour and has made something (below);
- **Resume tour**, when a tour was closed midway;
- **Keyboard shortcuts**: the tour's, the lists', the editor's and the cut suggestions' keys, from `bardo_app::SHORTCUTS`;
- **Reset tours**: every tour reads as never seen, and the welcome offer comes back.

Shift+F1 starts the stage tour when one is offered, else the screen tour, and the menu shows the key on that row. The menu takes ↑/↓, Tab, Enter and Esc; a click outside closes it.

## Screen tours

A screen with a tour of its own (Research, Themes, Performance, Projects, Personas, Templates, Channels and Accounts) shows **Tour this screen** in its header, after the ⓘ, and the Guide menu lists it. A tour never starts on its own:

- **Nothing to show, no tour**: while the screen is empty (no research results, no themes, no posts), the button and the menu row are hidden and Shift+F1 does nothing; the screen's empty state says what to do first. The guide page's **Show me** still starts the tour, and steps fall back as they say.
- **The "new" mark**: from the first visit with content, the button and the menu row carry **New** until the tour is completed or dismissed (a tour closed midway keeps it), and again when the tour's content version rises. These rules are `app` logic (`Bardo::screen_tour`).
- **Offer tours on new screens** (Settings › Appearance, on by default): turned off, the mark never shows; the button stays.
- **Reset tours** brings the mark back on every screen tour.

### Stage tours

The Script, Narration, Scenes and Clips stages of a project have tours of their own. The project header then shows **Tour this stage** beside **Tour this screen** (the Projects tour: switcher, narrator, stages, unlocking, cost), with the same **New** mark and the same rules, where "something to show" means the stage has made something: a script, a narration, a scene plan, a clip. A stage that has made nothing offers no tour, and its empty state says what to do first. The screen says whether its stage has made something; the offer and the mark are `app` logic (`Bardo::stage_tour`); starting a stage tour from the guide opens the stage first.

A step whose control is out of view scrolls the page to it; a step whose control is missing (no scene selected, no narration yet) lights the part that holds it, or is passed over when the step says so.

Each ⓘ on a screen with a guide page ends with **More in the guide**, which opens the section that explains it.

### The editor and the Render stage

The editor covers the whole window, so it carries its tours itself: **Editor tour** in its top bar (Shift+F1), with the **New** mark, once the cut has something in it. Its controls are tagged by the editor (the preview, the tracks, the tools, Snap to words, undo, the audio headers, ducking, captions, the 16:9 and 9:16 switch, cut suggestions, Review & render).

- It comes in two parts, so neither runs past seven steps: playback, the tracks, cutting, snapping and undo; then the mix, ducking, captions, framing, cut suggestions and leaving to render. The button starts the first part the user has neither completed nor dismissed (and the first again once both are), with the part's name as its label.
- Over the editor, the Guide shows the tour's cards alone: no first-run offer, menu or shortcuts card, and the missed posts list (not drawn over the editor) does not hold a tour back.
- Over the editor the Guide menu, with Resume tour, is out of reach, so the editor's button (and Shift+F1) takes a tour closed midway back to the step it closed on (`Bardo::continue_tour`).
- Every step holds the editor: playback pauses when a step or the guide shows, and nothing else changes; a test walks both parts and checks the cut, its undo history and the selection.
- F1 in the editor opens the Guide screen over it, at the editor's page, under a bar with **Back to the editor**, which returns to the editor as it was. A tour card's **Learn more** does the same. A guide link to another place, or another tour (the welcome tour included), closes the editor first.

The Render stage has a tour like the other stages ("something to show" is a cut to review): the figures (a new part anchor, tagged where each layout draws them), the targets, the picked target's checks, the toolbar and its last file.

### Publishing

- **Channels** and **Accounts** offer their tours once there is a channel, and a channel with network accounts. The Accounts tour lights the first account's card, its Edit, its preset and the first connection; Connect and Add are passed over when there is nothing to connect or add.
- **Settings › Networks** has a tab tour (`TourPlace::Settings`): **Tour this screen** shows in the Settings header on that tab only, and Shift+F1 starts it there. Starting it from the guide opens the tab first.
- The **Publish** stage offers its tour once the channel has networks to post on. Its steps only explain: the review's scheduling row is lit when the review is open, the upload otherwise; nothing is uploaded, exported, scheduled or linked.

## The Guide screen

The user guide inside the app, built from the same screen parts as every other screen, so each layout places it like the rest:

- **Collection**: a search box over the contents, grouped by pillar (Getting started, Strategy, Production, Editing, Publishing, Costs and budgets, Reference; empty groups are left out). While a query is typed, the collection lists the matching sections instead, as "Page › Section" with a snippet around the first match.
- **Content**: the page, rendered from Markdown. Links to other pages and sections open them and scroll to the section; `bardo:go/` links go to a place; `bardo:tour/` links start a tour; web links open in the browser.
- **Aside**: "On this page" with a link to each section, **Show me** when the page has a tour, and **Go to <place>** when it explains one.

Keys: F1 from any screen (over the editor too, see above) opens the page for the current screen, project stage or Settings tab, else the first page, with the search box focused. ↑/↓ move through the contents or the results, Enter opens the highlighted result, Esc clears the search. During a tour, F1 is the step's **Learn more**.

Tour cards whose step names a guide section show **Learn more**: it closes the tour (Resume tour brings it back) and opens that section.

The pages live in `docs/guide/<language>/<page>.md`; `docs/guide/README.md` says how to write one. The app embeds them when it is built, and its tests fail on a page missing in one language, a section that differs, a broken link or a tour step pointing to no section. A coverage report lists the places no page explains yet; #111 makes it fail.

## Later slices

| Issue | Adds |
| --- | --- |
| #107 | The Research, Themes and Performance tours and "Tour this screen" (built) |
| #108 | The Projects, Personas and Templates tours, the Script, Narration, Scenes and Clips stage tours and "Tour this stage" (built) |
| #109 | The editor's tour, the Render stage tour and the guide over the editor (built) |
| #110 | The Publishing tour (network guides in pt-BR too) |
| #111 | Costs, Jobs and Settings tours, and full coverage |
| #112 | The documentation site on GitHub Pages |

The user guide has one source, `docs/guide/<language>/`, read by the Guide screen and published as the site, so the app and the site never drift apart.
