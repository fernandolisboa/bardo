# Instagram Reels upload fixtures

Raw HTTP responses (status line, headers, body) that the Instagram upload
tests replay instead of calling Meta. Noise headers (trace ids, dates,
proxy and debug headers) were dropped. No fixture holds a real token,
container or media id.

Recorded on 2026-10-04 from the live hosts with a made-up token:

- `container-invalid-token.http`: `POST /<IG_USER_ID>/media` on the Graph
  API (v25.0) with a token Meta cannot read (code 190).
- `rupload-not-authorized.http`: a `POST` to
  `rupload.facebook.com/ig-api-upload/v25.0/<container>` with the same
  token. The upload host answers `debug_info`, not a Graph error.

Written from the documented shapes of
[Content Publishing](https://developers.facebook.com/docs/instagram-platform/content-publishing),
[Resumable Uploads](https://developers.facebook.com/docs/instagram-platform/content-publishing/resumable-uploads),
the [IG Container](https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/ig-container)
and [content_publishing_limit](https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/ig-user/content_publishing_limit)
references and the
[error codes](https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/error-codes),
because recording them needs a real Meta app and an Instagram professional
account linked to a Page (re-record during the publishing regression pass
if an upload misbehaves):

- `container-created.http`, `container-recreated.http`: a resumable REELS
  container, with its `id` and upload `uri`.
- `rupload-ok.http`: the whole file arrived (`"success": true`).
- `rupload-retriable.http`: the upload host failing on its side with
  `retriable: true`. The guide names the field but shows no failure, so
  the shape follows the recorded `debug_info`.
- `status-*.http`: `GET /<container>?fields=id,status,status_code,video_status`.
  `status-published.http` follows the guide's example. The others are the
  same shape with each documented `status_code` (`IN_PROGRESS`,
  `FINISHED`, `ERROR`, `EXPIRED`). `status-partial.http` (4 bytes of 10
  arrived) and `status-nothing-yet.http` assume `uploading_phase` reads
  `in_progress` and `not_started` before the file is whole: the guide only
  shows `complete`.
- `status-finished-bare.http`: `FINISHED` without `video_status`, in case
  Meta leaves the phases out of a finished container.
- `status-not-found.http`: a container Meta does not have (code 100,
  subcode 33).
- `limit-room.http`, `limit-full.http`: `content_publishing_limit` with
  `fields=config,quota_usage`, with room and with none.
- `limit-odd-window.http`: a made-up answer whose `quota_duration` is far
  past any real window, which the uploader does not trust.
- `limit-no-permission.http`: the limit read without the permission it
  needs (code 10).
- `publish-ok.http`: `media_publish` with the new media id.
- `publish-config-issue.http`: the same with a `config_issue`. The
  reference does not describe this field; the text shape is assumed, and
  the uploader also reads an object with a message or a list.
- `publish-not-ready.http`, `publish-limit.http`, `publish-expired.http`:
  `media_publish` refused with the documented codes 9007/2207027 (not
  ready yet), 9/2207042 (the publishing limit) and -2/2207020 (the
  container expired).
- `permalink.http`: `GET /<media>?fields=permalink,shortcode`.
- `rate-limited.http`: code 4, the app's request limit.
