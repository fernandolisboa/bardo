# YouTube upload fixtures

Raw HTTP responses (status line, headers, body) that the YouTube upload
tests replay instead of calling Google. Noise headers (dates, request ids)
were dropped. No fixture holds a real token, session or video id.

Recorded on 2026-10-03 from the live API with an invalid access token:

- `videos-invalid-token.http`: `videos.list part=status` with a bearer
  token Google does not know.

Written from the documented shapes of the
[resumable upload protocol](https://developers.google.com/youtube/v3/guides/using_resumable_upload_protocol),
[videos.insert](https://developers.google.com/youtube/v3/docs/videos/insert)
and the [video resource](https://developers.google.com/youtube/v3/docs/videos),
because recording them needs a real channel and OAuth client (re-record
during the publishing regression pass if an upload misbehaves):

- `session-started.http`, `session-restarted.http`: a new resumable session,
  its address in `Location`.
- `chunk-incomplete-1.http`, `chunk-incomplete-2.http`: `308 Resume
  Incomplete` after the first and second 256 KiB chunk (`Range: bytes=0-N`).
- `status-nothing-yet.http`: a status query (`Content-Range: bytes */TOTAL`)
  before any byte arrived: 308 without `Range`.
- `status-first-chunk.http`: a status query after the first chunk arrived.
- `upload-complete.http`: `201 Created` with the video after the last chunk.
- `session-gone.http`: a session YouTube no longer knows (404).
- `server-unavailable.http`: a 503, which the protocol says to resume after.
- `quota-exceeded.http`: the project's daily uploads bucket is spent.
- `upload-limit.http`: the channel's own upload limit (`uploadLimitExceeded`).
- `invalid-title.http`: a title YouTube does not take (`invalidTitle`).
- `videos-processing.http`, `videos-processed-public.http`,
  `videos-processed-private.http`, `videos-rejected.http`,
  `videos-failed.http`: `videos.list part=status` while processing, when
  processed (as asked, and kept private), rejected and failed.
- `videos-none.http`: the video is gone.
