# ElevenLabs fixtures

Raw HTTP responses (status line, headers, body) that the voice list and
narration tests replay instead of calling `GET /v2/voices` and
`POST /v1/text-to-speech/{voice_id}/with-timestamps`. Noise headers
(cookies, dates, trace ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `voices-rejected.http`, `tts-rejected.http`.

Written from the documented response shape, because recording them needs a
real key (re-record when the voice picker or narration misbehaves with a
real key):

- `voices-page-1.http`: a default voice, a cloned voice and an entry with an
  unusable id; more pages follow.
- `voices-page-2.http`: the last page, repeating one voice from page 1.
- `voices-missing-permission.http`, `voices-rate-limited.http`.
- `tts-hello.http`: "Hello, world. It is 1969." with character timings of
  the text as sent and of the normalized text, and a `character-cost`
  header. The audio is a one-second tone (the media crate's
  `tone-1s-raw.mp3`), not speech.
- `tts-quota-exceeded.http`, `tts-rate-limited.http`,
  `tts-voice-not-found.http`.

Synthetic, for edge cases: `voices-not-json.http`,
`tts-normalized-only.http` (no timings of the text as sent, no cost
header), `tts-no-audio.http`, `tts-misaligned.http` (timing lists of
different lengths).
