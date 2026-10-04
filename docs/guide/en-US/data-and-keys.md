---
id: data-and-keys
title: Where your data and keys live
group: reference
---

# Where your data and keys live

Bardo keeps everything on your computer, under your Windows account. There is no Bardo server and no Bardo account: what leaves the computer goes straight to the providers and networks you use, with your own keys and sign-ins.

<a id="database"></a>
## The database

Your channels, personas, templates, projects and their scripts, research, ideas, costs, jobs, posts and settings are in one database file, `%APPDATA%\Bardo\bardo.db`. It holds no key, secret or sign-in.

<a id="projects"></a>
## Project folders

Each video project's media is in its own folder under `%LOCALAPPDATA%\Bardo\projects`: the narration, the images, the clips, imported files, the editor's preview copies and the rendered files. Media is large and belongs to this computer, so it stays out of a roaming Windows profile. Voice samples are a cache in `%LOCALAPPDATA%\Bardo\voice-samples`: only the newest are kept, and an older one is made again when you play it.

<a id="exports"></a>
## Exports

Exports go to `Videos\Bardo`, one folder per project with a folder per network inside, holding the rendered file and its `metadata.txt`, where they are easy to drag into a network's uploader. Persona packages go wherever you save them, in Documents unless you pick another folder.

<a id="credentials"></a>
## Windows Credential Manager

Your API keys, your networks' app credentials and the sign-ins of your connected accounts are kept in Windows Credential Manager, readable only by your Windows account, under **Generic Credentials** with names that start with `Bardo/`. They stay on this computer: on another one, save your keys and credentials again and reconnect the accounts. Removing a key or a credential, or disconnecting an account, deletes it from Credential Manager too.

<a id="logs"></a>
## Logs

Bardo writes what it does to `%LOCALAPPDATA%\Bardo\logs\bardo.log`, kept to a bounded size. Every line passes through the same masking as the screen: no key, secret or token is ever written to it.

<a id="backup"></a>
## Backing up and moving

To back up your work, copy `bardo.db` and the `projects` folder while Bardo is closed. Keys, credentials and sign-ins are not in them, on purpose: after restoring on another computer, save them again in [Settings](settings.md#tabs) and reconnect each account on [Accounts](network-accounts.md#connect).
