---
id: privacy
title: Privacy policy
group: reference
---

# Privacy policy

Bardo is a desktop app that runs on your own computer. There is no Bardo server and no Bardo account: Bardo's makers receive nothing about you, your channels or your videos. This policy says what Bardo reads, keeps and sends, and how you remove it. Bardo asks you to accept it before it uses YouTube; [Settings › About](about.md#privacy) shows whether you did and links back here.

<a id="youtube"></a>
## YouTube API Services

Bardo uses the **YouTube API Services** to upload and schedule your videos, show which channel is connected, read your posts' numbers and research niches. By using Bardo's YouTube features you agree to be bound by the [YouTube Terms of Service](https://www.youtube.com/t/terms). What Google does with data it holds is covered by the [Google Privacy Policy](https://www.google.com/policies/privacy).

<a id="read"></a>
## What Bardo reads

From YouTube, with your own API key or your channel's authorization:

- the connected channel's id and title;
- your uploads' processing and privacy status and publish time;
- your posts' public numbers (views, likes, comments) and, for a connected channel, its owner's numbers from YouTube Analytics: engaged views, watch time, average view, the audience retention curve and, for a channel in the YouTube Partner Program, estimated revenue and CPM;
- for niche research: recent public videos in a market (their ids, publish dates and view counts) and their channels' subscriber counts.

From you: what you type into Bardo (channels, scripts, metadata), the media you import or generate, and your API keys and app credentials.

Bardo reads no comments, messages, subscribers' identities or any other channel's private data, and nothing about people who watch your videos beyond the totals above.

<a id="use"></a>
## How Bardo uses it

Only to do what you ask in the app: upload and schedule your videos, show how your posts do on Performance, rank your theme ideas and show niche research. Bardo has no ads, sells nothing and builds no profile of you.

<a id="kept"></a>
## Where it is kept, and for how long

Everything stays on your computer, under your Windows account ([Where your data and keys live](data-and-keys.md)): API keys, app credentials and sign-in tokens in Windows Credential Manager, and everything else in Bardo's local database.

Data from YouTube is kept no longer than 30 days, with one exception: your connected channel's own numbers stay while Bardo holds that channel's authorization and has confirmed it within the last 30 days. In practice:

- Niche research results and the theme rankings made from them are deleted 30 days after they were read.
- Your posts' numbers read with the API key are deleted 30 days after they were read. A connected channel's posts are read through the channel's authorization instead, so their history stays while it is connected.
- Once a day, Bardo confirms each connected channel's authorization with Google. A connection that cannot be confirmed for 30 days is removed, with everything read through it.
- A post YouTube no longer shows for 30 days loses its numbers.
- Bardo reads your posts' numbers again before they turn 30 days old when it opens, whatever **Sync on start** says, so they are refreshed rather than lost.

Backups leave out the data that expires (research, rankings and numbers read with the key); restored, the 30-day rule applies again at once ([Backup and restore](backup.md)). What is yours stays as long as you keep it: your channels, projects, the link of each post and what Bardo uploaded.

<a id="shared"></a>
## What is sent, and to whom

Bardo sends data only to the services you set up, with your own keys, to do what you ask:

- **YouTube** (Google): your videos and their metadata when you upload, and the requests above.
- **The AI providers you use**: Anthropic's Claude writes theme ideas and scripts, and TypeSafe's JEV ranks ideas. To do that, Bardo sends them your channel's details and a few numbers worked out from YouTube data: a niche's competition and trend scores, median views per day and upload volume, and the first-week views of your channel's earlier videos. No video, title, comment or channel read from YouTube is sent.
- **Other providers** (ElevenLabs, Gemini, Higgsfield, Instagram, TikTok) get only what you send them for narration, images, clips and posts; none of it comes from YouTube.

Each of those services handles what it receives under its own privacy policy. Nothing goes to Bardo's makers or to anyone else. [Export diagnostics](about.md#diagnostics) makes a file only you can choose to share, with every key, token and account name masked.

<a id="cookies"></a>
## Cookies

Bardo stores no cookies and places nothing on your device to track you. Your browser opens Google's pages only while you connect a channel, under Google's own policies.

<a id="revoke"></a>
## Removing Bardo's access

**Disconnect** on the YouTube account card, in [Accounts](network-accounts.md), revokes Bardo's access at Google and deletes the channel's owner numbers and retention curves at once, with any of its numbers older than the 30-day limit. You can also remove Bardo's access at any time on your Google account's [security settings page](https://security.google.com/settings/security/permissions); Bardo notices within a day and deletes what that access gave as described above.

<a id="delete"></a>
## Deleting your YouTube data

**Delete YouTube data**, in [Settings › About](about.md#privacy), deletes at once every datum Bardo read from YouTube: niche research, theme rankings, and your posts' numbers and retention curves. It does not change anything on YouTube, and it keeps your channels, themes, projects, connections and the link of each post. To remove everything Bardo keeps, uninstall it and delete its folders, as [Where your data and keys live](data-and-keys.md) lists.

<a id="changes"></a>
## Changes to this policy

When Bardo starts using data in a way this policy does not describe, the policy changes first and Bardo asks you to accept it again before using YouTube. Earlier versions are in the history of this page in [Bardo's repository](https://github.com/fernandolisboa/bardo/commits/main/docs/guide/en-US/privacy.md).

<a id="contact"></a>
## Contact

Questions about this policy or a request about your data: open an issue on [GitHub](https://github.com/fernandolisboa/bardo/issues). Since Bardo keeps your data only on your computer, the steps above are how you see, change or delete it.
