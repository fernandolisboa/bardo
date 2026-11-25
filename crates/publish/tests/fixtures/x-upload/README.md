# X upload fixtures

Raw HTTP responses (status line, headers, body) that the X upload and post
tests replay instead of calling X. Noise headers (trace ids, dates, CDN
headers) were dropped. No fixture holds a real token or a real media,
post or user id.

Recorded on 2026-10-14 from `api.x.com` with a made-up token (body
verbatim):

- `not-a-user-token.http`: `POST /2/media/upload/initialize` with a
  bearer token X does not know, which it takes for an app-only one
  (HTTP 403 `unsupported-authentication`).

Written from the documented shapes of the
[chunked media upload](https://docs.x.com/x-api/media/quickstart/media-upload-chunked),
[initialize](https://docs.x.com/x-api/media/initialize-media-upload),
[append](https://docs.x.com/x-api/media/append-media-upload),
[finalize](https://docs.x.com/x-api/media/finalize-media-upload),
[status](https://docs.x.com/x-api/media/get-media-upload-status) and
[create post](https://docs.x.com/x-api/posts/create-post) references,
because recording them needs a real X app, account and credits (re-record
during the publishing regression pass if an upload misbehaves):

- `initialize-ok.http`, `initialize-again.http`: an upload started, with
  its media id and `expires_after_secs` (a day, as in the reference).
- `append-ok.http`: a chunk taken.
- `append-unknown-upload.http`, `status-not-found.http`: an upload X no
  longer knows (404 `resource-not-found`).
- `finalize-processing.http`: a finalize with `processing_info` pending.
- `finalize-already.http`: a finalize of an upload finalized already
  (400). The message is assumed.
- `status-in-progress.http`, `status-succeeded.http`,
  `status-failed.http`, `status-no-processing.http`: each documented
  processing state, and an answer without `processing_info` (ready).
- `post-created.http`: a post made (201 with its id).
- `post-media-invalid.http`, `post-duplicate.http`,
  `post-video-too-long.http`: a media id X refuses, a duplicate text, and
  a video over the account's length (403, as the post reference says).
  The messages are assumed.
- `post-created-unreadable.http`: a post made whose answer was cut
  before its id (201 with half a body).
- `post-bad-gateway.http`: a gateway's page in place of X's answer to a
  post (502).
- `posts-with-video.http`, `posts-none.http`, `posts-more-to-come.http`,
  `posts-same-video-twice.http`, `posts-errors-only.http`: the account's
  own posts ([user posts](https://docs.x.com/x-api/users/get-posts) with
  `tweet.fields=attachments`, checked 2026-11-25): three posts, two of
  them with an uploaded video each; none in the time asked; a first page
  with a `next_token`; two posts with the same video, the newer with a
  key that is the bare media id; and a 200 that holds only `errors`. The
  media keys follow the examples of the
  [data dictionary](https://docs.x.com/x-api/fundamentals/data-dictionary)
  (a kind, an underscore, the media's id), which does not spell the rule
  out. Not recorded: re-record with the others.
- `credits-depleted.http`: an app out of credits (402 `credits`). The
  title and detail are assumed.
- `rate-limited.http`: over the 100 posts per 15 minutes per user (429).
