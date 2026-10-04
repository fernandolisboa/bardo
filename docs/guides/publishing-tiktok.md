# Connecting a TikTok account

Bardo signs in to TikTok with an app you register in your own TikTok for
Developers account (ADR-0008). Bardo ships no app of its own, so the app,
its sandbox and any audit belong to you. This guide sets that app up once;
after that, each account connects from its network account card in a few
clicks.

Bardo sends TikTok videos to your inbox as **drafts**: you finish the
caption, choose who can watch and post (or schedule) in the TikTok app.
Bardo never posts to TikTok by itself.

## What you need

- The TikTok account you post from.
- A [TikTok for Developers](https://developers.tiktok.com/) account (sign
  up with any email; it does not have to be the posting account).

## 1. Register the app

1. In TikTok for Developers, open your profile menu › **Manage apps** and
   select **Connect an app**. Register it under your individual account
   (or an organization if you have one).
2. Fill in **Basic information**: an **App icon** (1024 × 1024 px), an
   **App name** (for example `Bardo`), a **Category** and a
   **Description**; the description shows on TikTok's authorization page.
   TikTok also asks for a **Terms of Service URL**, a **Privacy Policy URL**
   and, for **Desktop**, a website; any pages you control will do while the
   app stays in its sandbox.
3. Under **Platforms**, select **Desktop**.

The **Credentials** section shows the app's **Client key** and **Client
secret**. You will copy them from the sandbox in step 3, not from here.

## 2. Add the products and scopes

1. Select **Add products** and add:
   - **Login Kit**;
   - **Content Posting API**. Leave **Direct Post** off: Bardo only
     uploads drafts.
2. In **Login Kit**, open the **Desktop** settings and add this
   **Redirect URI** exactly, trailing slash included:

   ```
   http://127.0.0.1:*/callback/
   ```

   The `*` is TikTok's wildcard port: while you authorize, Bardo listens
   once on `127.0.0.1` on a random port, at `/callback/`, and TikTok sends
   your browser back there.
3. Under **Scopes**, make sure the app has all three (Bardo asks for them
   at once):
   - `user.info.basic`: the account's name, shown on the card;
   - `video.upload`: sending a video to your inbox as a draft;
   - `video.list`: reading your videos' numbers for owner metrics.

## 3. Create a sandbox and add your account

Bardo works from the app's sandbox; it never needs the app approved.

1. Next to the app's name, switch to **Sandbox** and select **Create
   Sandbox**. Name it (for example `Bardo`) and clone the configuration
   from production, so Login Kit, the Content Posting API, the redirect URI
   and the scopes come along. Check them, then select **Apply changes**.
2. In **Sandbox settings › Target users**, select **Add account**, sign in
   with the TikTok account you post from and accept the developer terms.
   TikTok can take up to an hour to show it. Only target users can
   authorize a sandbox app (up to 10 per sandbox).
3. Copy the sandbox's **Client key** (it starts with `sb`) and **Client
   secret** from its **Credentials**. A sandbox has its own credentials;
   the production ones will not sign in a target user.

## Why the app is not sent to review

TikTok's Content Sharing and App Review guidelines reject apps for private
or personal use; "a utility tool to help upload contents to the account(s)
you or your team manages" is listed as not acceptable. A personal Bardo
setup is exactly that, so it would not pass, and it does not need to:
uploading drafts to your own inbox needs no audit. What an audit would add
is Direct Post (posting without the TikTok app), which Bardo does not
build.

**Keep the account private while you test.** TikTok documents that every
account posting through an unaudited app must be private at the time of
posting, and that such posts are visible only to the creator
(`SELF_ONLY`). TikTok writes these rules for Direct Post; whether it also
applies them to drafts sent from a sandbox is checked in the publishing
regression pass. Set the account to **Private account** in TikTok's
**Settings and privacy › Privacy** until then.

## 4. Save the app in Bardo

1. Open **Settings › Networks**.
2. Paste the client key and secret into **TikTok · TikTok for Developers
   app** and select **Save**.

Both are kept in Windows Credential Manager under your Windows account, per
Bardo profile. They never reach Bardo's database, logs or error messages,
and the secret never shows on screen again; the card shows its last four
characters.

## 5. Connect the account

1. Open **Accounts**, pick the channel and add (or open) its TikTok
   account.
2. Select **Connect**. Your browser opens TikTok's authorization page.
3. Sign in with the target user's account and allow every permission.
   Leaving one off makes Bardo refuse the connection and revoke what was
   granted.
4. The browser shows "Bardo is connected" and the card shows
   **Connected as (display name)**.

Bardo waits five minutes for the browser; **Cancel** stops waiting.

The access and refresh tokens are kept in Windows Credential Manager,
keyed by profile and network account. Bardo's database keeps only the
account's `open_id` and display name, the scopes, the token expiry and the
last refresh.

## Day to day

- TikTok's access token lasts 24 hours and its refresh token 365 days.
  Bardo renews the access when it is used or checked, and when the app
  starts if it has run out, so the connection lasts as long as Bardo is
  opened now and then. TikTok may hand back a new refresh token at each
  renewal; Bardo always keeps the newest.
- **Check** renews the access if it is close to expiring and reads the
  display name again.
- **Reconnect needed** means TikTok refused to renew the access: you
  removed Bardo from the account's authorized apps, the refresh token went
  a year unused, the client secret changed, or the sandbox no longer lists
  the account as a target user. Select **Reconnect**.
- **Disconnect** revokes Bardo's access at TikTok and forgets the tokens.
  Revoking needs the app's credentials: if they were removed from
  **Settings › Networks**, or TikTok cannot be reached, Bardo still forgets
  the tokens and tells you to remove its access yourself, from the apps and
  services permissions in the TikTok app's security settings.
- A connected account must be disconnected before it can be removed.

## Limits to know

- TikTok keeps at most 5 drafts from an app waiting in your inbox in any
  24 hours.
- The Content Posting API has no scheduling field: you schedule the draft
  in the TikTok app when you post it.
