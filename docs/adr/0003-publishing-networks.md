# ADR-0003: Publishing networks in the MVP

- Status: Accepted; upload paths amended by [ADR-0008](0008-network-connections.md); X upload by [ADR-0013](0013-x-direct-upload.md)
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
- X and Kick are export-only until their API situation changes; each can become a `publish` adapter later without touching the core. X became one with [ADR-0013](0013-x-direct-upload.md) (pay-per-use API, 2026-10-14); Kick stays export-only.

## Consequences

- Actions for the owner, which gate public posting. Deferred until creation and editing are usable (see ADR-0002):
  1. Google Cloud project with YouTube Data API v3 enabled, OAuth consent screen, then the YouTube API audit request.
  2. TikTok for Developers app with Content Posting API, then the audit request.
  3. Instagram Professional account and a Meta app with Instagram publishing; add the owner's account as an app role (no App Review needed for own use).
- Until audits clear, YouTube and TikTok uploads land as private. The app must surface this clearly instead of failing silently.
- The Instagram 50/24h limit is enforced client-side before queuing.

## Update (2026-10-03)

The docs checked again for the publishing phase correct three findings above; [ADR-0008](0008-network-connections.md) holds the decisions that follow from them.

- **YouTube**: `videos.insert` now draws on a separate Video Uploads bucket of 100 uploads per day per project. The private lock for unaudited projects is unchanged.
- **TikTok**: the audit excludes personal tools, so a client used only for the owner's own account stays unaudited, posts only as `SELF_ONLY` and needs the account itself private. Bardo uploads TikTok videos to the creator's inbox as drafts instead of posting directly.
- **Instagram**: local-file upload needs Facebook Login for Business and a Facebook Page linked to the professional account. The publishing limit reads 100 per 24 h in the guide and 50 in the API reference, so it is read from `content_publishing_limit` rather than fixed at 50.

## Update (2026-10-13)

YouTube's [`videos.insert`](https://developers.google.com/youtube/v3/docs/videos/insert) reference, updated 2026-10-08, now reads "Videos uploaded from unverified API projects are not restricted to private viewing mode." The [revision history](https://developers.google.com/youtube/v3/revision_history) still carries the 2020-07-28 rule and has no entry lifting it, so the private lock above may or may not still apply. Bardo marks a publication restricted only when YouTube returns it private, so its behavior is right either way; the "Kept private" message no longer names the audit as the cause, and the guide's [Platform audits and reviews](../guide/en-US/platform-audits.md) page lists the causes and follows whatever a real upload shows. The same check found the default quota is now 100 `videos.insert` calls, 100 `search.list` calls and 10,000 units for everything else per day.
