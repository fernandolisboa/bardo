# ADR-0003: Publishing networks in the MVP

- Status: Accepted
- Date: 2026-09-30

## Context

Automatic publishing depends on each platform's API access rules, which are outside our control and can take weeks to clear. Findings from the official docs (checked 2026-09-30):

- **YouTube** ([videos.insert](https://developers.google.com/youtube/v3/docs/videos/insert)): uploads from unverified API projects created after 2020-07-28 are restricted to private. The project must pass the YouTube API audit to publish public videos. `status.publishAt` supports native scheduling; `status.containsSyntheticMedia` declares synthetic content. Upload cost is 1 unit in the Video Uploads quota bucket.
- **TikTok** ([Content Posting API](https://developers.tiktok.com/doc/content-posting-api-get-started)): content from unaudited clients is restricted to private viewing. No scheduling parameter is documented; posting is immediate.
- **Instagram** ([Content Publishing](https://developers.facebook.com/docs/instagram-platform/content-publishing)): Reels publishing requires an Instagram Professional account. App Review is required only for users without a role on the app, so a single user who owns the app can publish in development mode. Limit: 50 API-published posts per 24h. Resumable upload supports local video files. No native scheduling documented.
- **X**: video upload through the API requires a paid access tier.
- **Kick**: no public upload API for videos was found.

## Decision

- **Automatic upload and scheduling**: YouTube, TikTok and Instagram Reels. YouTube and TikTok are the primary focus.
- **Export** (file in the network's render preset + metadata to copy) for **all five** networks, including those with automatic upload.
- X and Kick are export-only until their API situation changes; each can become a `publish` adapter later without touching the core.

## Consequences

- Actions for the owner, to start in parallel with development because they gate public posting:
  1. Google Cloud project with YouTube Data API v3 enabled, OAuth consent screen, then the YouTube API audit request.
  2. TikTok for Developers app with Content Posting API, then the audit request.
  3. Instagram Professional account and a Meta app with Instagram publishing; add the owner's account as an app role (no App Review needed for own use).
- Until audits clear, YouTube and TikTok uploads land as private. The app must surface this clearly instead of failing silently.
- The Instagram 50/24h limit is enforced client-side before queuing.
