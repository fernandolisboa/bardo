---
id: backup
title: Backup and restore
group: reference
place: settings/backup
tour: backup
---

# Backup and restore

[Settings › Backup](bardo:go/settings/backup) saves your work in one file, backs up the database by itself every week, and brings a backup back. [Show me the tab](bardo:tour/backup).

<a id="back-up"></a>
## Backing up

**Back up…** asks where to save a `.zip` file (Documents, unless you pick another folder) and makes it as a job, with its progress on the tab and on [Jobs](jobs.md). Bardo stays open and keeps working meanwhile: the database is copied in one consistent moment, even while the [background agent](background-agent.md) sends a post.

The backup holds the [database](data-and-keys.md#database): your channels, personas, templates, projects, scripts, research, posts and schedules, costs and settings. Tick **Include media** to take the [project folders](data-and-keys.md#projects) too: the narration, images, clips, music, imported files and renders. The tab says how big they are. The editor's preview copies are left out, because the editor makes them again when a project opens, and so are the voice sample cache, the logs and the [exports](data-and-keys.md#exports), which are your own files already.

A backup leaves out the YouTube data Bardo may keep only for 30 days: niche research, theme rankings, and the numbers of YouTube posts whose channel is not connected. A connected channel's own numbers stay ([Privacy policy](privacy.md#kept)).

A backup never holds a key, an app credential or a sign-in: they are in [Windows Credential Manager](data-and-keys.md#credentials), not in the database or the folders. Store the file wherever you like, for example on an external disk or a cloud folder.

<a id="automatic"></a>
## Automatic backups

With **Back up the database every week** on, as it is at first, the first time Bardo opens in a week it backs up the database, without media, to `%LOCALAPPDATA%\Bardo\backups`, and keeps the last four. They are listed on the tab, newest first, each with **Restore**; **Open folder** shows them in File Explorer.

They live on the same disk as your data: they protect against a mistake or a bad update, not against a failed disk or a lost computer. Keep a backup with media somewhere else too.

<a id="settings-file"></a>
## Settings file

A settings file takes your keys, preferences, personas and channels to another copy of Bardo: from one built from source to the installed one, or to a new computer, without the projects and posts a backup carries. A password protects it.

**Export settings…** lists what this copy has, all ticked but the sign-ins:

- **API keys** and **App credentials**, each saved one by name.
- **Preferences and costs**: the interface language, theme and layout, whether Bardo offers tours, backs up every week and checks for updates, how post numbers sync, the lowest score of the cut suggestions shown, your budgets and the prices you changed.
- **Personas** (without their looks: a drawn look travels in the persona's own package) and **Channels**, each channel with its network accounts.
- **Sign-ins**: an account's sign-in, which goes only with its channel. Usually leave these off and sign in again in the other copy. Some networks, X among them, keep only one copy signed in: the first copy to renew its sign-in signs the other out.

Type a password of at least 10 characters twice and select **Save file…**. Bardo cannot recover a forgotten password: without it the file does not open. The file is encrypted with that password, so keep the file and the password apart; anyone with both can read your keys.

**Import settings…** asks for the file, shows which Bardo made it and when, and asks for its password. Then it lists what the file holds, all ticked but the sign-ins, each marked **New here** or **Replaces the one here**. What replaces takes the file's details and keeps its place: a persona or channel of the same name keeps its projects, and a key or app credential of the same network is overwritten. Nothing you leave unticked, and nothing the file does not hold, changes. **Import** brings the ticked items in and lists any that could not be saved.

A file made by a newer Bardo opens if its format is one this Bardo knows; values or parts this Bardo does not know are listed and left out. Update Bardo to import them. A file of a newer format asks you to update Bardo first ([Updating](install.md#update)).

The first time a new copy of Bardo opens, a bar over the screens offers to import a settings file; **Not now** closes it, and the tab offers the same at any time. The installer can take a settings file too ([Setting it up](install.md#setup)).

<a id="restore"></a>
## Restoring

**Restore from a file…** asks for a backup and reads it first. Bardo refuses a file that is not a Bardo backup, a damaged one, and one made by a newer Bardo (update Bardo first, see [Updating](install.md#update)). Otherwise it shows when the backup was made, by which version, and what it holds, and asks you to confirm.

Restoring replaces everything in Bardo with the backup: work done since it was made is lost. A backup without media keeps the project folders on this computer as they are. While jobs are running the restore waits: let them end, or cancel them on [Jobs](jobs.md).

**Restore and restart** unpacks and checks the backup beside your data, waits for the background agent to finish what it is sending, and restarts Bardo, which puts the backup in place before it opens anything. If the files cannot be replaced, Bardo puts everything back and opens your data as it was, saying so. In the rare case it cannot put a file back, nothing is deleted: the tab says which folder holds your earlier data. Only one Bardo window restores: with two open, restore from the first.

Scheduled posts come back as they were in the backup. A post whose time came while it was being sent, or after the backup was made, shows as [missed](missed-posts.md), so Bardo checks with the network before anything goes and never sends it again on its own. An upload that was waiting or sending when the backup was made, and that Bardo does not schedule itself (YouTube, TikTok, or an Instagram post sent at once), shows as failed instead of starting again, because it may have gone after the backup: check the network and retry it if the post is not there. A post you sent by hand after the backup was made shows as scheduled again: cancel it.

<a id="after-restore"></a>
## After a restore

The tab lists what this computer still needs: the keys and app credentials the backup's computer had and this one lacks, and the accounts whose sign-in is not here. Each line opens the place to fix it: [API keys](api-keys.md), [Network app credentials](app-credentials.md) or [Accounts](network-accounts.md#connect). On the same computer there is usually nothing to do. **Done** closes the list.

<a id="moving"></a>
## Moving to another computer

1. On the old computer, back up with **Include media** ticked, and copy the file to the new one.
2. [Install Bardo](install.md) on the new computer, open it, and restore the file from **Settings › Backup**.
3. After the restart, follow the list: save your keys and app credentials again and reconnect each account. A [settings file](#settings-file) made on the old computer brings the keys and app credentials in one go.
