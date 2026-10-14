---
id: platform-audits
title: Platform audits and reviews
group: publishing
---

# Platform audits and reviews

Each network can audit or review the app you registered for Bardo. Because that app is yours and serves only your own accounts, most of it does not apply: Bardo uploads, schedules and reads your numbers without any audit. This page says, network by network, when a review is needed, what it asks, what Bardo already gives you for it and what changes once it is approved. Nothing here needs doing to use Bardo; it is for when you decide to.

In [Settings › Networks](bardo:go/settings/networks), each app card's **Audits and reviews** opens its network's part of this page as a step-by-step screen.

<a id="summary"></a>
## Do you need one?

| Network | Needed today? | What the review asks | What changes in Bardo |
| --- | --- | --- | --- |
| YouTube | Only for quota above the defaults, and possibly for public uploads (see [Kept private](#youtube-private)) | Compliance with YouTube's Developer Policies, a privacy policy, screenshots and a demo account | Nothing in code; your project's quota |
| TikTok | No, and it could not pass: personal tools are rejected. Drafts to your inbox work from the sandbox | A public website with privacy policy and terms, a demo video of the whole flow, a reason per scope | Nothing, unless Bardo ships an app of its own |
| Instagram | No: Standard Access serves the accounts you own or manage | App Review (a screen recording per permission) and Business Verification, for Advanced Access | Nothing, unless accounts you don't manage should connect |
| X | No: X has no app review; access is paid per use from credits | Accepting the Developer Agreement and Policy, and a short description of your use | Nothing |

Facts checked on 2026-10-13 (X on 2026-10-14) against each network's documentation, linked in each section.

<a id="youtube-need"></a>
## YouTube: do you need the audit?

Google's [quota and compliance audits](https://developers.google.com/youtube/v3/guides/quota_and_compliance_audits) page gives every project that enables the YouTube Data API 100 `videos.insert` calls (uploads), 100 `search.list` calls and 10,000 units for everything else, per day. The audit is how a project asks for more. With one channel and a few uploads a day, the defaults are plenty: an upload costs 1 unit of its own bucket, and checking, scheduling and syncing metrics cost a few units each.

You may want it if:

- you upload more than 100 videos a day from one project, or run more than 100 niche searches a day (each niche costs one `search.list` call);
- every upload comes back **Kept private** (see the next section).

The audit is separate from Google's **OAuth app verification**, which concerns the consent screen. An app for [your personal use, under 100 users](https://support.google.com/cloud/answer/13464323) needs no verification; you click through the "unverified app" screen once, as connecting YouTube describes.

<a id="youtube-private"></a>
## If YouTube kept a video private

Bardo shows **Kept private** when YouTube has the video as private although you chose public or unlisted (for a scheduled video, still private a quarter of an hour after its time). YouTube does not say why, so the post says only that YouTube kept it private. The causes known so far:

1. **The 2020 rule for unaudited projects.** Since 2020-07-28, YouTube locked uploads from unverified API projects created after that date to private until the project passed the audit. Google's pages now disagree: the [`videos.insert`](https://developers.google.com/youtube/v3/docs/videos/insert) reference, updated 2026-10-08, says "Videos uploaded from unverified API projects are not restricted to private viewing mode", while the [revision history](https://developers.google.com/youtube/v3/revision_history) still carries the 2020 rule and no entry lifting it. If every upload from Bardo stays private, your project is under this rule.
2. **A change made in YouTube Studio.** You (or another manager of the channel) made the video private or cancelled its schedule before Bardo checked on it.
3. **A restriction YouTube applied to the video.** Open the video in [YouTube Studio](https://studio.youtube.com/): its **Visibility** and **Restrictions** columns show what YouTube applied and why.

Either way, the video is on YouTube: make it public in YouTube Studio. Under the 2020 rule, every later upload stays private until the audit passes.

<a id="youtube-asks"></a>
## YouTube: what the audit asks

The audit checks that the app follows the [YouTube API Services Terms and Developer Policies](https://developers.google.com/youtube/terms/developer-policies). The [Audit and Quota Extension Form](https://support.google.com/youtube/contact/yt_api_form) asks for:

- who you are: individual or organization, name, address, website and contacts;
- how you work with YouTube: a description of the business (100 to 5,000 characters), the audience and how it makes money;
- the API client: its name, the address where it is used, a **privacy policy URL** (required), a terms of service URL, and a demo account with instructions for the reviewer;
- per Google Cloud project: the **project number**, the use case (for Bardo, "Video Uploading & Account Management"), the methods it calls, the quota it wants for `videos.insert` and `search.list` with the reason, and screenshots of the privacy policy, of the home page linking to it, of the sign-in flow and of the upload screen.

**What Bardo has for it.** The policies ask for a privacy policy that names the YouTube API Services and links to Google's, a link to YouTube's Terms of Service in the app, and stored YouTube data refreshed or deleted within 30 days. Bardo's [privacy policy](https://fernandolisboa.github.io/bardo/en-US/privacy.html) is in this guide and on its website, and Bardo asks you to accept it before using YouTube; [Settings › About](bardo:go/settings/about) links to it, to YouTube's Terms and to Google's Privacy Policy, and deletes the YouTube data on request; and Bardo deletes by itself what it may no longer keep ([Where it is kept, and for how long](https://fernandolisboa.github.io/bardo/en-US/privacy.html#kept)).

<a id="youtube-form"></a>
## YouTube: the form step by step

1. In the [Google Cloud console](https://console.cloud.google.com/home/dashboard), pick the project of your OAuth client and copy its **Project number** from the **Project info** card.
2. Open the [Audit and Quota Extension Form](https://support.google.com/youtube/contact/yt_api_form) signed in with the Google account that owns the project. Choose a quota extension (or a re-audit if one passed before).
3. Fill in who you are and your business as the form asks.
4. Under the API client, name it as your consent screen does (the form asks whether the name contains "YouTube"), give the privacy policy's address, `https://fernandolisboa.github.io/bardo/en-US/privacy.html` (**Privacy policy online** in Settings › About opens it), and say it is a desktop app installed on your own computer, not a website.
5. For the project, paste the project number, choose **Video Uploading & Account Management** and tick the methods below. For the description of how the client uses the API, select **Copy for the form** under these steps and paste the text. It is in English, as Google's reviewers read it:

   ```text
   Bardo is a Windows desktop app that one creator installs on their own computer to publish videos to their own YouTube channel. It signs in with the creator's own OAuth client (Desktop app, loopback redirect, PKCE). Every upload is reviewed and confirmed by the creator first; nothing is posted on a timer the creator did not set.

   YouTube Data API v3, with the creator's OAuth token:
   - videos.insert (resumable upload): uploads a rendered video with its title, description, tags, privacy status, publish time, made-for-kids answer and altered or synthetic content disclosure.
   - videos.list (part=snippet,status): reads back the processing and privacy status and the publish time of the creator's own uploads.
   - videos.update (part=status): changes or cancels the publish time of the creator's scheduled videos.
   - channels.list (mine=true): shows which channel is connected, and confirms once a day that the authorization still works.
   - videos.list (part=statistics): public views, likes and comments of the creator's own posts.

   YouTube Analytics API, with the creator's OAuth token:
   - reports.query (ids=channel==MINE): views, engaged views, watch time, average view, audience retention and, for channels in the YouTube Partner Program, estimated revenue and CPM of the creator's own videos.

   YouTube Data API v3, with the creator's API key (public data only):
   - search.list, videos.list and channels.list: niche research, reading recent public videos, their view counts and their channels' subscriber counts in a market.
   - videos.list (part=statistics): public views, likes and comments of the creator's own posts while their channel is not connected.

   OAuth scopes:
   - https://www.googleapis.com/auth/youtube.upload: upload the creator's videos.
   - https://www.googleapis.com/auth/youtube: read the connected channel, and read and change the status and publish time of the creator's videos.
   - https://www.googleapis.com/auth/yt-analytics.readonly: the owner's analytics reports.
   - https://www.googleapis.com/auth/yt-analytics-monetary.readonly: the revenue and CPM reports.

   Data handling: everything stays on the creator's computer. Tokens and keys are kept in Windows Credential Manager; channel ids, video ids and metrics are kept in a local database. Bardo runs no server. The creator accepts Bardo's privacy policy (https://fernandolisboa.github.io/bardo/en-US/privacy.html) before any YouTube feature works; the app links to it, to the YouTube Terms of Service and to the Google Privacy Policy.

   Retention: nothing read from YouTube is kept more than 30 days, except the connected channel's own numbers while its authorization is confirmed, once a day, within the last 30 days. Niche research, theme rankings and numbers read with the API key are deleted 30 days after reading; a connection unconfirmed for 30 days is removed with its data; a daily job does the deleting. Disconnecting in the app revokes the token at Google and deletes the channel's Authorized Data at once; Settings › About deletes all stored YouTube data on request.

   Sharing: only the creator's AI providers (Anthropic Claude for ideas, TypeSafe JEV for ranking) receive a few aggregate numbers (a niche's competition and trend scores, median views per day, upload volume, and first-week views of the creator's own videos), never videos, titles or channels read from YouTube.
   ```

6. Set the quota you want for `videos.insert` and `search.list` and say why, for example the number of videos a day across your channels.
7. Attach the screenshots the form asks for: the privacy policy, the page linking to it (Settings › About, its **Privacy and YouTube** section), Google's consent screen while connecting a channel, and Bardo's upload review.
8. For the demo account, give the reviewer a test Google account with a test channel, never your own.

Google's team writes back by email; the page gives no expected time.

<a id="youtube-after"></a>
## YouTube: what changes after approval

- Your project gets the quota the reviewers granted, on the Google Cloud console's **Quotas** page. Bardo keeps no count of YouTube's quota of its own: it learns the limit from YouTube's answers, so it uses the new one at once.
- Under the 2020 rule, uploads from then on stay public or unlisted as you chose. Videos already kept private stay private: make them public in YouTube Studio.
- Nothing changes in Bardo or its settings. Google may audit the project again later; keep the privacy policy and the policies' rules in place.

<a id="tiktok-need"></a>
## TikTok: do you need the audit?

No, and it could not pass. TikTok's [Content Sharing guidelines](https://developers.tiktok.com/doc/content-sharing-guidelines) list "A utility tool to help upload contents to the account(s) you or your team manages" as not acceptable, and its [App Review guidelines](https://developers.tiktok.com/doc/app-review-guidelines) say "Apps must not be for private or personal use". A personal Bardo setup is exactly that.

It is not needed either. Bardo sends TikTok videos to your inbox as drafts from the app's sandbox, with up to 10 of your own accounts as target users. The audit governs Direct Post (posting without the TikTok app), which Bardo does not build: TikTok restricts unaudited Direct Post clients to private posts from private accounts, 5 users a day.

<a id="tiktok-asks"></a>
## TikTok: what App Review asks

For an app meant for the public, TikTok's [App Review guidelines](https://developers.tiktok.com/doc/app-review-guidelines) ask for:

- a website that is more than a landing or login page, with visible links to a privacy policy and terms of service;
- an app icon and a description;
- at least one demo video (up to five) of the complete flow, recorded in the sandbox;
- a detailed explanation of how each product and scope is used;
- for Direct Post, the posting screen TikTok's [Content Sharing guidelines](https://developers.tiktok.com/doc/content-sharing-guidelines) require: the creator's nickname, a visibility the creator picks with no default, comments, duets and stitches off until the creator turns them on, the commercial content disclosure, and the creator's express consent before anything is sent.

Apps still in development or testing are not approved.

<a id="tiktok-after"></a>
## TikTok: what changes after approval

For your own setup, nothing: the sandbox keeps working and drafts go on landing in your inbox. An approved app would matter only if Bardo shipped an app of its own for other people, with Direct Post built to TikTok's rules; that is a separate decision, not part of Bardo today.

<a id="instagram-need"></a>
## Instagram: do you need App Review?

No. Meta's [Instagram Platform overview](https://developers.facebook.com/docs/instagram-platform/overview/) says "If your app only serves your Instagram professional account or an account you manage, Standard Access is all your app needs." Standard Access needs no App Review and no Business Verification: it serves every Facebook user with a role on the app (and on a business portfolio that claimed it), which is you.

Keep the app in **Development** mode. Meta's [app modes](https://developers.facebook.com/docs/development/build-and-test/app-modes/) page says an app in **Live** mode can use only the permissions approved through App Review, even for people with a role on the app, so switching it to Live without a review stops Bardo from publishing. The same page says data an app creates in Development mode, "such as test posts", can be seen only by people with a role on it. It is unconfirmed whether that covers Reels published to your own account: check your first Reel from a browser where you are signed out.

<a id="instagram-asks"></a>
## Instagram: what App Review asks

Advanced Access, for accounts you don't own or manage, needs [App Review](https://developers.facebook.com/docs/app-review/) and [Business Verification](https://developers.facebook.com/docs/development/release/business-verification/). The [submission guide](https://developers.facebook.com/docs/resp-plat-initiatives/individual-processes/app-review/submission-guide/) asks for:

- an app icon (1024 × 1024), a privacy policy URL and a user data deletion URL or instructions;
- for each permission (`instagram_basic`, `instagram_content_publish`, `instagram_manage_insights`, `pages_show_list`, `pages_read_engagement`): its own description of how the app uses it, a screen recording showing that use, and at least one successful call made in the last 30 days;
- instructions for the reviewer to reach the app, without your personal credentials;
- the data handling questions, and Business Verification of the business behind the app.

Meta says a decision usually comes within a week. Apps with Advanced Access also complete Meta's yearly Data Use Checkup.

<a id="instagram-after"></a>
## Instagram: what changes after approval

For your own accounts, nothing: Standard Access already serves them. Advanced Access would let Instagram accounts whose owners have no role on your app connect to it, which Bardo does not offer: each person who uses Bardo registers their own app.

<a id="x-need"></a>
## X: do you need a review?

No. X's [Getting access](https://docs.x.com/x-api/getting-started/getting-access) page sets an app up in three steps (sign up in the Developer Console, create the app, save its credentials) with no review in between. Access is [paid per use](https://docs.x.com/x-api/getting-started/pricing) from credits you buy, so what limits an app is its credits, its spending limit and X's rate limits (for posts, 100 per 15 minutes per account and 10,000 per day per app), not an approval.

<a id="x-asks"></a>
## X: what X asks

- In the Developer Console: accepting the Developer Agreement and Policy, and basic information about how you'll use the API.
- For the app: a name, a description and a use case.
- Afterwards, following the Developer Policy and X's automation rules: Bardo posts only what you reviewed, from your own account, one post per video.

<a id="x-after"></a>
## X: what changes after approval

There is no approval to wait for. More volume than pay per use allows (3 million post reads a month, which Bardo never comes near) needs X's Enterprise plan, which Bardo does not need.
