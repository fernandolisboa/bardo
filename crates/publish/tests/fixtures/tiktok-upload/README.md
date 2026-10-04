# TikTok draft upload fixtures

Raw HTTP responses (status line, headers, body) that the TikTok upload
tests replay instead of calling TikTok. Noise headers (trace ids, timing,
dates, CDN headers) were dropped. No fixture holds a real token, publish
id or upload address.

Recorded on 2026-10-04 from the live hosts with a made-up token and upload
address (bodies verbatim):

- `init-invalid-token.http`: `POST /v2/post/publish/inbox/video/init/`
  with an access token TikTok does not know (HTTP 401).
- `status-invalid-token.http`: `POST /v2/post/publish/status/fetch/` with
  the same token.
- `chunk-unknown-slot.http`: a `PUT` of a chunk to
  `open-upload.tiktokapis.com/video/?upload_id=…&upload_token=…` with an
  upload id and token TikTok never handed out. Its upload edge answers an
  HTML 405, not the documented 404, so the uploader takes both for an
  upload TikTok does not know.

Written from the documented shapes of the
[upload to inbox](https://developers.tiktok.com/doc/content-posting-api-reference-upload-video),
[media transfer](https://developers.tiktok.com/doc/content-posting-api-media-transfer-guide)
and [post status](https://developers.tiktok.com/doc/content-posting-api-reference-get-video-status)
references, because recording them needs a real TikTok app and account
(re-record during the publishing regression pass if an upload misbehaves):

- `init-ok.http`, `init-again.http`: an init with its `publish_id` and
  `upload_url`, in the reference's example shape.
- `init-pending-cap.http`: `spam_risk_too_many_pending_share` (HTTP 403),
  five drafts already waiting in the inbox. The message is made up.
- `init-scope-not-authorized.http`, `init-rate-limited.http`: the
  documented `scope_not_authorized` (401) and `rate_limit_exceeded` (429).
- `chunk-partial.http`, `chunk-complete.http`: a chunk taken with more to
  come (206, with `Content-Range`) and the last chunk (201).
- `chunk-expired.http`: an upload address past its hour (403).
- `chunk-out-of-range.http`: 416, with the `Content-Range` TikTok has.
  The guide says every chunk answer carries one; that a 416 does is
  assumed.
- `chunk-unavailable.http`: a 503 from the upload host.
- `status-*.http`: each documented `status` (`PROCESSING_UPLOAD`,
  `SEND_TO_USER_INBOX`, `PUBLISH_COMPLETE`, `FAILED` with a
  `fail_reason`) and `invalid_publish_id` (400).

Synthetic, for an edge case: `init-elsewhere.http` (an init whose upload
address is not on TikTok's hosts over HTTPS, which the uploader refuses).
