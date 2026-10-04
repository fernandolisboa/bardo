---
id: network-accounts
title: Network accounts
group: publishing
place: accounts
tour: accounts
---

# Network accounts

A network account is a [channel](channels.md)'s presence on one network: its handle, the defaults every post there starts from, and the format its videos render in. YouTube, Instagram Reels and TikTok accounts can also connect, so Bardo uploads to them and reads their numbers. Accounts live on [Accounts](bardo:go/accounts). [Show me the screen](bardo:tour/accounts).

<a id="channel"></a>
## Pick the channel

The list shows your channels; pick one to see its accounts beside it. A channel has to be created on [Channels](bardo:go/channels) first.

<a id="accounts"></a>
## One account per network

A channel has at most one account on each of the five networks: YouTube, TikTok, Instagram Reels, X and Kick. Each card shows the network, the handle (without the @), whether the render preset is the network's default or custom, and the preset in one line.

Renders make one file per account, and the [Publish](bardo:go/projects/publish) stage writes one post per account.

<a id="metadata"></a>
## Metadata defaults

**Edit** opens the account's form. Its metadata defaults are what every post on this network starts from:

- **Metadata language**: the language the post's text is written in; **Same as the channel** follows the channel's.
- **Visibility**: public, unlisted or private on YouTube, public or private on TikTok. Posts on Instagram, X and Kick are always public.
- **Tags**: tags every post starts with, one per line or separated by commas, without #. Networks that take hashtags get them in the caption.
- **Description footer**: links, credits or a call to follow, added to the end of every description or caption.

Claude writes each post's title, description and tags on top of these, and you can edit the result at the Publish stage. See [Metadata and its limits](uploading.md#metadata).

<a id="preset"></a>
## Render preset

Each network renders in its own preset: the frame (16:9 or 9:16), the size, the codec, the bitrate, the longest video it takes and the loudness it aims at. The defaults are in [Networks and presets](render.md#targets). Under **Edit**, change any of them, or keep **network default**; the line under the fields shows the preset that results. A changed preset marks the network's last render out of date.

<a id="connect"></a>
## Connecting an account

YouTube, Instagram Reels and TikTok accounts show their connection on the card. Connecting needs your own app for that network, saved once in [Settings › Networks](app-credentials.md); each network's guide walks through it:

- **YouTube** connects in your browser: [Connecting YouTube](connect-youtube.md).
- **Instagram** connects with a token you paste from Meta's Graph API Explorer: [Connecting Instagram](connect-instagram.md).
- **TikTok** connects in your browser: [Connecting TikTok](connect-tiktok.md).

X and Kick don't connect: their posts are [exported](exporting.md) and posted by hand.

<a id="states"></a>
## Connection states

| State | Means | You can |
| --- | --- | --- |
| Not connected | Bardo has no access | **Connect** |
| Waiting for the browser | Your browser has the network's consent page open; Bardo waits up to five minutes | **Cancel** |
| Checking the token | Instagram: Bardo is trading the pasted token with Meta | Wait |
| Choose an account | Instagram: the token reaches several accounts | **Connect this one** on the right one |
| Connected as | Bardo can upload and read numbers as that account | **Check**, **Disconnect** |
| Reconnect needed | The network refused to renew the access: it was revoked or ran out | **Reconnect**, **Disconnect** |

The tokens are kept in Windows Credential Manager, per profile and account; Bardo's database keeps only the account's name and id and when the access runs out. A connected account has to be disconnected before it can be removed.

<a id="add"></a>
## Adding a network

Under the cards, **Add (network)** opens the form for each network the channel has no account on yet. Fill in the handle, change the defaults if you want, and **Add account**. To remove an account, open the **⋯** menu on its card; Bardo asks first.
