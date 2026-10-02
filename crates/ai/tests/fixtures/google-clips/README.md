# Google clip fixtures

Raw HTTP responses (status line, headers, body) that the clip tests replay
instead of calling the Gemini API: Veo's
`POST /v1beta/models/{model}:predictLongRunning` and `GET /v1beta/{operation}`,
Gemini Omni Flash's `POST /v1beta/interactions` and
`GET /v1beta/interactions/{id}`, and the clip download from the Files API.
Noise headers were dropped; ids are placeholders.

Recorded on 2026-10-02 from the live API with an invalid key:

- `veo-submit-rejected.http`.
- `omni-submit-rejected.http`: the Interactions API wraps its error in a
  one-element array.

Written from the documented response shapes (ai.google.dev: Veo 3.1, Gemini
Omni Flash, background execution, API errors), because recording them needs
a funded key. Re-record when Google clips misbehave with a real key:

- Veo: `veo-submit.http`, `veo-running.http`, `veo-done.http`,
  `veo-filtered.http` (`raiMediaFilteredCount` with a reason, no video),
  `veo-error.http` (a finished operation with an error),
  `veo-submit-rate-limited.http` (429), `veo-submit-overloaded.http` (503),
  `veo-submit-model-not-found.http` (404), `veo-submit-invalid.http` (400).
- Omni: `omni-submit.http`, `omni-queued.http`, `omni-in-progress.http`,
  `omni-completed.http` (the clip inline in a `model_output` step),
  `omni-completed-text.http` (a refusal as text, no clip),
  `omni-failed.http`, `omni-cancelled.http`.
- `not-found.http` (an operation or interaction Google no longer keeps),
  `download-expired.http`.

Synthetic, for edge cases: `veo-done-empty.http` (done, neither a video nor
a filter reason), and `download-clip.http` and the inline clip in
`omni-completed.http` (not a real video: just the `ftyp` box marker an MP4
starts with).
