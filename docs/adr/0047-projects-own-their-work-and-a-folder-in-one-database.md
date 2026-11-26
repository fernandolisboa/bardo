# ADR-0047: Projects own their work and a folder, in one database

- Status: Accepted
- Date: 2026-11-26
- Issue: #332
- Spec: `docs/spec/projects.md`
- Amends: ADR-0005 (personas, templates and products may belong to a project as well as to the profile), ADR-0010 (media is backed up and restored per project folder, not from one `projects` folder), ADR-0014 (the base folder follows the data home, as project folders did), ADR-0023 (a localized version is offered only for the project's own channels)
- Builds on: the cast and places of a scene plan (#246, #258, ADR-0035), products (#273)

## Context

Everything in Bardo belongs to the user profile. Each screen picks a channel on its own, the libraries (personas, templates, products) hold everything ever made, the people and places of a video cannot be reused in the next one, and no channel, persona or video can be archived or deleted. It is hard to tell where things are and what comes next.

The goal is to open one *project* at a time, as an IDE opens a repository: its work physically separate on disk, easy to archive or delete, with characters, narrators and prompts of its own, and a global library to share from on purpose.

Two storage designs were weighed:

- **A database per project** inside the project's folder, so copying the folder takes everything. Backups, the settings file, the background agent, costs, budgets and the job queue all read across the whole profile today and would each need rework, and every row would move between files.
- **The central database stays, and each project gets a folder for its media.** Separation on disk, archive and delete, at a fraction of the risk. Performance decides nothing either way: SQLite handles the rows of many projects with indexes, and the heavy media is already in files.

## Decision

1. **A project is a new aggregate owned by the user profile**: a name unique per profile, a color, a folder, one or more channels (none while new), whether it is a favorite and when it was last opened, and an *active* or *archived* state. In code `Project`, table `project`. References to it are named `owner_project_id`, because `project_id` already means a video project across the schema. The user interface calls a video project a *video* and a persona a *narrator* from now on.
2. **The user's work has exactly one owner**: a project or the global library. Channels belong to a project, and themes and videos through their channel; library items (characters, places, objects, narrators, products, templates) carry `owner_project_id`, NULL for global, with names unique per owner. A loose video belongs to none and has no channel. **Profile-wide by design:** jobs and cost records (which gain `owner_project_id` and name snapshots of project, channel and video, to group, filter and survive deletions), budgets, rates, network connections' app credentials, tour progress, preferences and caches (niche research, voice samples).
3. **One database stays the source of truth.** Media moves out of `%LOCALAPPDATA%\Bardo\projects` into each project's folder (`Videos\<slug>-<short id>`, `World\`), and the global library's into `<base folder>\Global`. The base folder follows the data home (`Bardo`, `Bardo Dev`). Narrator looks, product images and logos stay in the database, as they are small and bounded. A folder that cannot be reached stops the project's jobs instead of reading as missing files, so nothing paid is drawn again.
4. **The door to a database per project stays open.** No record of a project references another project's records: reuse across projects is a reference to a global item or a copy, and localized versions stay inside a project. A project package (`.bardo-project`) holds the project's records in a SQLite file of the current schema, copies of the global items it uses, and its media, so turning that package into the live storage later changes where rows are read, not the model.
5. **Sharing is explicit and of two kinds.** A project's library item is shared only when the user marks it *shareable*. Another project imports it as a **linked copy** (read-only; its rows written in the same transaction as every change of its original, by the user or by a job; its pictures copied right after) or an **independent copy**. *Unlink*, and the loss of the original (deleted, unmarked, its project deleted), make a linked copy independent. *Make global* moves an item to the global library and turns linked copies of it into references, so a copy never follows a global item. Templates copy only their latest version.
6. **A video uses only the library items picked for it.** The scene planner gets the picked characters, places and objects and may add extras of its own; it never adds an unpicked library item. Reference pictures belong to the item, keyed by look (a catalog style, or a channel's notes with their text), so a look draws each once and deleting a video never takes them away.
7. **Deleting goes to the Recycle Bin by default**, asks for the name, says beforehand when Windows would delete permanently instead, and offers permanent deletion. It is refused while a publication is scheduled (in Bardo or held on a network) or a job is queued or running. It disconnects the project's network accounts and removes its keys. Cost records keep the name and a short id, which the user may forget; amounts never change.
8. **Migration.** Channels linked by a localized version become one project; every other channel becomes a project of its name. Existing personas, templates and products become global; scene plans keep their cast and places; media moves into the project folders by a resumable job that holds the job runner and the background agent.

## Consequences

- The open project scopes every work screen, so the per-screen channel pickers go away for projects with one channel, and the navigation is rebuilt around the project, in both layouts, with its guide pages and tours. Jobs and Costs stay profile-wide with a project filter.
- Global items are used by reference, so editing one shows where it is used before saving, as personas already do.
- A linked copy duplicates its original's rows and pictures rather than pointing at them, so a project package, and a later database per project, read it without the other project.
- Backups walk every project folder and the global folder; a restore stages each folder on its own volume and swaps them all, or none.
- Deleting a video returns its theme to *approved*; the theme may start another video.
- Moving a channel between projects carries its themes, videos, accounts and media, and is refused while localized versions would cross projects.
- A project's own API keys (Windows Credential Manager, per project) and budgets come last, and fall back to the app's.
