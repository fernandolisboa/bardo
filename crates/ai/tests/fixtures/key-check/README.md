# Key check fixtures

Raw HTTP responses (status line, headers, body) that the key check tests
replay instead of calling providers. Noise headers (cookies, request ids,
dates) were dropped.

Recorded on 2026-10-01 from the live APIs with an invalid key:

- `*-rejected.http`, one per provider (Kling's on 2026-10-20).

Written from each provider's documented response shape, because recording
them needs a real key (re-record when a check misbehaves with a real key):

- `*-valid.http`, `claude-overloaded.http`,
  `elevenlabs-missing-permission.http`, `gemini-service-disabled.http`,
  `higgsfield-no-credits.http`, `typesafe-rate-limited.http`,
  `youtube-data-quota.http`, `kling-no-such-task.http`,
  `kling-no-units.http`.

Synthetic, for edge cases: `proxy-error-page.http`, `unknown-status.http`.
