---
id: your-projects
title: Your projects
group: getting-started
---

# Your projects

A **project** holds the channels of one brand, with their research, ideas and videos. You work in one project at a time: the **open project**. Overview, Ideas, Videos, Channels and accounts and Performance show only its channels and their work; narrators, products and templates are shared by every project for now ([Global library](global-library.md)). [Show me the switcher](bardo:tour/welcome).

When you updated to this version, Bardo made a project for each channel, named after it. Channels linked by a localized version went into one project together, named after the oldest of them.

<a id="switcher"></a>
## The project switcher

The title bar shows the open project beside the Bardo logo: its color, its name and how many channels it has. Click it to open the switcher:

- **Search** narrows the list as you type; Enter opens the first project that matches.
- **Favorites** are the projects you starred, in the order you set. The arrow moves a favorite up.
- **Recent** lists the last eight projects you opened, most recent first, leaving out favorites.
- **All projects** lists the rest by name.

Click a project to open it. Bardo remembers the open project, and opens on it the next time. **Show folder** opens the open project's folder in Explorer. A project whose folder cannot be found is marked **Folder not found**, with **Locate…** ([Folder not found](storage.md#not-found)). The star beside a project adds it to the favorites or takes it out. The switcher is not shown while the editor is open: close the editor to switch.

<a id="no-project"></a>
## No project

**No project**, after the projects in the switcher, opens the loose videos: videos with no project and no channel, for a quick try ([Loose videos](projects.md#loose-videos)). It shows how many there are. While it is open, the title bar says **No project**, Videos lists the loose videos and the other project screens ask you to open a project. Bardo remembers it like any project, and opens on it the next time.

<a id="new"></a>
## A new project

**New project…** at the bottom of the switcher asks for a name, a color and a folder: the base folder by default, or another place with **Choose…**. The form shows the folder it will make ([A project's folder](storage.md#project-folder)). Names are unique among your projects, whatever their case. The new project opens with no channel, and every screen asks for one: create its first channel on [Channels](channels.md).

<a id="archive"></a>
## Archiving a project

**Archive project…** in the switcher puts the open project away: it leaves the switcher, Recent and Favorites, and becomes read-only, so no job of it runs and nothing of it publishes. Its published posts go on syncing their metrics, and its network accounts stay connected. Another project opens in its place.

**Compress media** packs its videos' folders into **Videos.zip** in the project folder, as a job ([Jobs](jobs.md)); Bardo reads the archive back to check it before it removes the folders. The **World** folder is never packed.

**Archived projects** at the bottom of the switcher lists them, marked **Media compressed** when they are. **Unarchive** brings one back, with its star if it had one; a compressed one is unpacked first, and comes back when that job ends.

<a id="delete"></a>
## Deleting a project

**Delete project…** in the switcher (or the bin beside an archived project) asks you to type the project's name. It removes the project with its channels, ideas and videos, disconnects its network accounts as **Disconnect** does ([Network accounts](network-accounts.md)), and sends its folder to the Recycle Bin. The dialog says beforehand when the folder will be deleted permanently instead: on a removable or network drive, when the drive's Recycle Bin is off, or when the folder is larger than that bin holds. **Delete permanently** skips the bin on purpose. A folder Bardo cannot reach is left alone, and only the records go.

Before a project's folder goes to the Recycle Bin, Bardo writes the project's records into it, in **bardo-project.db**. Restored from the bin, the folder opens again with **Open a project from a folder…** ([Opening a project from a folder](#open-folder)). If the records cannot be written, nothing is deleted. A folder deleted permanently takes no records along.

Nothing is deleted on the networks, and [Costs](costs.md#deleted) keeps the project's spending under its name until you forget it.

Archiving and deleting are refused while something is pending, and the dialog lists what to clear first:

- a post waiting in Bardo for its time ([schedule](uploading.md)),
- a post waiting on its network to go public later,
- a job for the project, one of its channels or videos, queued or running ([Jobs](jobs.md)).

<a id="export"></a>
## Exporting a project

**Export project…** in the switcher writes the open project to one **.bardo-project** file, wherever you choose, as a job with progress and **Cancel** ([Jobs](jobs.md)). The package holds:

- the project's channels, ideas, videos, world, narrators, products and templates, with its posts as history;
- copies of the global narrators, products, characters and templates the project uses, so it opens complete elsewhere;
- the media of its videos and of its world.

It never holds API keys, app credentials, network sign-ins or tokens, costs, unfinished jobs, or the numbers YouTube lets apps keep only for a while ([Data and keys](data-and-keys.md#credentials)). Export it to move a project to another computer or profile, or to keep a copy of one project apart from a full [backup](backup.md).

<a id="import"></a>
## Importing a project package

**Import a project package…** asks for a **.bardo-project** file, then shows what it holds, when and by which version of Bardo it was written, and asks for the new project's name, color and folder. The name is the package's own, numbered when you have a project of that name. **Import** brings it in as a job; the project shows in the switcher when the job ends.

- It always comes in as a **new project**, with a folder of its own and new ids: importing a package into the profile it came from makes a second project beside the first.
- Its channels come without network connections: connect them again on [Network accounts](network-accounts.md). A channel whose name you already use gets the project's name after it.
- Its posts come as history. A post this profile already has (the same network and post) is left out, so nothing counts twice in [Performance](performance-metrics.md).
- The global narrators, products and characters it carried become the project's own. Its global templates do too only when ticked in the dialog; the others stay this profile's global ones, and the texts already written keep what they were written with ([Templates](templates.md)).
- A package written by a newer Bardo is refused: update Bardo first ([Updates](about.md#updates)).

<a id="open-folder"></a>
## Opening a project from a folder

**Open a project from a folder…** asks for a project's folder (the one with **bardo-project.json**) and opens its project:

- a project Bardo knows, whose folder you moved, points at the new place, as **Locate…** does ([Folder not found](storage.md#not-found));
- a deleted project's folder, restored from the Recycle Bin, comes back as a new project from the records it holds, with its media where it is. One whose media was compressed comes back archived: **Unarchive** unpacks it.

A folder from another copy of Bardo or another profile is refused: export the project there and import its package here.

<a id="move"></a>
## Moving a channel

A channel can change projects on [Channels](channels.md#project) with **Move to another project…**. It takes its ideas, videos, network accounts and media along: the media moves into the new project's folder as a job ([Moving media](storage.md#moving-media)). Bardo refuses while one of these is pending, and says which:

- a post waiting in Bardo for its time ([schedule](uploading.md)),
- a post waiting on its network to go public later,
- a job for the channel or one of its videos, queued or running ([Jobs](jobs.md)),
- a video of the channel that is a localized version of a video in another channel of the project, or has one there. Versions stay inside one project, so a version is only offered for the project's other channels.

<a id="jobs-costs"></a>
## Jobs and costs

[Jobs](jobs.md#filter) and [Costs](costs.md#projects) stay the whole profile's: jobs run and money is spent whatever project is open. Both have a project filter that starts on the open project, and each job and cost keeps the names of the project, channel and video it was for, as they were then.
