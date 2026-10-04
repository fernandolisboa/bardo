---
id: troubleshooting
title: Troubleshooting
group: reference
---

# Troubleshooting

What Bardo's messages mean when something stops, and what to do. A job that stopped shows why on its card in [Jobs](jobs.md#retry), with **Details**; once you fix the cause, **Retry** goes on from where it stopped.

<a id="missing-key"></a>
## A key is missing

"Save a … key under Settings" means the step needs a provider whose key is not saved yet. Save it in [Settings › API keys](bardo:go/settings/keys), then do the step again or **Retry** its job. "The provider rejected the key" means it was saved but is wrong, revoked or lacks a permission: **Test key** says which, and [Testing a key](api-keys.md#testing) says what each answer means.

<a id="budget"></a>
## A budget is reached

**Over budget** before a generation means it would make a provider reach the monthly budget you set, or the budget is already used. **Generate anyway** goes on for this generation only; **Cancel** leaves it. To stop being asked, raise or remove the budget on [Costs](costs.md#budgets). Nothing is ever cut off midway because of a budget.

<a id="quota"></a>
## A quota is used up

"A quota or rate limit stopped the provider" means the provider is refusing calls for now, not that anything is broken. YouTube's quotas, for the Data API and for uploads, reset daily at midnight Pacific time; research results are kept for seven days, so running again within a week costs none. A provider that runs on credits stops when they run out: add credit in its dashboard. Jobs stopped by a rate limit retry by themselves first; when they give up, **Retry** them later.

<a id="reconnect"></a>
## An account needs to reconnect

**Reconnect needed** on an account means the network refused to renew Bardo's access: you revoked it, it expired (a Google client in Testing status lasts seven days, a TikTok sign-in a year unused), or, on Instagram, the account is no longer linked to a Page you manage. Until you reconnect it, uploads to it wait and syncs skip its owner numbers. Press **Reconnect** on its card on [Accounts](network-accounts.md#states); for Instagram, generate a new token first.

<a id="upload-limit"></a>
## An upload is held by a limit

**Over the publishing limit** means the network takes only so many posts from apps in a day: Instagram counts posts per 24 hours, TikTok drafts per 24 hours, and YouTube has a daily upload quota per Google project and an upload limit per channel. The upload stays queued, the card says when it goes, and Bardo sends it then by itself (keep Bardo open at that time). Posts sent by other apps count too, so Bardo reads the limit again before it tries. See [Upload states](uploading.md#states).

<a id="out-of-date"></a>
## A render or an export is out of date

**Out of date** on a network's render means the cut or the account's preset changed after the file was made; render again, and the review ticks it for you ([The last file](render.md#last)). **Out of date** on an export means the render or the metadata changed after it; export again ([Out of date](exporting.md#outdated)). Earlier stages mark what was made from them the same way: a narration after the script changed, scenes after the narration changed.

<a id="logs"></a>
## Still stuck

Bardo's log, at `%LOCALAPPDATA%\Bardo\logs\bardo.log`, records each step with no key or token in it ([Logs](data-and-keys.md#logs)). A failed job's **Details** shows the provider's own message, which its documentation explains.
