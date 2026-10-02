# Gemini fixtures

Raw HTTP responses (status line, headers, body) that the image tests replay
instead of calling `POST /v1beta/models/{model}:generateContent`. Noise
headers (cookies, dates, trace ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `generate-rejected.http`.

Written from the documented response shape, because recording them needs a
real key (re-record when image generation misbehaves with a real key):

- `generate-image.http`: a thought (text and a draft image, both marked
  `thought`) and the final image, with usage including thinking tokens.
  The images are tiny 16×9 PNGs (`scene-16x9.png` is the final one), not
  drawings.
- `generate-blocked-prompt.http` (`promptFeedback.blockReason`),
  `generate-image-safety.http` (`finishReason: IMAGE_SAFETY`, no image),
  `generate-rate-limited.http`, `generate-overloaded.http`.

Synthetic, for edge cases: `generate-text-only.http` (text, no image),
`generate-not-an-image.http` (inline data that is not an image).
