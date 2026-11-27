---
id: project-keys
title: A project's own keys
group: reference
place: settings/project
tour: project-keys
---

# A project's own keys

[Settings › This project](bardo:go/settings/project) gives the open project its own API key for a provider, and a monthly budget for what that key pays. **Keys and budgets…** in the [project switcher](your-projects.md#switcher) opens it too. [Show me the tab](bardo:tour/project-keys).

<a id="own-keys"></a>
## Own keys

Each provider has a card. Without a key of its own, the card says **Uses the app's key**: the project's work calls the provider with the key in [Settings › API keys](api-keys.md), as before. Saving a key here makes every call for this project use it instead: the steps you start on its videos, its narrators' looks, its products and its world, and the jobs the [background agent](background-agent.md) runs for it. A job queued before you saved the key uses the new key when it runs.

The key is kept in Windows Credential Manager for this project only, never in Bardo's database, and the card shows only its last characters. **Test key** checks it with the provider, **Replace key** saves another one and **Remove** goes back to the app's key. When this computer cannot read a saved key (after a [restore](backup.md#restore), for example), the card says so and the project's work for that provider stops until you save it again: Bardo never falls back to the app's key without telling you.

Work outside a project (a loose video, a global narrator or product) always uses the app's keys.

<a id="budgets"></a>
## Budgets

A provider with the project's own key can have its own monthly budget. What that key pays counts only against it, never against the app's budget for the provider, and the app's budget counts only what the app's key pays. As with the app's budgets ([Costs](costs.md#budgets)), Bardo asks before a step that would take the project's budget past it. Removing the key removes its budget with it.

[Costs](costs.md#project-keys) shows each project's own keys this month, with what they paid against their budgets, and each call notes which key paid it.

<a id="leaving"></a>
## What never leaves the computer

A project's keys, like the app's, never go into a [project package](your-projects.md#export), an item file or a [settings file](backup.md#settings-file). Deleting the project removes them from Windows Credential Manager. A backup holds only the note that the project has a key: after a restore on another computer, the restore checklist lists each one by project and provider to save again ([Data and keys](data-and-keys.md#credentials)).
