---
id: connect-tiktok
title: Connecting TikTok
group: publishing
---

# Connecting TikTok

Bardo signs in to TikTok with an app you register in your own TikTok for Developers account. Bardo ships no app of its own, so the app, its sandbox and any audit belong to you. This guide sets that app up once; after that, each account connects from its network account card in a few clicks.

Bardo sends TikTok videos to your inbox as **drafts**: you finish the caption, choose who can watch and post (or schedule) in the TikTok app. Bardo never posts to TikTok by itself.

<a id="need"></a>
## What you need

- The TikTok account you post from.
- A [TikTok for Developers](https://developers.tiktok.com/) account (sign up with any email; it does not have to be the posting account).

<a id="register"></a>
## 1. Register the app

1. In TikTok for Developers, open your profile menu › **Manage apps** and select **Connect an app**. Register it under your individual account (or an organization if you have one).
2. Fill in **Basic information**: an **App icon** (1024 × 1024 px), an **App name** (for example `Bardo`), a **Category** and a **Description**; the description shows on TikTok's authorization page. TikTok also asks for a **Terms of Service URL**, a **Privacy Policy URL** and, for **Desktop**, a website; any pages you control will do while the app stays in its sandbox.
3. Under **Platforms**, select **Desktop**.

The **Credentials** section shows the app's **Client key** and **Client secret**. You will copy them from the sandbox in step 3, not from here.

<a id="products"></a>
## 2. Add the products and scopes

1. Select **Add products** and add:
   - **Login Kit**;
   - **Content Posting API**. Leave **Direct Post** off: Bardo only uploads drafts.
2. In **Login Kit**, open the **Desktop** settings and add this **Redirect URI** exactly, trailing slash included: `http://127.0.0.1:*/callback/`. The `*` is TikTok's wildcard port: while you authorize, Bardo listens once on `127.0.0.1` on a random port, at `/callback/`, and TikTok sends your browser back there.
3. Under **Scopes**, make sure the app has all three (Bardo asks for them at once):
   - `user.info.basic`: the account's name, shown on the card;
   - `video.upload`: sending a video to your inbox as a draft;
   - `video.list`: reading your videos' numbers for owner metrics.

<a id="sandbox"></a>
## 3. Create a sandbox and add your account

Bardo works from the app's sandbox; it never needs the app approved.

1. Next to the app's name, switch to **Sandbox** and select **Create Sandbox**. Name it (for example `Bardo`) and clone the configuration from production, so Login Kit, the Content Posting API, the redirect URI and the scopes come along. Check them, then select **Apply changes**.
2. In **Sandbox settings › Target users**, select **Add account**, sign in with the TikTok account you post from and accept the developer terms. TikTok can take up to an hour to show it. Only target users can authorize a sandbox app (up to 10 per sandbox).
3. Copy the sandbox's **Client key** (it starts with `sb`) and **Client secret** from its **Credentials**. A sandbox has its own credentials; the production ones will not sign in a target user.

<a id="review"></a>
## Why the app is not sent to review

TikTok's Content Sharing and App Review guidelines reject apps for private or personal use; "a utility tool to help upload contents to the account(s) you or your team manages" is listed as not acceptable. A personal Bardo setup is exactly that, so it would not pass, and it does not need to: uploading drafts to your own inbox needs no audit. What an audit would add is Direct Post (posting without the TikTok app), which Bardo does not build.

**Keep the account private while you test.** TikTok documents that every account posting through an unaudited app must be private at the time of posting, and that such posts are visible only to the creator (`SELF_ONLY`). TikTok writes these rules for Direct Post; whether it also applies them to drafts sent from a sandbox is checked in the publishing regression pass. Set the account to **Private account** in TikTok's **Settings and privacy › Privacy** until then.

<a id="save"></a>
## 4. Save the app in Bardo

1. Open [Settings › Networks](bardo:go/settings/networks).
2. Paste the client key and secret into **TikTok · TikTok for Developers app** and select **Save**.

Both are kept in Windows Credential Manager under your Windows account, per Bardo profile. They never reach Bardo's database, logs or error messages, and the secret never shows on screen again; the card shows its last four characters.

<a id="connect"></a>
## 5. Connect the account

1. Open [Accounts](bardo:go/accounts), pick the channel and add (or open) its TikTok account.
2. Select **Connect**. Your browser opens TikTok's authorization page.
3. Sign in with the target user's account and allow every permission. Leaving one off makes Bardo refuse the connection and revoke what was granted.
4. The browser shows "Bardo is connected" and the card shows **Connected as (display name)**.

Bardo waits five minutes for the browser; **Cancel** stops waiting.

The access and refresh tokens are kept in Windows Credential Manager, keyed by profile and network account. Bardo's database keeps only the account's `open_id` and display name, the scopes, the token expiry and the last refresh.

<a id="day-to-day"></a>
## Day to day

- TikTok's access token lasts 24 hours and its refresh token 365 days. Bardo renews the access when it is used or checked, and when the app starts if it has run out, so the connection lasts as long as Bardo is opened now and then. TikTok may hand back a new refresh token at each renewal; Bardo always keeps the newest.
- **Check** renews the access if it is close to expiring and reads the display name again.
- **Reconnect needed** means TikTok refused to renew the access: you removed Bardo from the account's authorized apps, or the refresh token went a year unused. Select **Reconnect**. A client key or secret TikTok no longer accepts (say, after resetting the secret) shows "TikTok doesn't recognize the client key or secret" instead: save the new one in **Settings › Networks**.
- **Disconnect** revokes Bardo's access at TikTok and forgets the tokens. Revoking needs the app's credentials: if they were removed from **Settings › Networks**, or TikTok cannot be reached, Bardo still forgets the tokens and tells you to remove its access yourself, from the apps and services permissions in the TikTok app's security settings.
- A connected account must be disconnected before it can be removed.

<a id="draft"></a>
## Sending a draft

1. At a project's **Publish** stage, pick the TikTok account and select **Review upload**. The review shows the rendered file, the connected account and the caption to paste. A file TikTok would not take is listed instead, with what to change.
2. Tick **Remind me of the AI-generated content label** when the video has realistic AI content (it is ticked when the narration uses a realistic voice). TikTok sets the label in its app; Bardo reminds you when the draft arrives.
3. Select **Send to TikTok inbox**. Bardo sends the file in chunks and resumes from the last one TikTok confirmed if the connection drops or you stop it. After an hour TikTok forgets an unfinished upload, so a later resume sends the file again.
4. When TikTok has the draft, the post shows **Draft in TikTok** with the caption and a **Copy caption** button. Open TikTok's inbox in the app, paste the caption, turn on the AI label if reminded, and post (or schedule) it.
5. Back in Bardo, select **Mark as posted** and paste the post's link, so Bardo follows it like any other post.

<a id="metrics"></a>
## Numbers on the Performance screen

With the account connected, every metrics sync reads the views, likes, comments and shares of the channel's linked TikTok posts, 20 per request, with no YouTube key. TikTok reports no watch time, retention or revenue, so those stay empty.

- TikTok returns only the account's public posts. A post it leaves out (deleted, made private, or not the account's) shows as **Not found** and keeps the numbers it had.
- A draft is followed once you mark it as posted with its link.
- Without a connected account, a linked post keeps only its link. When the account needs to reconnect, syncs skip its posts until it does.

<a id="limits"></a>
## Limits to know

- TikTok keeps at most 5 drafts from an app waiting in your inbox in any 24 hours. Bardo counts the drafts it sent and refuses a sixth in the review, saying when the next may go. If TikTok still refuses one (say, drafts from another app), the upload waits in the queue and Bardo tries again later.
- TikTok takes MP4, WebM or MOV in H.264, H.265, VP8 or VP9, 23 to 60 frames per second, 360 to 4096 pixels per side, up to 10 minutes and 4 GB. Bardo's presets meet these.
- The Content Posting API has no scheduling field: you schedule the draft in the TikTok app when you post it.
