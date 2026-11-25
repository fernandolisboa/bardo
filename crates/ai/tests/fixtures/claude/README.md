# Claude fixtures

Raw HTTP responses (status line, headers, body) that the Claude text
generation tests replay instead of calling the Messages API. Noise headers
(cookies, dates, request ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `messages-rejected.http`.

Written from the documented response and stream shapes, because recording
them needs a real key and spends credits (re-record when generation
misbehaves with a real key). An answer is a stream of server-sent events
(`content-type: text/event-stream`):

- `messages-themes.http`: a structured-output answer (an omitted thinking
  block with a keep-alive inside it, then the JSON text in several deltas)
  with three themes.
- `messages-refusal.http`, `messages-max-tokens.http`: a whole stream that
  ends in that stop reason.
- `messages-overloaded-at-start.http`: an `error` event before any message
  starts, so nothing was generated.
- `messages-overloaded-midway.http`: an `error` event after part of the
  text arrived.
- `messages-dropped-midway.http`: a stream that ends inside an event,
  after part of the text arrived.
- `messages-themes-not-streamed.http`: the same answer as one JSON
  message, as a server that does not stream hands it over.

Error statuses answer with a JSON body, streamed request or not:

- `messages-overloaded.http`, `messages-rate-limited.http`,
  `messages-credit-balance.http`, `messages-beta-not-enabled.http`.
