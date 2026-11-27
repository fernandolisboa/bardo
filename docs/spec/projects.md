# Projects spec

- Status: Proposed. Defaults chosen on 2026-11-26; each slice may revisit its own details before it starts.
- Date: 2026-11-26
- Parent issue: #332 (a sub-issue of the PRD, #4)
- Builds on: channels, personas and network accounts (ADR-0005), templates, the scene plan with a cast and places (#246, #258, ADR-0035), visual styles (#259), products (#273), localized versions (ADR-0023), backups and the settings file (ADR-0010, ADR-0014), costs and budgets
- Decision record: ADR-0047
- Screens: `docs/design/projects.md`

## Goal

Bardo is hard to find one's way in: every screen has its own channel picker, the libraries (personas, templates, products) mix everything the user ever made, the recurring people of a video cannot be reused in the next one, and nothing can be archived or deleted. This feature organizes Bardo around **projects**: the user opens one project at a time from a switcher next to the logo, every work screen shows only that project, each project keeps its media in its own folder on disk, and its world (characters, places, objects), narrators, products and templates belong to it or to a global library the user shares from on purpose.

## Principles

- **One project open at a time.** What a work screen shows is the open project's. Global items show where the project uses them and in the Global library. Jobs and Costs stay profile-wide, with a project filter.
- **Every piece of the user's work has one owner:** a project, or the global library. Nothing is owned by two projects, and no record points into another project. Reuse across projects is a reference to a global item or a copy. The profile-wide records (jobs, costs, budgets, rates, connections, tour progress, caches) are listed in ADR-0047.
- **Nothing becomes shared on its own.** Marking an item shareable, importing it into another project, exporting it, making it global: each is an action the user takes.
- **Only what the user chose appears.** A video uses the characters, places and objects picked for it. The scene planner may add extras of its own, never a library item that was not picked.
- **Physically separate.** A project's media lives in its own folder, which the user picks and can open in Explorer. Archiving and deleting work on a whole project or on one video.
- **A first step to fully separate storage.** The central database stays the source of truth, and every rule here keeps a project exportable as one self-contained package, so a later change can make that package the live storage without a redesign (ADR-0047).
- **Nothing irreversible without the user, and nothing paid twice.** Deleting asks for the name and goes to the Recycle Bin unless the user chooses permanent deletion; it is refused while something is scheduled or running. Moving or losing a folder never makes Bardo draw again what it already paid for.

## Vocabulary

Each slice adds its terms to `docs/CONTEXT.md` when it lands, and renames the ones it changes (*Project folder* becomes *Video folder*; *Persona* is shown as *Narrator*).

| Term | Meaning |
| --- | --- |
| **Project** | What the user opens to work: a name (unique per profile), a color, a folder on disk, one or more channels (none while new), its videos, its world, and its own narrators, products and templates. Owned by the user profile. *Active* or *archived*; deleting removes it. In code `Project`/`ProjectId`, table `project`; references to it are named `owner_project_id`, because `project_id` already means a video project in the schema. |
| **Open project** | The project the work screens show, chosen in the **project switcher** next to the Bardo logo and remembered per profile. *No project* is a choice too (see Loose videos). The switcher keeps the user's **favorites** (starred projects) and the **recent** ones at the top. |
| **Video** | What the user interface called a *project* until now: one video in production (code `VideoProject`, unchanged). The *Projects* screen becomes *Videos*. A video belongs to a project through its channel, or to none when it is a loose video. |
| **Video folder** | Where a video's media lives (until now *Project folder*): `Videos\<slug>-<short id>` inside its project's folder. |
| **Global library** | The library items owned by no project. Any project can use a global item by reference. A screen of its own. |
| **Library item** | A character, place, object, narrator, product or template: owned by a project or global, with the sharing rules below. Its name is unique per owner. |
| **World** | A project's characters, places and objects (the *World* screen, with a tab for each). |
| **Character** | Someone who can appear in a video's scenes (not a voice): a name, a fixed appearance, what they always and never do, their belongings, and reference portraits. The library form of a cast member (#246). Always an original person, never a real one. |
| **Place** | Somewhere scenes happen: a name, a fixed description and reference pictures. The library form of a scene plan's place (#258). |
| **Object** | Something that appears in scenes (a motorbike, a guitar): a name, a fixed description, optionally reference pictures, optionally the character it belongs to. |
| **Reference picture** | A character's portrait, or a place's or object's picture, kept with the item for one **look**: a catalog visual style, or *from a channel's notes* for that channel and the notes' text. A video reuses a reference picture of its own look for free. |
| **Narrator** | What the interface called a *persona*: who narrates or presents (voice reference, tone, script style, presets and an optional look for performed clips). Code keeps `Persona`. A narrator is not a character: a character has no voice, and a narrator does not appear in drawn scenes. |
| **Who appears** | A video's choice of characters, places and objects from the project's world and the global library, each with an instruction for that video only ("arrives on the motorbike in the first scene", "never speaks"). |
| **Shareable** | A mark on a project's library item that lets other projects import it. Off unless the user turns it on. |
| **Linked copy** | An item a project imported from another project that **follows** its original: read-only, it takes every change of the original. *Unlink* turns it into an independent copy. |
| **Independent copy** | An item copied from another project or from the global library that the project then changes on its own. |
| **Loose video** | A video with no project and no channel, made from the global library alone, for a quick try. Shown under *No project*. |
| **Project folder** | Where a project's media lives: `<base folder>\<folder name>` unless the user chose another place when creating it. Holds `Videos\`, `World\` and `bardo-project.json`. |
| **Base folder** | Where new projects and the global folder go: `Documents\Bardo` (`Documents\Bardo Dev` for a source build, as the data home does in ADR-0014) unless changed in Settings › Storage. The **global folder** `<base folder>\Global` holds the global library's pictures and `Loose videos\`. |
| **Project package** | A project exported to one file (`.bardo-project`): its records, copies of the global items it uses, and its media. Never keys, tokens or app credentials. |

## Navigation

The title bar shows `[logo] Bardo / <open project ▾>` and the number of its channels. The sidebar (Workspace layout) or the top tabs (Studio layout) show:

- **Project:** Overview, Ideas (Research and Themes as tabs), Videos, World, Narrators, Products, Templates, Channels and accounts (two tabs), Performance.
- **Global library** (tabs: World, Narrators, Products, Templates).
- Pinned as today: Jobs, Costs, Guide, Settings. Jobs and Costs show the whole profile, grouped by project, with a filter that starts on the open project.

When the open project has one channel, screens drop their channel picker; with more, the picker lists only the project's channels. **Overview** lists the project's videos in progress, each with its stage and its next step as a button that goes there, then the ideas waiting for the user and a summary of the world. The app opens on the last open project's Overview.

The **project switcher** shows a search box, then **Favorites** (starred projects, in the order the user sets), **Recent** (the last eight opened, most recent first, favorites left out), **All projects** (the rest, by name), *No project*, then: New project, Open a project from a folder, Import a project package, Archived projects; and for the open project: Show folder in Explorer, Project settings (name, color, folder, keys, budgets), Export project, Archive project, Delete project.

`docs/design/projects.md` describes the four screens this spec was designed with. The slice that changes the navigation updates `docs/design/` and every guide page and tour it moves (the coverage test in `crates/app/src/guide.rs`), in both layouts and all ten themes.

## Projects and the data that exists

- **Migration.** Channels linked by a localized version (ADR-0023), in either direction, become one project, named after the oldest of them; every other channel becomes a project of its own name. Each project owns its channels, their themes and videos. Existing personas, templates and products become global items, so every channel goes on using what it used. Existing cast members and places stay inside their scene plans.
- **Move a channel to another project** (Channels and accounts) takes the channel with its themes, videos, network accounts and media. It is refused while the channel has a scheduled publication or a queued or running job, or while one of its videos has a localized version in a channel that stays behind. A project left with no channel stays, empty, until the user deletes it.
- **Owners.** Channels belong to a project; themes and videos through their channel. Library items carry `owner_project_id`, NULL for global. `job` and `cost_record` gain `owner_project_id` and the names of the project, channel and video at the time, so Jobs and Costs can group and filter by project and keep names after a deletion. Paid work with no video (a world picture drawn from the World screen, a narrator look, a product research) records the project of the item it was for, none for a global item.
- **Localized versions** are offered only for the project's other channels. A version's video, cast and places stay in that project, so no version points into another project.
- **When it was last opened** and whether it is a favorite are kept per project. Archiving a project takes it out of Recent and Favorites; unarchiving gives its star back.
- **A new project** starts with a name, a color and a folder (the folder comes with the folders slice; until then a project has none). Overview then asks for a first channel; until there is one, Ideas and Videos wait for it.

## The project folder

- **Base folder.** `Documents\Bardo` (or `Documents\Bardo Dev` for a source build) unless changed in Settings › Storage. When the Documents folder lies under the folder of an `OneDrive`, `OneDriveConsumer` or `OneDriveCommercial` environment variable, the default becomes `%USERPROFILE%\Bardo` (or `Bardo Dev`) and Settings says why: syncing gigabytes of clips while they are written fails and fills the cloud quota. The user may still pick a synced folder, with that warning.
- **A project's folder** is chosen when it is created (the base folder by default). Its name is the project's name made safe for Windows: reserved names (`CON`, `NUL` and the rest), the characters `<>:"/\|?*` and trailing dots and spaces are replaced, and it is cut to 40 characters; when a folder of that name exists, the project's short id is added. Renaming a project never renames or moves its folder.
- **Inside it.** Videos live in `Videos\<slug>-<short id>`, the slug made the same way from the title when the video is created (cut to 30 characters) and never following later renames. World pictures live in `World\`. `bardo-project.json` names the project's id and name, the profile and the data home, so Bardo can recognize the folder and refuses one that belongs to another data home or profile.
- **Paths stay short.** Bardo checks when a project is created that its folder leaves room for its deepest file under the Windows path limit, and asks for a shorter place otherwise.
- **What stays in the database.** Narrator looks, product images and logos are small, bounded and already kept in the database so backups carry them (ADR-0010, #273); they stay there and travel inside a project package.
- **Moving the existing media.** A job moves each video's folder from `%LOCALAPPDATA%\Bardo\projects\<id>` to its video folder, with progress, cancel and resume: a rename on the same volume, otherwise a copy that is checked before the original is removed. While it runs, it holds the job runner and the background agent (the hold backups already use), so no job reads or writes a folder that is moving and no scheduled post starts from one.
- **Reading media** goes through the video's folder as the project's folder says, never through a fixed root.
- **A folder that cannot be reached** (moved, renamed, a drive not plugged in) marks the project *Folder not found* with *Locate…*, which accepts the folder whose `bardo-project.json` names this project. Until then no job runs for it and the agent skips it: a file in an unreachable folder is never taken as unreadable, so nothing is let go and drawn again (ADR-0035), and a scheduled post of that project waits and is reported as missed if its time passes.
- **Show folder in Explorer** opens the project's folder, or a video's.
- **Backups** (ADR-0010) carry every project folder and the global folder when *Include media* is ticked, each under its own entry with the place it came from. Restoring stages each folder beside where it goes (on the same volume) and swaps them all only once every one is staged; when a swap fails, those already swapped go back. A folder whose place is gone is restored under the base folder, and the project points to it. A backup from before this feature restores its media into the video folders of the projects the migration makes.

## World

- **Characters, places and objects** belong to a project or to the global library and are created on the World screen (or the Global library's World tab), before or after any video. A character can own objects (its belongings).
- **Reference pictures.** A video that shows an item with no picture for its look draws one just before the first scene that shows it, as cast portraits are drawn today (ADR-0035), and keeps it with the item for that look, so the next video with the same look reuses it for free. The user can also draw one from the World screen (paid, with the estimate, kept once the user reviews it) or import one for a place or an object (with the same confirmation as product images). A reference picture has its own provenance, owned by the item and not by the video that first needed it, so deleting that video leaves it; its cost is recorded under the video that asked, or under the item's project when drawn from the World screen.
- **Who appears.** Creating a video asks who and what appears, from the project's world and the global library, with an instruction per item for that video. The Scenes stage shows the choice and lets the user change it before planning; changing it after planning marks the plan for review.
- **The scene planner** receives the chosen items (name, fixed description, always and never notes, belongings, the video's instruction). It uses them as the plan's cast and places and may add extras of its own, which stay cast members or places of that video only, as today. It never adds a library item that was not chosen. A scene that shows a chosen character is drawn with that character's reference picture for the video's look (ADR-0035 still holds).
- **Saving to the world.** Any cast member or place of a video can be saved to the project's world or to the global library, with its picture for the video's look.
- **Editing an item's appearance** drops its reference pictures in every look; the screen says which looks and in how many videos before saving. Pictures already drawn keep theirs and read *old portrait*, as the cast does today. Editing its name, notes or belongings keeps the pictures.

## Library scope and sharing

These rules hold for every library item: characters, places, objects, narrators, products and templates (templates have the additions below).

- **Names** are unique per owner (each project, and the global library). Copying, importing or making global into an owner that already has the name asks for another, suggesting `<name> (<source project>)`.
- **Global by reference.** A project uses a global item directly; editing it changes it everywhere, and its screen shows where it is used before saving (as personas do today). A global item never enters a video that did not pick it.
- **Bring into the project** makes an independent copy of a global item, owned by the project.
- **Shareable.** A project's item is shareable when the user marks it.
- **Import from another project** lists the shareable items of the other projects (archived ones too) and imports each chosen one as a **linked copy** or an **independent copy**.
- **A linked copy** shows *Follows <item> from <project>* and cannot be edited. Every change of the original, by the user or by a job (a research, a drawn look), reaches its rows in the same transaction; its pictures follow by a copy that runs right after, and the copy shows *Updating* until they arrive (or *Waiting for its folder* while its project's folder cannot be reached). A linked copy in an archived project is kept up to date too. *Unlink* makes it independent with what it holds. When the original is deleted, unmarked as shareable, or its project deleted, every linked copy becomes independent and says so once.
- **Export to another project** copies an item into another project as an independent copy.
- **Make global** moves a project's item to the global library. The project keeps using it by reference, and every linked copy of it in other projects becomes a reference to the global item (its rows dropped, its uses pointed at the global one), so a copy never follows a global item.
- **Narrators.** A channel's default narrator, and a video's, can be the project's or a global one.
- **Templates.** A template kind has a global template and may have a project's own, which wins inside that project; the Templates screen shows, per kind, which the project uses. A copy, linked or independent, carries only the latest version of the original, as its first version; a linked copy takes each new version. *Make global* of a project's template adds its text as the next version of the global template, after a confirmation that lists the projects using the global one. Generations keep pointing at the version they used.
- **Item files.** Every library item can be saved to and opened from an item file (`.bardo-item`, versioned JSON, with its pictures except imported photos of real people); `.bardo-persona` files still open. Opening one adds an independent copy to the open project or to the global library.

## Loose videos

- **No project** in the switcher opens *Loose videos*: videos with no project and no channel, kept in `<base folder>\Global\Loose videos`.
- **A loose video starts from an idea the user types** (title and angle) and holds itself everything a channel would give it: language, country, format, length, visual style (or aesthetic notes), production mode, narrator (global), caption style, transition, motion, clip and take models, clip fit and stretch limit, and thumbnail look. They start from a channel the user picks to copy from, or from the defaults a new channel gets. Only global library items can appear in it.
- **Storage.** `video_project.channel_id` and `theme_id` become optional (a table rebuild), with the loose video's own title, angle and settings beside it.
- **Every stage works as in any video, except publishing:** a loose video exports files and cannot be uploaded or scheduled. It renders and exports for one **file target** per network: the network's built-in preset with no account behind it, never stored as a network account. A render keeps the target's id, so `render.account_id` no longer references `network_account` (a table rebuild; deleting an account deletes its renders by trigger).
- **Move to a project** is available at any time: the user picks the project and one of its channels, the video takes that channel's network accounts for publishing and keeps its own settings as overrides, and its folder moves into the project's. A language or country that differs from the channel's is shown before the move.
- **Write my own idea** joins Ideas in every project too: a theme the user types, ranked like any other; with more than one channel it asks which.

## Archive and delete

For a project and for a video.

- **What blocks both:** a publication scheduled in Bardo, a post held on a network to go public later (a YouTube upload with a publish time), or a queued or running job. Bardo lists them; the user cancels them, or for a network-held post, makes it public or removes it on the network.
- **Archive** hides it (from the switcher, or from the Videos list unless *Archived* is shown) and makes it read-only for the user: no job runs and nothing publishes. Metrics of its published posts go on syncing, and its network connections go on being renewed, so unarchiving brings it back as it was. *Unarchive* is in Archived projects, or in the Videos list.
- **Compress media** is offered when archiving a project: its videos' folders are packed into `Videos.zip` in the project folder, checked, and only then removed; unarchiving unpacks them first. `World\` is never packed.
- **Delete** asks the user to type the name. It removes the records and sends the folder to the Windows Recycle Bin. When the folder is on a removable or network drive, or larger than the room Windows reports in that drive's bin, the dialog says it will be deleted permanently instead. *Delete permanently* is also a choice in the dialog. Once project packages exist, deleting a project first writes its records into its folder, so a folder restored from the bin opens again with *Open a project from a folder*; before that, a restored folder is files only.
- **What deleting a project also does.** Its network accounts are disconnected as *Disconnect* does (tokens revoked where the network allows, and removed from Windows Credential Manager), and so are its own API keys. Linked copies of its items in other projects become independent. Global items it used are untouched.
- **A deleted video's theme** goes back to *approved* with no video, and may start a new one.
- **Never on the networks.** Deleting in Bardo never deletes a published post. A deleted video's first week leaves its channel's past performance.
- **Costs keep their history.** Cost records of a deleted project or video keep its name and short id, and Costs shows them as *<name> (deleted on <date>)*, grouped in one collapsible line *Deleted projects*. *Forget the name* replaces it with *Deleted project #<short id>*; the amounts never change.

## Export, import and opening a folder

- **Export project** writes a project package: a SQLite file with the project's records in the current schema, copies of every global item it uses, a manifest (app and schema versions, when, what it holds) and the media. Never keys, tokens, app credentials or the YouTube data that expires (ADR-0012).
- **Import a project package** adds it as a new project with a folder the user picks. A package of a newer schema is refused. Every record gets a new id, references inside records and job payloads included. The global items it carries come in as the project's own items, except templates, which come in as the project's own only when the user ticks them (so they never silently win over the global ones). Channels come in without network connections (the user signs in again); publications come in as history, and one whose post is already in the profile is left out.
- **Open a project from a folder** accepts a folder with a `bardo-project.json` of this data home and profile: a project Bardo knows (moved by the user) is relinked to it; a folder holding a deleted project's records is imported as a package.
- **The settings file** (ADR-0014) carries global narrators, and each channel with its project's name and the narrator it uses, project-owned ones included. Importing a channel whose project does not exist creates it under the base folder; a channel of the same name that already exists keeps its project. Narrators match global ones by name.

## Keys and budgets per project

- A project may have its own API key for any provider, set in Project settings and kept in Windows Credential Manager per project; without one it uses the app's key.
- A project with its own key for a provider has its own budget for it; its spending shows under the project in Costs and counts against that budget, not the app's.
- Keys, like tokens, never go into a project package, an item file or the settings file; deleting the project removes them; the restore checklist (ADR-0010) lists them by name.

## Not in this feature

- A database file per project as the live storage (the later step ADR-0047 keeps open).
- A project shared with other people or synchronized between machines. A project package moves one by hand.
- Sharing anything between projects without the user's action, or a global item entering a video without being picked.
- Deleting or editing posts on the networks.
- Characters who speak with a voice of their own; the narrator speaks.

## Acceptance

- After the migration every channel is in a project (channels linked by a localized version together), every existing video, theme, persona, template and product is where it was used, and nothing paid runs again, the media move included.
- With a project open, no work screen lists another project's channels, themes, videos or world items, and global items show only in the Global library and where the project uses them. Jobs and Costs filter by project.
- A character created in a project appears in a video only after being picked for it, and its portrait is drawn once per look.
- A linked copy cannot be edited and shows every change of its original; unlinking keeps its current content.
- A project whose folder cannot be reached runs no job and draws nothing again until it is located.
- Deleting a project sends its folder to the Recycle Bin (or deletes it, when the dialog said permanent), keeps its costs under its name, disconnects its accounts, and leaves its published posts online.
- A project package exported and imported on a clean profile opens with its videos, world and media, and with no key, token or app credential.
- Every new or moved screen has a guide page and a tour in both languages, in both layouts.

## Slices

In order, one at a time (they share migrations, the shell and i18n). Each is a sub-issue of #332. Each slice that adds a kind of record also extends delete, backup, the settings file and (once it exists) the project package for it.

1. #333 Projects: one per brand of channels, the switcher with favorites and recents, every screen scoped to the open project, moving a channel
2. #334 Navigation by project: Overview with next steps, Ideas, Videos, Channels and accounts, the Global library (after 1)
3. #335 Project folders on disk: base folder, a folder per project, existing media moved, backups and restore per folder (after 1)
4. #336 Archive and delete videos and projects, with costs that keep their names (after 3)
5. #337 World: characters picked per video, reference pictures kept per look (after 2 and 4)
6. #338 World: places and objects, and characters' belongings (after 5)
7. #339 Narrators, products and templates owned by a project or global (after 2 and 4)
8. #340 Sharing between projects: shareable items, linked and independent copies, make global, item files (after 6 and 7)
9. #341 Loose videos and "Write my own idea" (after 6 and 7)
10. #342 Export and import a project package, and open a project from a folder (after 8 and 9)
11. #343 API keys and budgets per project (after 10)
