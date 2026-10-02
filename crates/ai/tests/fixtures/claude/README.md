# Claude fixtures

Raw HTTP responses (status line, headers, body) that the Claude text
generation tests replay instead of calling the Messages API. Noise headers
(cookies, dates, request ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `messages-rejected.http`.

Written from the documented response shapes, because recording them needs
a real key and spends credits (re-record when generation misbehaves with a
real key):

- `messages-themes.http`: a structured-output answer (an omitted thinking
  block, then the JSON text) with three themes.
- `messages-refusal.http`, `messages-max-tokens.http`,
  `messages-overloaded.http`, `messages-rate-limited.http`,
  `messages-credit-balance.http`, `messages-beta-not-enabled.http`.
