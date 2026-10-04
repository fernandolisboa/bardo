---
id: connect-instagram
title: Connecting Instagram
group: publishing
---

# Connecting Instagram

Bardo signs in to Instagram through a Meta app you register in your own Meta developer account, with the Instagram API with Facebook Login for Business. Bardo ships no app of its own, so the app, its access level and any review belong to you. This guide sets the app up once; after that, each Instagram account connects from its network account card.

<a id="need"></a>
## What you need

- An Instagram **professional** account (Business or Creator).
- A **Facebook Page** linked to that Instagram account. Uploading a local video file is documented only for apps that use Facebook Login for Business, and that login reaches Instagram through the linked Page.
- A Facebook account with a role on the Page (it can create content there) and on the Meta app below.
- Access to [Meta for Developers](https://developers.facebook.com/apps/).

To link the account to a Page: in Instagram, open **Settings › Accounts Center** (or **Edit profile › Page**) and connect the Page, or do it from the Page's settings in Meta Business Suite.

<a id="app"></a>
## 1. Create the Meta app

1. In **My Apps**, select **Create app**.
2. Use case: **Manage messaging & content on Instagram** (the Instagram use case). App type, if asked: **Business**. Do not set the app type to Native or Desktop: Facebook Login for Business needs a web login.
3. In the use case's settings, open **API setup with Facebook login** and add **Facebook Login for Business** if it is not there yet.
4. In **App settings › Basic**, copy the **App ID** (a number) and the **App secret** (select **Show**).

The app stays in **Development** mode with **Standard Access**: that serves every account whose Facebook user has a role on the app, which is you. No App Review and no Business Verification are needed for your own accounts.

<a id="save"></a>
## 2. Save the app in Bardo

1. Open [Settings › Networks](bardo:go/settings/networks).
2. Paste the app ID and secret into **Instagram · Meta app** and select **Save**.

Both are kept in Windows Credential Manager under your Windows account, per Bardo profile. They never reach Bardo's database, logs or error messages, and the secret never shows on screen again; the card shows its last four characters.

<a id="token"></a>
## 3. Generate a token

Facebook Login does not hand a desktop app its sign-in through the browser the way Google does: it only returns to an exact HTTPS address or to an embedded web view. So you sign in once in Meta's Graph API Explorer and paste the token it gives into Bardo.

1. Open the [Graph API Explorer](https://developers.facebook.com/tools/explorer/) (the account card's **Open the Graph API Explorer** button goes there).
2. Under **Meta App**, pick your app. Under **User or Page**, choose **Get User Access Token**.
3. Tick these permissions:
   - `instagram_basic`
   - `instagram_content_publish`
   - `instagram_manage_insights`
   - `pages_show_list`
   - `pages_read_engagement`

   If the Page belongs to a business portfolio (Meta Business Suite) and your role on it comes from there, also tick `business_management`, `ads_management` and `ads_read`.
4. Select **Generate Access Token**, log in, and in the dialog choose the Page and the Instagram account Bardo may use. Allow every permission.
5. Copy the token from the **Access Token** field.

The token lasts about an hour, so paste it in Bardo right away.

<a id="connect"></a>
## 4. Connect the account

1. Open [Accounts](bardo:go/accounts), pick the channel and add (or open) its Instagram Reels account.
2. Select **Connect**, paste the token and select **Connect** again.
3. Bardo trades the token for a long-lived one, checks that every permission was allowed, and finds the Pages it reaches with their linked Instagram account:
   - one account: the card shows **Connected as @username**;
   - several: the card lists each one with its Page; pick the one for this channel with **Connect this one**;
   - no Page, or no Page with a linked Instagram professional account: the card says which, and nothing is kept.

Bardo never stores the token you pasted. It keeps, in Windows Credential Manager per profile and network account, the long-lived user token and the chosen Page's token, which is what publishes. Bardo's database keeps only the Instagram account id and username, the permissions, the expiry and the last renewal.

<a id="day-to-day"></a>
## Day to day

- **The long-lived token lasts about 60 days.** Once it is within a week of expiring, Bardo trades it for a fresh one, and reads the Page's token again, the next time you open Bardo or select **Check**. If Bardo is not opened during that last week, the token runs out and the card asks you to reconnect with a new token.
- **Check** does that renewal if it is due and reads the account's username again. If the Page is now linked to another Instagram account, the card turns to **Reconnect needed**.
- **Reconnect needed** means Meta refused the token: it expired, it was revoked (removing the app from your Facebook settings does this), your password changed, Meta asked you to re-authorize the app (data access expires 90 days after you last used it in a Meta login), or the Page is no longer linked. Generate a new token as in step 3 and select **Reconnect**.
- **Disconnect** removes Bardo's access at Meta and forgets the tokens. Meta revokes an app's access for the whole Facebook account at once, so when another account in the same Bardo profile is still connected through the same app, Bardo forgets this account's tokens and leaves the access in place, and says so. If Meta cannot be reached, Bardo still forgets the tokens and tells you to remove the app from your Facebook account's **Settings & privacy › Business integrations**.
- A connected account must be disconnected before it can be removed.

<a id="upload"></a>
## Uploading a Reel

At the Publish stage, a connected Instagram Reels account offers **Review upload** once its preset is rendered and its caption written.

- **Reel specs.** Before the review opens, Bardo checks the rendered file against what Instagram takes as a Reel: MP4 or MOV in fast start, H.264 or HEVC, 23 to 60 frames per second, at most 1920 pixels wide, 3 seconds to 15 minutes and at most 300 MB. Each one it fails is listed under the upload, and the review stays closed until a new render passes.
- **The review** shows the file, the account, and the caption with its hashtags as Instagram gets them. You choose the **cover** (the frame at that time, as seconds or m:ss), **Also show in Feed** (on: the Reel also shows in your grid and followers' feeds), and the **AI info label**, on when the narration used a realistic voice.
- **Upload and publish** sends the file and, once Instagram has processed it, publishes the Reel. Bardo checks once a minute; if Instagram is still processing after about a quarter of an hour, the post shows **Still processing** and **Check again** picks it up later.
- **Publishing limit.** Instagram lets an account publish a set number of posts through apps in 24 hours (Bardo reads the number from Instagram). Over it, the Reel stays queued and the post says when it goes. When other apps also published on the account, Bardo can't tell when their posts leave the window, so it says when it reads the limit again (within the hour) instead. Stopping and resuming it reads the limit again.
- **Stop and resume.** A stopped or interrupted upload resumes from what Instagram already has. A container left unpublished for 24 hours expires at Instagram; Bardo then sends the file again in a new one, twice at most.
- If Instagram publishes the Reel with a warning (for example that it left the audio out), the post shows Instagram's warning.
- If the account is reconnected as another Instagram account before the Reel goes, the upload stops and asks for a new review.

<a id="schedule"></a>
## Scheduling a Reel

In the review, **When** offers **Once processed** or **Schedule**. Instagram takes no publish time from apps, so with **Schedule** Bardo sends the file ahead and publishes the Reel itself at the date and time you type (read in your computer's time zone, which the review names). The post shows **Scheduled in Bardo** with the time it posts, and **Schedule in Bardo** confirms the review.

Keep Bardo open at that time and the computer on. If Bardo is closed or the computer is off then, nothing is posted: the next time Bardo opens, it lists the post among the [missed posts](missed-posts.md), for you to post it now, give it a new time or cancel it. A time that has already passed is refused when you confirm.

<a id="metrics"></a>
## Numbers on the Performance screen

With the account connected, every metrics sync reads the insights of the channel's Reels: the ones Bardo published, and posts linked with **Mark as posted**. No YouTube key is needed for them.

- **What it reads:** views, reach, likes, comments, shares, saves, interactions and, on a Reel, average and total watch time. Instagram reports no revenue.
- **Late data.** Instagram's numbers arrive up to two days after posting. Until then the post says it has no numbers yet, and a number Instagram leaves out shows as "—", not 0.
- **Linked posts.** A pasted link carries a shortcode, not the id insights take, so the first sync looks for the post in the account's media (the newest 2,000) and keeps its id. A linked post the account doesn't have shows as **Not found**.
- **Cost.** About one request per Reel per sync (two for a feed post), plus the media list for a linked post until the sync finds it there.
- If Instagram won't show a post's insights (it holds them back from posts with too few viewers), the post keeps its link and waits for the next sync; it is not marked **Not found**.
- Without a connected account, a linked post keeps only its link. When the account needs to reconnect, syncs skip its posts until it does.

<a id="authorization"></a>
## If the Page needs publishing authorization

Meta may ask a Page's admins to complete **Page Publishing Authorization** (an identity check in the Page's settings) before anything is published through it. If an upload is refused for that reason, complete it in the Page's settings, then try again.
