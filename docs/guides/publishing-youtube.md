# Connecting a YouTube channel and uploading

Bardo signs in to YouTube with an OAuth client you register in your own
Google Cloud project (ADR-0008). Bardo ships no client of its own, so the
quota, the consent screen and any audit belong to your project. This guide
sets that project up once; after that, each channel connects from its
network account card in a few clicks.

## What you need

- A Google account that owns (or manages) the YouTube channel.
- Access to the [Google Cloud console](https://console.cloud.google.com/).

## 1. Create a project and enable the APIs

1. In the Google Cloud console, create a project (for example `bardo`).
2. Open **APIs & Services › Library** and enable:
   - **YouTube Data API v3** (uploads, scheduling, the connected channel);
   - **YouTube Analytics API** (views, watch time and revenue reports).

Without them, connecting fails with "Access was refused. Enable YouTube
Data API v3 and YouTube Analytics API…".

## 2. Configure the consent screen

Open **Google Auth Platform**:

1. **Branding**: give the app a name (for example `Bardo`) and your support
   email.
2. **Audience**: choose **External**. While the app is in **Testing**, add
   your own Google account under **Test users**.
3. **Data Access**: add these scopes (Bardo asks for all four at once, since
   a desktop app cannot add scopes later):
   - `https://www.googleapis.com/auth/youtube.upload`
   - `https://www.googleapis.com/auth/youtube`
   - `https://www.googleapis.com/auth/yt-analytics.readonly`
   - `https://www.googleapis.com/auth/yt-analytics-monetary.readonly`
4. Back in **Audience**, select **Publish app** to move it **In production**.

Why production: Google ends every authorization made to an app in
**Testing** seven days after consent, refresh token included. Bardo would
then show **Reconnect needed** every week. An app **In production** that
only you use does not need Google's verification; Google shows an
"unverified app" screen at consent, where you continue through
**Advanced › Go to (app name)**. Unverified apps are capped at 100 users in
total, which a personal setup never reaches.

## 3. Create the OAuth client

1. In **Google Auth Platform › Clients**, select **Create client**.
2. Application type: **Desktop app**. Name it (for example `Bardo desktop`).
3. Copy the **Client ID** (ends in `.apps.googleusercontent.com`) and the
   **Client secret** (starts with `GOCSPX-`).

A desktop client needs no redirect address: Bardo listens once on
`127.0.0.1` on a random port while you consent, and Google allows any
loopback port for desktop clients.

## 4. Save the client in Bardo

1. Open **Settings › Networks**.
2. Paste the client ID and secret into **YouTube · Google OAuth client** and
   select **Save**.

Both are kept in Windows Credential Manager under your Windows account, per
Bardo profile. They never reach Bardo's database, logs or error messages,
and the secret never shows on screen again; the card shows its last four
characters.

## 5. Connect the channel

1. Open **Accounts**, pick the channel and add (or open) its YouTube
   account.
2. Select **Connect**. Your browser opens Google's consent page.
3. Sign in with the account that owns the channel, pick the channel if
   Google asks, and allow every permission. Leaving one unticked makes
   Bardo refuse the connection and revoke what was granted.
4. The browser shows "Bardo is connected" and the card shows
   **Connected as (channel name)**.

Bardo waits five minutes for the browser; **Cancel** stops waiting.

The access and refresh tokens are kept in Windows Credential Manager,
keyed by profile and network account. Bardo's database keeps only the
channel id and name, the scopes, the token expiry and the last refresh.

## Day to day

- **Check** renews the access if it is close to expiring and reads the
  channel name again.
- **Reconnect needed** means Google refused to renew the access: it was
  revoked from your Google account, the client secret changed, or the app
  is still in **Testing** and the seven days ran out. Select **Reconnect**.
- **Disconnect** revokes Bardo's access at Google and forgets the tokens.
  If Google cannot be reached, Bardo still forgets the tokens and tells you
  to remove its access from your Google account's security settings
  (**Third-party apps & services**).
- A connected account must be disconnected before it can be removed.

## Uploading a video

1. Render the project for the YouTube account and write its metadata.
2. At the project's **Publish** stage, pick **YouTube** and select
   **Review upload**. The button stays off, with the reason under it, while
   the account is not connected, a render is running or out of date, or the
   metadata is missing or breaks YouTube's limits.
3. The review shows the file, the connected channel, and the title,
   description (with the account's footer) and tags as YouTube gets them.
   Pick the visibility, answer **Made for kids**, and check **Altered or
   synthetic content** (already on when the narrator's voice is flagged as
   realistic).
4. If the project already has a YouTube post linked, tick the box that
   replaces it in Bardo; the post itself stays on YouTube.
5. Select **Upload**. Nothing is sent before this. If the render, the cut
   or the metadata changed since the review opened, Bardo closes it and asks
   you to review again.

The upload runs as a job: the Post section shows its progress, then
**Processing** while YouTube works on the file, and **Uploaded** with the
video's link. **Stop** keeps what YouTube already received; **Resume**
sends only the rest. A dropped connection retries by itself from the same
point.

While the upload runs, Bardo refuses to render the project, since the
render would rewrite the file being sent; **Resume** waits for a running
render the same way. If the project was rendered again while the upload was
stopped, resuming ends that upload with "The render changed after the
review": the rest of the file is not what you reviewed, so review the
upload again.

Bardo checks on YouTube's processing for about an hour. If YouTube is still
processing the video after that, the post shows **Still processing**;
select **Check again** later.

Pasting a post's link over an uploaded video asks first, and replaces the
upload in Bardo only; the video stays on YouTube.

## Uploads stay private until the audit

Google locks every video uploaded through an unaudited API project created
after 2020-07-28 to **private**, and scheduled videos too; ADR-0008 records
how Bardo reports such a publication: **Kept private**, with "Kept private
by YouTube: your Google project has not passed the YouTube API audit". It
is not a failure. To publish publicly from Bardo, request the
[YouTube API Services audit](https://support.google.com/youtube/contact/yt_api_form)
for your project; it changes the result, not anything in Bardo. Until
then, you can make a video public yourself in YouTube Studio.

## Quota

Uploads draw on a separate bucket of 100 uploads per day per project; other
calls share 10,000 units per day. Connecting and checking cost 1 unit each.
When the quota runs out, Bardo says so and the counter resets at midnight
Pacific time. An upload that hits the daily upload quota stops at once
instead of retrying; select **Retry** after the reset.
