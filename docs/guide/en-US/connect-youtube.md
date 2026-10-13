---
id: connect-youtube
title: Connecting YouTube
group: publishing
---

# Connecting YouTube

Bardo signs in to YouTube with an OAuth client you register in your own Google Cloud project. Bardo ships no client of its own, so the quota, the consent screen and any audit belong to your project. This guide sets that project up once; after that, each channel connects from its network account card in a few clicks.

<a id="need"></a>
## What you need

- A Google account that owns (or manages) the YouTube channel.
- Access to the [Google Cloud console](https://console.cloud.google.com/).

<a id="project"></a>
## 1. Create a project and enable the APIs

1. In the Google Cloud console, [create a project](https://console.cloud.google.com/projectcreate) (for example `bardo`), or pick the one you already use for the YouTube Data API key.
2. With that project selected, open each API's page in **APIs & Services › Library** and select **Enable**:
   - [YouTube Data API v3](https://console.cloud.google.com/apis/library/youtube.googleapis.com) (uploads, scheduling, the connected channel);
   - [YouTube Analytics API](https://console.cloud.google.com/apis/library/youtubeanalytics.googleapis.com) (views, watch time and revenue reports).

Without them, connecting fails with "Access was refused. Enable YouTube Data API v3 and YouTube Analytics API…".

<a id="consent"></a>
## 2. Configure the consent screen

Open [Google Auth Platform](https://console.cloud.google.com/auth/overview). A project that has none yet shows **Get started**, which asks for the app's name, your support email, the audience (**External**) and a contact email in one go; then check each page below.

1. [Branding](https://console.cloud.google.com/auth/branding): give the app a name (for example `Bardo`) and your support email.
2. [Audience](https://console.cloud.google.com/auth/audience): choose **External**. While the app is in **Testing**, add your own Google account under **Test users**.
3. [Data Access](https://console.cloud.google.com/auth/scopes): select **Add or remove scopes** and add these (paste them under **Manually add scopes**) (Bardo asks for all four at once, since a desktop app cannot add scopes later):
   - `https://www.googleapis.com/auth/youtube.upload`
   - `https://www.googleapis.com/auth/youtube`
   - `https://www.googleapis.com/auth/yt-analytics.readonly`
   - `https://www.googleapis.com/auth/yt-analytics-monetary.readonly`
4. Back in [Audience](https://console.cloud.google.com/auth/audience), select **Publish app** to move it **In production**.

Why production: Google ends every authorization made to an app in **Testing** seven days after consent, refresh token included. Bardo would then show **Reconnect needed** every week. An app **In production** that only you use does not need Google's verification; Google shows an "unverified app" screen at consent, where you continue through **Advanced › Go to (app name)**. Unverified apps are capped at 100 users in total, which a personal setup never reaches.

<a id="client"></a>
## 3. Create the OAuth client

1. Open [Google Auth Platform › Clients](https://console.cloud.google.com/auth/clients/create) to create a client.
2. Application type: **Desktop app**. Name it (for example `Bardo desktop`).
3. Select **Create**, then copy the **Client ID** and the **Client secret** right away: Google shows the secret in full only when the client is created, and afterwards only its last four characters. The ID ends in `.apps.googleusercontent.com`; the secret usually starts with `GOCSPX-`. If you lose the secret, add a new one to the client and save that in Bardo.

A desktop client needs no redirect address: Bardo listens once on `127.0.0.1` on a random port while you consent, and Google allows any loopback port for desktop clients.

<a id="save"></a>
## 4. Save the client in Bardo

1. Open [Settings › Networks](bardo:go/settings/networks).
2. Paste the client ID and secret into **YouTube · Google OAuth client** and select **Save**.

Both are kept in Windows Credential Manager under your Windows account, per Bardo profile. They never reach Bardo's database, logs or error messages, and the secret never shows on screen again; the card shows its last four characters.

<a id="connect"></a>
## 5. Connect the channel

1. Open [Accounts](bardo:go/accounts), pick the channel and add (or open) its YouTube account.
2. Select **Connect**. Your browser opens Google's consent page.
3. Sign in with the account that owns the channel, pick the channel if Google asks, and allow every permission. Leaving one unticked makes Bardo refuse the connection and revoke what was granted.
4. The browser shows "Bardo is connected" and the card shows **Connected as (channel name)**.

Bardo waits five minutes for the browser; **Cancel** stops waiting.

The access and refresh tokens are kept in Windows Credential Manager, keyed by profile and network account. Bardo's database keeps only the channel id and name, the scopes, the token expiry and the last refresh.

<a id="day-to-day"></a>
## Day to day

- **Check** renews the access if it is close to expiring and reads the channel name again.
- **Reconnect needed** means Google refused to renew the access: it was revoked from your Google account, the client secret changed, or the app is still in **Testing** and the seven days ran out. Select **Reconnect**.
- **Disconnect** revokes Bardo's access at Google and forgets the tokens. If Google cannot be reached, Bardo still forgets the tokens and tells you to remove its access from your Google account's security settings (**Third-party apps & services**).
- A connected account must be disconnected before it can be removed.

<a id="upload"></a>
## Uploading a video

1. Render the project for the YouTube account and write its metadata.
2. At the project's **Publish** stage, pick **YouTube** and select **Review upload**. The button stays off, with the reason under it, while the account is not connected, a render is running or out of date, or the metadata is missing or breaks YouTube's limits.
3. The review shows the file, the connected channel, and the title, description (with the account's footer) and tags as YouTube gets them. Pick the visibility, answer **Made for kids**, and check **Altered or synthetic content** (already on when the narrator's voice is flagged as realistic).
4. If the project already has a YouTube post linked, tick the box that replaces it in Bardo; the post itself stays on YouTube.
5. Select **Upload**. Nothing is sent before this. If the render, the cut or the metadata changed since the review opened, Bardo closes it and asks you to review again.

The upload runs as a job: the Post section shows its progress, then **Processing** while YouTube works on the file, and **Uploaded** with the video's link. **Stop** keeps what YouTube already received; **Resume** sends only the rest. A dropped connection retries by itself from the same point.

While the upload runs, Bardo refuses to render the project, since the render would rewrite the file being sent; **Resume** waits for a running render the same way. If the project was rendered again while the upload was stopped, resuming ends that upload with "The render changed after the review": the rest of the file is not what you reviewed, so review the upload again.

Bardo checks on YouTube's processing for about an hour. If YouTube is still processing the video after that, the post shows **Still processing**; select **Check again** later.

Pasting a post's link over an uploaded video asks first, and replaces the upload in Bardo only; the video stays on YouTube.

<a id="schedule"></a>
## Scheduling

In the review, **When** offers **Once processed** or **Schedule**. With **Schedule**, type the date and time YouTube makes the video public; the fields read them in the interface language's order (MM/DD/YYYY and 6:30 PM in English, DD/MM/AAAA and 18:30 in Portuguese) and in your computer's time zone, which the card names. Bardo sends the video as private with that publish time, and YouTube publishes it by itself: Bardo and the computer can be off. A time that has already passed is refused when you confirm, since YouTube would publish the video at once.

The post then shows **Scheduled** with the time it goes public. Until then, **Change time** sends a new time and **Cancel schedule** leaves the video private on YouTube with no publish time; publishing it later is done in YouTube Studio. Both resend the made-for-kids answer and the synthetic-content disclosure YouTube already has, because YouTube clears whatever a change leaves out. A change made in YouTube Studio shows up the next time Bardo syncs metrics.

Each metrics sync reads a scheduled video back through the connected account. Once YouTube made it public, the post becomes **Posted** with YouTube's publish time and its metrics are read like any other post's. The sync needs no API key while only scheduled videos are tracked.

<a id="private"></a>
## Uploads stay private until the audit

Google locks every video uploaded through an unaudited API project created after 2020-07-28 to **private**, and scheduled videos too. Bardo reports such a publication as **Kept private**, with "Kept private by YouTube: your Google project has not passed the YouTube API audit". A scheduled video still private a quarter of an hour after its publish time is reported the same way. It is not a failure. To publish publicly from Bardo, request the [YouTube API Services audit](https://support.google.com/youtube/contact/yt_api_form) for your project; it changes the result, not anything in Bardo. Until then, you can make a video public yourself in YouTube Studio.

<a id="owner-metrics"></a>
## Owner metrics

Once the channel's YouTube account is connected, every metrics sync also reads the owner's numbers of each YouTube post it finds, linked by hand or uploaded, from the YouTube Analytics API: engaged views, views, watch time, the average view (length and percentage), the audience retention curve and, for a channel in the YouTube Partner Program, estimated revenue, CPM and playback-based CPM. RPM is worked out by Bardo as revenue per 1,000 views. The public views, likes and comments still come from the YouTube Data API key, so the sync needs the key as before.

- **No new permission.** The two `yt-analytics` scopes are among the four Bardo asks for at connection, and a connection missing any of them is refused, so a channel connected before owner metrics existed reads them without reconnecting.
- **Engaged views lead.** YouTube's views now count every start of a video (Shorts since March 2025, every format since August 2026); engaged views keep the earlier meaning, so they are the headline number on the post and on Strategy › Performance.
- **Not monetized.** A channel outside the Partner Program gets a refusal for revenue reports. Bardo shows **Not monetized** in their place and reads everything else.
- **Two or three days late.** YouTube Analytics data arrive 48 to 72 hours after the fact; the post says so under its owner numbers. A new video shows only its public numbers until its first analytics arrive.
- **Reconnect needed.** While the account needs to reconnect, syncs keep reading the public numbers, and Strategy › Performance asks you to reconnect. A channel that was never connected keeps exactly the public numbers it had.
- Theme ranking's past performance keeps reading the public views: owner numbers trail by days and a channel's older posts may have none.

<a id="quota"></a>
## Quota

Uploads draw on a separate bucket of 100 uploads per day per project; other calls share 10,000 units per day. Connecting, checking and reading a scheduled video back cost 1 unit each; changing or cancelling a schedule costs 51 (a read, then the update). YouTube Analytics has its own quota, apart from the Data API's: each sync asks it for two reports per post of a connected channel (the numbers and the retention curve), one unit each, plus one more per sync for a channel that is not monetized (its first revenue report is refused, then read again without revenue). When the quota runs out, Bardo says so and the counter resets at midnight Pacific time. An upload that hits the daily upload quota stops at once instead of retrying; select **Retry** after the reset.
