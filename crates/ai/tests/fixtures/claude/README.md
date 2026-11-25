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
  after part of the text arrived. This one and the one above are lost
  answers (ADR-0030, ADR-0031): never sent again, and without a usage,
  because a stream counts its output only at its end.
- `messages-themes-not-streamed.http`: the same answer as one JSON
  message, as a server that does not stream hands it over.

Error statuses answer with a JSON body, streamed request or not:

- `messages-overloaded.http`, `messages-rate-limited.http`,
  `messages-credit-balance.http`, `messages-beta-not-enabled.http`.

Lost and failed answers (ADR-0030):

- `messages-api-error.http`: the API's own 500 (`api_error`), written from
  the documented error shape.
- `messages-gateway-timeout.http`: synthetic, a gateway's 504 page in
  place of the API's answer.
- `messages-no-text.http`: synthetic, a finished answer with an empty
  thinking block and no text, with its usage.

Research on the web (`ClaudeWebResearcher`, ADR-0037), written from the
documented shapes of the web search and web fetch tools as of November
2026: a `server_tool_use` block whose input streams as `input_json_delta`,
a result block that arrives whole in its `content_block_start`, citations
as `citations_delta`, and a usage with `iterations`, the tokens through
the prompt cache and `server_tool_use` counts. No real key was used;
re-record with one when a research misbehaves.

- `research-whole.http`: a fetch of the product page, a search with three
  results, then the notes in two text blocks, the second with a citation.
- `research-paused.http`, `research-resumed.http`: a turn the API pauses
  (`pause_turn`) with a fetch called and not yet run, and the answer to
  its continuation, which opens with that fetch's result.
- `research-paused-again.http`: a continuation that pauses again.
- `research-tool-error.http`: a fetch that fails and a search refused for
  passing `max_uses`, both as error objects inside a successful answer.
- `research-refusal.http`: a refusal after the first search, with its
  usage.
- `research-max-tokens.http`: notes cut at the token limit.
