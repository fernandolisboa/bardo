# ADR-0048: A project package is the project's records and media, brought in under new ids

- Status: Accepted
- Date: 2026-11-27
- Issue: #342
- Spec: `docs/spec/projects.md` (Export, import and opening a folder)
- Builds on: ADR-0047 (item 4, the door to a database per project), ADR-0010 (backup and restore), ADR-0038 (sharing library items)

## Context

ADR-0047 promised a `.bardo-project` package: the project's records in a SQLite file of the current schema, copies of the global items it uses, and its media. It also has to bring a deleted project back when its folder is restored from the Recycle Bin, and import a package into the profile it came from without clashing with the original.

The schema has more than a hundred tables and grows with every slice. A hand-kept list of what a project owns would fall behind the next migration silently.

## Decision

1. **The package is a zip** of `bardo-package.json` (format, app version, schema version, when, the project's name and color, and what it holds), `records.db` (the project's rows in a fresh database of the exporting schema) and the media: each video's folder, the project's `World` folder and the global world pictures its carried items use. It is written by a job with progress and cancel, and a newer schema is refused on import.
2. **The rows are chosen from the schema, not from a list.** Starting from the project row, every table whose foreign key reaches a chosen row with `ON DELETE CASCADE`, or whose `owner_project_id` names the project, is followed until nothing new comes in. The global library items the project uses (narrators, products, characters, templates) come along with their own parts; another project's item never does, even when a moved channel still points at it. A short list of tables stays out on purpose: other projects' and profile-wide data (costs, budgets, rates, tour progress), the job queue except finished jobs, network connections and anything else that holds a secret or a token, YouTube data that the API terms let an app keep only for a while (niche research, theme rankings), deleted work and staging. A test reads the package and fails if a key, token or app credential got in.
3. **Every id is new on import.** Every UUID in the package that names one of its rows is replaced with a fresh one wherever it appears: in key columns, in other text columns, in JSON job payloads and plans, and in the names of media files and folders. A UUID that names nothing in the package (a global template kept as a reference, a network post id) is left alone. Importing into the profile the package came from therefore makes a second, independent project.
4. **Carried items become the project's own.** Global narrators, products and characters the package carries become the new project's. Carried global templates do so only when the user ticks their kind; an unticked one whose version exists in this profile stays a reference to it, and one that does not exist comes in as the project's own, retired, so the texts written with it keep their history. Linked copies come in independent.
5. **What would clash is renamed or left out.** The project's name is numbered when taken; a channel whose name is taken gets the project's name after it; carried items are numbered within the new project. A publication the profile already has (same network and post id) is left out, and only posts that went out (or were marked posted by hand) come at all, as history.
6. **Channels come without network connections**, and with no mirror of another project's linked items.
7. **Deleting a project to the Recycle Bin first writes its records** into its folder (`bardo-project.db`, the same records file as the package) and refuses the deletion if that fails. *Open a project from a folder* relinks the folder of a project this profile knows, or, for a deleted project's folder holding records, imports it in place with new ids and removes the records file. A folder from another data home or profile is refused; its package is the way across.
8. **The import writes nothing until the end.** The media is unpacked into the new project's folder, then the rows go in one transaction with foreign keys checked before commit; on a failure or a cancel the folder is removed and no project appears. A run cut short by closing Bardo leaves the folder, and the job's next run starts over in it, as the folder names that import's project.
9. **A package is untrusted input.** On import the same rules apply again as on export: one project, nothing of the tables that never travel (an unfinished job would run and pay here), the same tidying, and every file name that is later joined to a folder must stay inside it, or the package is refused as damaged.
10. **A deleted project whose media was compressed comes back archived**, so *Unarchive* unpacks `Videos.zip` as it does for any archived project. Opening a deleted project's folder renames the files whose names carry an id and rewrites `bardo-project.json` before the records commit, and puts both back if anything fails.

## Consequences

- A migration that adds a table owned by a project is carried by packages without touching the package code; one that adds a table that must stay out has to join the list, which the package tests' secret check and the reviewer catch.
- Rewriting UUIDs inside free text means a UUID typed by the user into a field that happens to equal a package row's id would be rewritten. That is accepted: ids are random and never shown.
- Costs stay with the profile that spent them: an imported project starts with no spending, and a restored deleted project keeps its spending under its old name in Costs.
- A package is not a backup: it holds one project and never keys. Backups (ADR-0010) stay the way to move a whole profile.
- Opening a deleted project's folder is undone on any error, but not on a crash between renaming its files and committing its records: the folder then names a project that never came. The window is short and accepted; the files are still there to recover by hand.
