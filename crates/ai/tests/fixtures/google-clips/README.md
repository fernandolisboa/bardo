# Google clip fixtures

Raw HTTP responses (status line, headers, body) that the clip tests replay
instead of calling the Gemini API: Gemini Omni Flash's
`POST /v1beta/interactions` and `GET /v1beta/interactions/{id}`, and the clip
download from the Files API. Noise headers were dropped; ids are
placeholders.

Recorded on 2026-10-02 from the live API with an invalid key, and the same
on 2026-11-25:

- `omni-submit-rejected.http`: the API wraps this error in a one-element
  array, with a numeric code and a status.

Written from the documented response shapes (aistudio.google.com/docs: Gemini
Omni Flash, the Interactions API reference, background execution, API
errors; read on 2026-11-25 for `gemini-omni-1.2-flash`), because recording
them needs a funded key. Google documents background runs for its text
models and agents, not for Omni; the shapes assume Omni answers the same
way, as 1.1 did. Re-record when Google clips misbehave with a real
key:

- `omni-submit.http`, `omni-in-progress.http`, `omni-completed.http` (the
  clip inline in a `model_output` step, and the usage of a 7-second 720p
  clip: 5,792 video tokens a second), `omni-completed-uri.http` (the clip
  as a Files API address), `omni-completed-text.http` (a refusal as text, no
  clip), `omni-failed.http`, `omni-failed-blocked.http` (a documented
  "generation blocked" code), `omni-cancelled.http`.
- Refusals, as the API errors page gives them (`error.code` in snake case
  and `error.message`; the messages are examples):
  `omni-submit-rate-limited.http` (429), `omni-submit-overloaded.http` (503),
  `omni-submit-model-not-found.http` (404), `omni-submit-invalid.http` (400)
  and `omni-submit-no-credits.http` (402, a prepaid balance at zero).
- `not-found.http` (an interaction Google no longer keeps),
  `download-expired.http`.

Kept from an earlier shape of the API: `omni-queued.http`. The reference no
longer lists `queued` among the statuses; the adapter still reads it.

Synthetic, for edge cases: `download-clip.http` and the inline clip in
`omni-completed.http` (not a real video: just the `ftyp` box marker an MP4
starts with).
