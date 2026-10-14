# ADR-0013: Direct upload to X

- Status: Accepted
- Date: 2026-10-14
- Amends: ADR-0003 (X leaves export-only), ADR-0008 (X's connection and upload path), ADR-0006 (X uses the in-app scheduler)

## Context

ADR-0003 kept X export-only because video upload through its API needed a paid access tier. X's official docs, checked on 2026-10-14, now describe:

- **Pricing.** The API is pay-per-use with credits bought ahead in the Developer Console, plus an optional spending limit; there is no free tier for posting. Creating a post costs US$ 0.015, or US$ 0.20 when its text holds a URL; uploading media is not billed; reading a user (`GET /2/users/me`) costs US$ 0.010. When the credits run out or the limit is reached, requests are refused.
- **OAuth 2.0.** Authorization code with PKCE (S256). An app of type "Web App, Automated App or Bot" is a confidential client: the token endpoint takes its client id and secret as HTTP Basic. The callback must match a registered address exactly, port included; X documents `127.0.0.1` rather than `localhost`. The code expires after 30 seconds; the access token after two hours; `offline.access` grants a refresh token, and each refresh hands out a new one. Tokens are revoked one at a time with `token_type_hint`.
- **Media.** Chunked upload in three steps (`POST /2/media/upload/initialize`, `.../{id}/append` per segment, `.../{id}/finalize`) with the `media.write` scope, then `GET /2/media/upload?command=STATUS` while X processes the video. An uploaded media id stays usable for 24 hours.
- **Posts.** `POST /2/tweets` with the media id and `made_with_ai` to label AI-made media; no scheduling field. Limits: 100 posts per 15 minutes per user, 10,000 per 24 hours per app.

## Decision

1. **X uploads and posts.** X joins YouTube, TikTok and Instagram Reels as a network Bardo uploads to; Kick stays export-only. The adapters live in `publish` (`x.rs` for the sign-in, `x_upload.rs` for the upload) behind the existing `NetworkSignIn` and `VideoUploader` interfaces.
2. **The user's own app (ADR-0008).** The user registers an X Developer Console app of type "Web App, Automated App or Bot" with Read and write permissions, and saves its OAuth 2.0 client id and secret in Settings › Networks. Tokens per network account stay in Windows Credential Manager. The app's credits and spending limit are the user's.
3. **A fixed loopback port.** X matches the callback with its port, so the loopback listener for X binds `http://127.0.0.1:47147/callback`, which the guide tells the user to register. A busy port is reported as such ("the port is in use") rather than as a generic failure. Every other network keeps a random port.
4. **Bardo posts, as for Reels.** The upload job sends the file and, once X has processed it, makes the post with the reviewed text (the account footer and hashtags in it) and `made_with_ai` set from the review's AI label. X takes no publish time, so a scheduled post goes through the in-app scheduler (ADR-0006): the file is sent up to 23 hours ahead, inside the media id's 24 hours, and Bardo posts at the due time. Processing is checked once a minute for about a quarter of an hour.
5. **Posts are paid calls.** The upload review shows the price X bills for the post (a post with a link when the text holds an address or a bare domain X would link), the provider budget for X is checked before the upload is queued, and a successful post is recorded on the Costs screen under X with its project and job, in a new `posts` meter (migration 0041). A post X refused is not recorded. X has no API key: it appears among paid providers on Costs, not under Settings › API keys. Account reads at connect and Check are billed by X but not recorded; the guide says so.

## Consequences

- Setting up X takes an app, a payment card and credits; the guide (`connect-x.md`) and the "Step by step" and "Audits and reviews" screens walk through it. No audit applies to an own app.
- A wrong price heuristic can show a post with a link as plain (or the reverse); the recorded cost then differs from X's bill by at most US$ 0.185 per post, and the rate table can be edited.
- Whether a refresh token survives revoking only the access token, the exact shape of the depleted-credits error and the video length limit per account are not confirmed against a live account; they go to the real-account checks of the publishing regression pass.
