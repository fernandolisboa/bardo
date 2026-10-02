# Higgsfield fixtures

Raw HTTP responses (status line, headers, body) that the clip tests replay
instead of calling the Higgsfield API: `POST /files/generate-upload-url`, the
presigned upload `PUT`, `POST /estimate/{model}`, `POST /{model}`,
`GET /requests/{id}/status` and the clip download. Noise headers were
dropped; storage and CDN hosts are placeholders.

Same body as the live API with an invalid key (recorded for the key check on
2026-10-01): `submit-rejected.http`.

Written from the documented response shapes (docs.higgsfield.ai: requests
and lifecycle, file uploads, errors, rate limits, billing), because
recording them needs a funded key. Re-record when clips misbehave with a
real key:

- `upload-url.http`, `upload-stored.http`, `estimate.http`,
  `submit-queued.http`.
- `status-queued.http`, `status-in-progress.http`, `status-completed.http`,
  `status-failed.http`, `status-nsfw.http`, `status-canceled.http`,
  `status-not-found.http`.
- `submit-no-credits.http` (403), `submit-concurrency.http` (400 when the
  account's concurrent requests are taken), `submit-invalid.http` (422 with
  a validation list), `submit-model-not-found.http` (404),
  `submit-overloaded.http` (503).

Synthetic, for edge cases: `status-completed-no-video.http`,
`upload-expired.http`, `download-expired.http`, and `download-clip.http`
(not a real video: just the `ftyp` box marker an MP4 starts with).
