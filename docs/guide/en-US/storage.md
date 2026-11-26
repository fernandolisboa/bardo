---
id: storage
title: Storage
group: reference
place: settings/storage
tour: storage
---

# Storage

[Settings › Storage](bardo:go/settings/storage) chooses the **base folder**, where each new project gets its folder. [Show me the tab](bardo:tour/storage).

<a id="base-folder"></a>
## The base folder

The base folder is `Documents\Bardo` unless you choose another one with **Change…**. **Use the default** goes back to it. Projects you already have keep their folders: the base folder only decides where new ones go.

When OneDrive syncs your Documents folder, the default is your user folder instead (`C:\Users\<you>\Bardo`), and the tab says why: syncing gigabytes of clips while Bardo writes them fails and fills your cloud storage. You may still pick a synced folder; the tab then warns you.

A copy of Bardo built from source uses `Bardo Dev` in place of `Bardo`, as it does for its other data ([A copy built from source](data-and-keys.md#source)).

<a id="project-folder"></a>
## A project's folder

Each project has a folder, chosen when you create it ([A new project](your-projects.md#new)). Its name is the project's name, with the characters Windows does not allow in a folder name replaced and cut to 40 characters; when a folder of that name is already there, a short code is added. Renaming the project never renames or moves its folder. Inside it:

- `Videos`, with a folder per video, named after its title when it was created and a short code. It holds the narration, images, clips, music, imported files, the editor's preview copies and the renders.
- `World`, for the project's pictures.
- `bardo-project.json`, which tells Bardo whose folder it is.

Bardo refuses a place so deep that the files inside would pass the Windows path limit, and asks for a shorter one. **Show folder** in the [project switcher](your-projects.md#switcher), and on a video, opens the folder in Explorer.

<a id="moving-media"></a>
## Moving media

Videos made before project folders kept their media in `%LOCALAPPDATA%\Bardo\projects`. After the update, Bardo moves each one into its video folder by itself, as a job you can follow, cancel and resume on [Jobs](jobs.md). It does the same for the videos of a channel you move to another project ([Moving a channel](your-projects.md#move)). On the same drive a folder is renamed at once; across drives each file is copied and checked before the original is deleted.

While media moves, no other job starts and the [background agent](background-agent.md) pauses, so nothing reads or writes a folder halfway through a move. Jobs waiting meanwhile start once it ends.

<a id="not-found"></a>
## Folder not found

When a project's folder cannot be found (it was moved or renamed, or its drive is not plugged in), the switcher and Overview mark the project **Folder not found**. Until you locate it, none of its jobs start, here or in the background agent: they wait on [Jobs](jobs.md), marked as waiting for the folder. Nothing is taken as missing meanwhile, so no portrait or image is drawn again. A scheduled post of that project waits too, and shows as [missed](missed-posts.md) if its time passes.

**Locate…** asks for the folder. Bardo accepts only the folder whose `bardo-project.json` names this project; a folder of another project, or of another copy of Bardo, is refused. Plugging the drive back in is enough too: Bardo sees the folder again when you open the switcher.
