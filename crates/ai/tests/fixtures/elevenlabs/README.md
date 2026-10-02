# ElevenLabs fixtures

Raw HTTP responses (status line, headers, body) that the voice list tests
replay instead of calling `GET /v2/voices`. Noise headers (cookies, dates,
trace ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `voices-rejected.http`.

Written from the documented response shape, because recording them needs a
real key (re-record when the voice picker misbehaves with a real key):

- `voices-page-1.http`: a default voice, a cloned voice and an entry with an
  unusable id; more pages follow.
- `voices-page-2.http`: the last page, repeating one voice from page 1.
- `voices-missing-permission.http`, `voices-rate-limited.http`.

Synthetic, for edge cases: `voices-not-json.http`.
