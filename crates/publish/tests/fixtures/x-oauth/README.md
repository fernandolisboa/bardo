# X sign-in fixtures

Raw HTTP responses (status line, headers, body) that the X sign-in tests
replay instead of calling X. Noise headers (trace ids, rate-limit counters,
dates, CDN headers) were dropped. No fixture holds a real token, client id
or user id.

Recorded on 2026-10-14 from `api.x.com` with a made-up client id, secret
and token (bodies verbatim):

- `token-invalid-client.http`: `POST /2/oauth2/token` (refresh grant)
  with a client id X does not know (HTTP 400 `invalid_client`).
- `revoke-missing-hint.http`: `POST /2/oauth2/revoke` without
  `token_type_hint`, which X requires (HTTP 400 `invalid_request`).
- `revoke-ok.http`: `POST /2/oauth2/revoke` of a token X never handed out,
  with its hint: X answers `{"revoked":true}` all the same.
- `users-me-not-a-user-token.http`: `GET /2/users/me` with a made-up
  bearer token, which X takes for an app-only one (HTTP 403
  `unsupported-authentication`).

Written from the documented shapes of the
[OAuth 2.0 PKCE flow](https://docs.x.com/fundamentals/authentication/oauth-2-0/authorization-code),
[user lookup](https://docs.x.com/x-api/users/get-my-user) and
[error](https://docs.x.com/x-api/fundamentals/response-codes-and-errors)
references, because recording them needs a real X app, account and
credits (re-record during the publishing regression pass if a sign-in
misbehaves):

- `token-exchange.http`, `token-refresh-rotated.http`: a code exchange
  and a refresh, each with a new refresh token (X rotates it).
- `token-exchange-no-refresh.http`, `token-exchange-missing-scope.http`:
  a grant without `offline.access`, and one without `media.write`.
- `token-code-invalid.http`, `token-refresh-invalid-token.http`: a code or
  refresh token X no longer takes. X answers these as `invalid_request`
  with "Value passed for the … was invalid" (as the recorded
  `invalid_client` answer words it); the exact wording is assumed.
- `users-me.http`, `users-me-unauthorized.http`: the account, and an
  expired access token (401).
- `users-me-credits-depleted.http`: an app out of credits (402). The
  status and problem type come from the pricing and error pages; the
  title and detail are assumed.
- `server-error.http`: a 503 from X.
