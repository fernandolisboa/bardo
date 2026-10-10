---
id: app-credentials
title: Network app credentials
group: publishing
place: settings/networks
tour: networks
---

# Network app credentials

To upload to a network and read its numbers, Bardo signs in to it with an app you register on that network. [Settings › Networks](bardo:go/settings/networks) keeps those apps' credentials. [Show me the tab](bardo:tour/networks).

<a id="networks"></a>
## One app per network

| Network | The app | What Bardo asks for |
| --- | --- | --- |
| YouTube | A Google OAuth client of type Desktop, in your Google Cloud project | Client ID and client secret |
| Instagram Reels | A Business app in your Meta developer account, with Facebook Login for Business | App ID and app secret |
| TikTok | An app (or its sandbox) in your TikTok for Developers account | Client key and client secret |

X and Kick have no card: Bardo exports their posts for you to post by hand. Each card's line under the network's name says what the app needs; the network's guide walks through it step by step.

<a id="why"></a>
## Why Bardo ships none

Bardo has no app of its own on any network. Your app is registered by you, so its quota, its consent screen and any review or audit are yours, on your schedule, and no one else's use of Bardo spends your quota. Your channels stay between you and the network.

Setting an app up takes a few minutes per network, once:

- [Connecting YouTube](connect-youtube.md)
- [Connecting Instagram](connect-instagram.md)
- [Connecting TikTok](connect-tiktok.md)

<a id="step-by-step"></a>
## Step by step for each app

Each card's **Step by step** opens a screen with that network's setup: each step on the network's developer site, with a link straight to the page it happens on, and the card's fields at the end to paste the ID and secret into and save. **Back** returns to the tab. The steps are the first sections of the network's guide.

<a id="save"></a>
## Saving an app

Paste the app's ID (TikTok calls it the client key) and secret from the network's developer site into its card, and select **Save**. Bardo checks their shape and says under a field what looks wrong. **Replace** saves a new pair over the old one, for example after you make a new secret; **Remove** forgets them.

If a network stops accepting the saved ID or secret, connecting or checking an account says so and points back here.

<a id="kept"></a>
## Where they are kept

The ID and secret go to Windows Credential Manager under your Windows account, per Bardo profile, like your [API keys](api-keys.md). They never reach Bardo's database, logs or error messages, and the secret never shows again: the card says **Saved, secret ending in** its last four characters.

<a id="connect"></a>
## Then connect

With the app saved, connect each channel's account on [Accounts](bardo:go/accounts). See [Connecting an account](network-accounts.md#connect).
