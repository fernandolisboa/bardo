# Higgsfield plan fixtures

Raw HTTP responses (status line, headers, body) that the tests replay
instead of calling Higgsfield's MCP server (`https://mcp.higgsfield.ai/mcp`)
and its OAuth endpoints (`https://mcp.higgsfield.ai/oauth2/…`). Noise
headers were dropped; CDN hosts and upload signatures are placeholders.
Tool results are wrapped as the server sends them: a JSON-RPC response,
in a server-sent event, whose `result.content[0].text` holds the tool's
JSON.

Recorded on 2026-11-19 and 2026-11-20 from the live server (tool payloads
through an MCP client signed in to a free account, nothing generated):
`unauthorized.http` (an unauthenticated `initialize`), `register.http`
(dynamic client registration with a loopback redirect; the client id replaced), and the payloads
of `balance.http`, `estimate.http` (Kling 3.0 Standard, 10 s, sound off),
`media-upload.http`, `generations-empty.http`, `submit-unknown-model.http`
and `wait-lookup-failed.http`.

Written from the tools' descriptions, because recording them needs a
generation paid with plan credits. Re-record when clips misbehave with a
real plan:

- `initialize.http`, `initialize-json.http`, `initialized.http`,
  `session-gone.http`: the Streamable HTTP transport as the MCP
  specification (2025-06-18) describes it.
- `submitted.http`: `generate_video_batch` "keeps its caller-provided
  index in the response" and returns job ids for `jobs_wait`.
- `wait-queued.http`, `wait-running.http`, `wait-completed.http`,
  `wait-failed.http`, `wait-nsfw.http`: `jobs_wait` "returns compact
  indexed statuses and result URLs"; "a failed, nsfw or ip_detected job
  may carry error".
- `generations-with-job.http`: `show_generations` "returns generations
  with {id, type, status, model, params, results}".
- `media-confirm.http`, `token.http`, `token-refreshed.http`,
  `token-invalid-grant.http` (RFC 6749 shapes).

Synthetic, for edge cases: `balance-plus.http`,
`generations-page-1.http` (a page with a `next_cursor`),
`generations-same-prompt.http` (an older clip of the same scene),
`submitted-no-id.http`, `submit-no-credits.http`, `answer-cut-off.http`
(an empty answer), `rate-limited.http`, `wait-unknown-over.http` and
`wait-unknown-going.http` (statuses the adapter does not know),
`put-ok.http`, `put-refused.http`, and `download-clip.http` (not a real
video: just the `ftyp` box marker an MP4 starts with).
