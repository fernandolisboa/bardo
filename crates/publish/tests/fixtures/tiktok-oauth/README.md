# TikTok OAuth and user info fixtures

Raw HTTP responses (status line, headers, body) that the TikTok sign-in
tests replay instead of calling TikTok. Noise headers (trace ids, timing,
dates) were dropped. No fixture holds a real token or client key.

Recorded on 2026-10-04 from the live API with made-up client keys, codes
and tokens (bodies verbatim). They show that TikTok's OAuth endpoints
answer errors with **HTTP 200** and an `error` field, so the adapter reads
the body, not the status:

- `token-code-expired.http`: `POST /v2/oauth/token/` exchanging a code
  TikTok does not know.
- `token-invalid-client.http`: a refresh with a client key and secret
  TikTok does not know.
- `revoke-invalid-token.http`: `POST /v2/oauth/revoke/` with an access
  token TikTok does not know.
- `user-info-invalid-token.http`: `GET /v2/user/info/` with an access token
  TikTok does not know (HTTP 401, the API's own error shape).

Written from the documented response shapes
([token management](https://developers.tiktok.com/doc/oauth-user-access-token-management),
[user info](https://developers.tiktok.com/doc/tiktok-api-v2-get-user-info),
[API errors](https://developers.tiktok.com/doc/tiktok-api-v2-error-handling)),
because recording them needs a real TikTok app and account (re-record
during the publishing regression pass if a sign-in misbehaves):

- `token-exchange.http`: the code exchange, with all three scopes granted
  (comma-separated, as TikTok writes them).
- `token-exchange-missing-scope.http`: the user unticked `video.upload`.
- `token-refresh-rotated.http`: a refresh that returns a new refresh token.
- `token-refresh-invalid-grant.http`: a refresh token TikTok revoked or that
  ran out after 365 days, in the shape of the recorded `invalid_grant`.
- `revoke-ok.http`: a revocation TikTok accepted (empty body).
- `user-info.http`: `fields=open_id,display_name`.
- `user-info-scope-not-authorized.http`, `user-info-rate-limited.http`: the
  API's documented error codes.

Synthetic, for edge cases: `token-exchange-no-refresh.http` (an exchange
without a refresh token), `token-unavailable.http` (TikTok's
`temporarily_unavailable`), `server-error.http` (an HTML 503 from TikTok's
edge).
