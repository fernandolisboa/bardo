# Kling fixtures

Raw HTTP responses (status line, headers, body) that the clip tests replay
instead of calling the Kling API: `POST /image-to-video/{model}`,
`GET /tasks?task_ids=…`, `GET /tasks?external_task_ids=…` and the clip
download. Noise headers were dropped; CDN hosts are placeholders.

Recorded from the live API with an invalid key on 2026-10-20:
`submit-rejected.http`.

Written from the documented response shapes (kling.ai/document-api: image
to video for Kling 3.0, 2.6 and 2.5 Turbo, task queries, error codes,
concurrency rules), because recording them needs a key with a resource
package. Re-record when clips misbehave with a real key:

- `submit-submitted.http`, `lookup-found.http`, `lookup-empty.http`.
- `status-submitted.http`, `status-processing.http`,
  `status-succeeded.http`, `status-failed.http`,
  `status-failed-moderation.http`.
- `submit-no-units.http` (429, code 1102), `submit-concurrency.http` (429,
  code 1303, message as documented), `submit-declined.http` (400, code
  1301), `submit-invalid.http` (400, code 1201),
  `submit-model-not-allowed.http` (403, code 1103),
  `submit-overloaded.http` (503, code 5001).
- `submit-duplicate.http`: Kling documents that custom task ids are unique
  per account but not the answer to a repeated one; this guesses code 1201.

Synthetic, for edge cases: `status-succeeded-no-video.http`,
`status-unknown-task.http`, `download-expired.http`, and
`download-clip.http` (not a real video: just the `ftyp` box marker an MP4
starts with).

Motion Control takes (#243) reuse the submission, lookup, status and
download fixtures above: `POST /motion-control/{model}` answers in the same
shape as image to video (kling.ai/document-api: Motion Control 3.0 and 2.6,
checked on 2026-10-25). `take-status-succeeded.http` is written from the
documented task query, with the video's `duration` as a number and the
`billing` block of a balance-paid task; `status-succeeded.http` keeps the
duration as text, which the adapter reads too.
