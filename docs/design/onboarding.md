# Onboarding: guided tours, the Guide and the user guide

- Status: Approved by the owner, 2026-10-04 (#105 to #112)
- Design proposal: https://claude.ai/artifact/2XhWoYxS71mW1ieQUSZfyC
- Built so far: #105 (tour engine, welcome tour, Guide menu, keyboard shortcuts)

Bardo teaches itself in three ways, all from the same place in the navigation, **Guide**:

1. **Guided tours**: the window dims, one component at a time is lit, and a card beside it says what it is and how to use it.
2. **The Guide screen** (#106): the user guide inside the app, read like a help center.
3. **The documentation** (#106, #112): the same guide as Markdown in `docs/guide/` and as a site on GitHub Pages.

Tours run on the user's own data. There is no sample project: a step whose component is not on screen (no projects yet, a stage still locked) falls back as the step says (see [Missing components](#missing-components)).

## Guided tour

### What is on screen

- **Scrim**: the content area (everything under the title bar) dimmed by the theme's `scrim` color, drawn as four rectangles around the lit component so the component itself stays at full contrast. Each theme sets its own `scrim` in `crates/app/data/themes.json` (`#RRGGBBAA`); the high contrast themes dim harder.
- **Ring**: 2 px in the theme's focus color around the lit component, 4 px out, with the theme's corner radius (square in the terminal themes). 3 px in the high contrast themes.
- **Card**: at most 360 px wide, on the side of the lit component the layout leaves open (right of the Workspace sidebar, below the Studio top bar), flipped to the other side when there is no room, then tried on the other axis, and kept 12 px inside the window. It shows "n of N" and "Esc closes", a title, at most three sentences, and Skip tour, Back (not on the first step), Next (Finish on the last). The terminal themes draw it in their monospace font, like everything else.
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
- **Screens tag their own controls** with `kit::anchor(TourAnchor::…, element)` (later tours).

A tag adds an invisible child that records the element's bounds while the window prepaints, into a per-window map rebuilt every frame. The spotlight is drawn deferred, above everything else (popovers and the missed posts list included), after the whole window has prepainted, so it reads the bounds of the same frame: resizing the window, or switching the layout, theme or language mid-tour, only moves the light.

A test checks that every anchor a tour step uses is tagged in every layout.

### Progress

Per profile and tour, `tour_progress` (migration 0035) keeps the **content version** the user saw, the state (offered, in progress, completed, dismissed), the last step and when it changed. Raising a tour's content version shows it as **New** in the Guide menu; it never restarts a tour on its own. Saving progress is best-effort: a failure is logged and the tour goes on.

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

- **Welcome tour**, marked **New** when never taken or changed since;
- **Resume tour**, when a tour was closed midway;
- **Keyboard shortcuts**: the tour's, the lists', the editor's and the cut suggestions' keys, from `bardo_app::SHORTCUTS`;
- **Reset tours**: every tour reads as never seen, and the welcome offer comes back.

The menu takes ↑/↓, Tab, Enter and Esc; a click outside closes it.

## Later slices

| Issue | Adds |
| --- | --- |
| #106 | The Guide screen, "Getting started", and `docs/guide/` in pt-BR and en-US from one source |
| #107 | The Strategy tour and a "Tour of this screen" button |
| #108 | The Production tour |
| #109 | The Editing and render tour |
| #110 | The Publishing tour (network guides in pt-BR too) |
| #111 | Costs, Jobs and Settings tours, and full coverage |
| #112 | The documentation site on GitHub Pages |

The user guide has one source, `docs/guide/<language>/`, read by the Guide screen and published as the site, so the app and the site never drift apart.
