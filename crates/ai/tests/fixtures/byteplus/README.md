# BytePlus fixtures

Raw HTTP responses (status line, headers, body) that the take tests replay
instead of calling BytePlus: ModelArk's `POST /contents/generations/tasks`,
`GET /contents/generations/tasks/{id}` and `GET /contents/generations/tasks`
(the task list), the user's TOS bucket (presigned `PUT`, `GET` and
`DELETE`) and the take's download.

All are written from the documented shapes (docs.byteplus.com, checked on
2026-10-23: create, retrieve and list video generation tasks, Seedance 2.5
draft mode, ModelArk error codes, TOS presigned URLs), because recording
them needs a funded ModelArk account and a bucket. Re-record them when
takes misbehave with a real key:

- `submit-submitted.http`, `submit-final.http`, `status-queued.http`,
  `status-running.http`, `status-succeeded.http` (a 480p draft with its
  tokens and seed), `status-succeeded-final.http` (its 1080p final, with
  `draft_task_id`).
- `submit-declined.http` (400, `InputImageSensitiveContentDetected.PrivacyInformation`),
  `submit-invalid.http` (400, `InvalidParameter`), `submit-rejected.http`
  (401), `submit-overdue.http` (403, `AccountOverdueError`),
  `submit-not-open.http` (404, `ModelNotOpen`), `submit-limit.http` (429,
  `SetLimitExceeded`), `status-unknown.http` (404, `NotFound`).
- `status-failed-refused.http`, `status-failed-mismatch.http`
  (`InvalidParameter.TaskTypeMismatch`), `status-failed.http`,
  `status-expired.http`.
- `list-one.http`, `list-none.http`, `list-several.http`,
  `list-final.http`.
- `tos-stored.http`, `tos-read.http`, `tos-deleted.http`, `tos-no-such-key.http`,
  `tos-no-such-bucket.http`, `tos-signature.http`, `tos-denied.http`.
  TOS answers errors as JSON (`Code`, `Message`); the adapter reads XML
  too.

Synthetic, for edge cases: `status-succeeded-no-video.http`,
`download-expired.http`, and `download-take.http` (not a real video: just
the `ftyp` box marker an MP4 starts with).

The presigned links in `tests/byteplus.rs` were made with BytePlus's own
Python SDK (`tos` 2.9.3, `Auth.sign_url`) at a fixed clock, since the
sample in TOS's docs does not verify.
