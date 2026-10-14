---
id: connect-x
title: Connecting X
group: publishing
---

# Connecting X

Bardo signs in to X with an app you register in your own X Developer Console account. Bardo ships no app of its own, so the app, its credits and its spending limit belong to you. This guide sets that app up once; after that, each X account connects from its network account card.

Bardo uploads the video to X and makes the post itself, with the text you reviewed, at once or at the time you schedule. X's API is paid per use: X bills each post from credits you buy ahead, and Bardo shows the price before you confirm.

<a id="need"></a>
## What you need

- The X account you post from.
- An account in the [X Developer Console](https://console.x.com), signed in with any X account (it does not have to be the posting one) and with the Developer Agreement and Policy accepted.
- A payment card for the app's credits. X has no free tier for posting.

<a id="app"></a>
## 1. Create the app

1. In the [Developer Console](https://console.x.com), create a new app. Give it a name (for example `Bardo`), a description and a use case.
2. Open the app's **User authentication settings** and set them up:
   - **App permissions**: **Read and write**. Bardo posts for you; with read only, X refuses the post.
   - **Type of App**: **Web App, Automated App or Bot**. That type is a confidential client, so X gives it a client secret, which Bardo needs.
   - **Callback URI / Redirect URL**: exactly `http://127.0.0.1:47147/callback`. X matches the address character by character, port included, and asks for `127.0.0.1` rather than `localhost`. While you authorize, Bardo listens once on that port of your own computer and X sends your browser back there.
   - **Website URL**: any page you control.
3. Save, then open **Keys and tokens** and copy the **OAuth 2.0 Client ID** and **Client Secret**. X shows the secret once; if you lose it, generate a new one and replace it in Bardo.

<a id="credits"></a>
## 2. Buy credits and set a limit

1. In the Developer Console, open the app's billing and buy credits. X deducts each request from them as it happens.
2. Set a **spending limit**, so a mistake can never cost more than you chose.

What Bardo's calls cost, as X priced them on 2026-10-14:

| What | X's price |
| --- | --- |
| A post | US$ 0.015 |
| A post whose text has a link | US$ 0.20 |
| Uploading the video | Not billed |
| Reading the account when you connect or select **Check** | US$ 0.010 |

X can change its prices; the Developer Console always shows the current ones. When the credits run out or the limit is reached, X refuses the post and Bardo says so: add credits, then **Retry**.

<a id="save"></a>
## 3. Save the app in Bardo

1. Open [Settings › Networks](bardo:go/settings/networks).
2. Paste the client ID and secret into **X · X Developer Console app** and select **Save**.

Both are kept in Windows Credential Manager under your Windows account, per Bardo profile. They never reach Bardo's database, logs or error messages, and the secret never shows on screen again; the card shows its last four characters.

<a id="connect"></a>
## 4. Connect the account

1. Open [Accounts](bardo:go/accounts), pick the channel and add (or open) its X account.
2. Select **Connect**. Your browser opens X's authorization page; sign in with the account you post from and select **Authorize app**.
3. X sends the browser back to Bardo, which reads the account and shows **Connected as @username**.

Bardo asks X for these permissions at once: `tweet.read` and `users.read` (who the account is), `tweet.write` (making the post), `media.write` (uploading the video) and `offline.access` (staying connected). If one is missing, nothing is kept and the card says which.

If the card says the port is in use, another program on your computer holds port 47147: close it, or wait a minute if you just tried, and connect again.

<a id="day-to-day"></a>
## Day to day

- **Tokens.** X's access token lasts two hours. Bardo renews it with the refresh token when it is due, and X hands out a new refresh token each time, which Bardo keeps in Windows Credential Manager.
- **Check** renews the token if it is due and reads the account's username again. X bills that read (see [Buy credits](#credits)).
- **Reconnect needed** means X refused the tokens: you revoked the app in X's settings (**Settings and privacy › Security and account access › Apps and sessions**), changed your password, or the app's client secret changed. Select **Reconnect**.
- **Disconnect** revokes Bardo's tokens at X and forgets them. If X cannot be reached, Bardo still forgets them; remove the app from your X account's settings to finish.

<a id="upload"></a>
## Posting a video

At the Publish stage, a connected X account offers **Review upload** once its preset is rendered and its text written.

- **The review** shows the file, the account and the post text as X gets it (with the account's footer and hashtags), the price X bills for it, and **Made with AI label**, on when the narration used a realistic voice. X has no title, visibility or cover for a post.
- **Upload and post** sends the file in chunks and, once X has processed it, makes the post. Bardo checks once a minute; if X is still processing after about a quarter of an hour, the post shows **Still processing** and **Check again** picks it up later.
- **Budget.** With a budget for X on the [Costs](costs.md) screen, a post that would reach it asks first; **Generate anyway** posts it.
- **Stop and resume.** A stopped or interrupted upload resumes after the last chunk X took. X keeps an upload for 24 hours; an upload past that, or one X dropped, goes again from the start, twice at most.
- **Video length.** X takes videos up to 2 minutes 20 seconds from most accounts, which is the length of Bardo's X preset. Longer videos need an X account that may post them; X refuses the post otherwise and Bardo shows X's reason.

<a id="schedule"></a>
## Scheduling a post

X takes no publish time from apps, so in the review **When** offers **Once processed** or **Schedule**, as for a Reel. With **Schedule**, Bardo sends the file up to 23 hours ahead and makes the post itself at the date and time you type.

Keep Bardo open at that time and the computer on, or turn on the [background agent](background-agent.md), which posts it with Bardo closed while you are signed in to Windows. If neither runs then, nothing is posted: the next time Bardo opens, it lists the post among the [missed posts](missed-posts.md).

<a id="costs"></a>
## The post on the Costs screen

Each post Bardo makes is recorded on the [Costs](costs.md) screen under X, with the video and channel it was for, at the price in the rate table (US$ 0.015, or US$ 0.20 with a link). Bardo prices a post with a link when its text has an address or a domain X would link, such as `example.com`. A post X refused is not billed and not recorded. The account reads made when connecting or checking are not recorded; the Developer Console shows them.
