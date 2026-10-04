# TikTok video query fixtures

Raw HTTP responses (status line, headers, body) that the TikTok post
numbers tests replay instead of calling TikTok. Noise headers were dropped.
No fixture holds a real token, creator or video.

Recorded on 2026-10-04 from the live API with an access token TikTok does
not know:

- `invalid-token.http`: `POST /v2/video/query/` answers 401 with
  `access_token_invalid`.

Written from the documented shapes of
[video query](https://developers.tiktok.com/doc/tiktok-api-v2-video-query),
the [video object](https://developers.tiktok.com/doc/tiktok-api-v2-video-object)
and [error handling](https://developers.tiktok.com/doc/tiktok-api-v2-error-handling),
because recording them needs a real creator account (re-record during the
publishing regression pass if an answer misbehaves):

- `query-ok.http`: two of the videos asked for, with their counts (a
  reported zero among them); a third asked for is left out.
- `query-none.http`: none of the videos asked for is the creator's public
  video.
- `scope-not-authorized.http`: a token without `video.list`.
- `rate-limited.http`: the per-minute limit.
