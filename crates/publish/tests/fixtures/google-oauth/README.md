# Google OAuth and channel fixtures

Raw HTTP responses (status line, headers, body) that the YouTube sign-in
tests replay instead of calling Google. Noise headers (cookies, dates,
request ids) were dropped. No fixture holds a real token.

Recorded on 2026-10-03 from the live API with an invalid access token:

- `channels-invalid-token.http`: `channels.list mine=true` with a bearer
  token Google does not know.

Written from the documented response shapes, because recording them needs
a real Google account and OAuth client (re-record during the publishing
regression pass if a sign-in misbehaves):

- `token-exchange.http`: the code exchange of
  [OAuth 2.0 for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app),
  with all four scopes granted.
- `token-refresh.http`: a refresh; Google keeps the refresh token, so the
  answer has none.
- `token-invalid-grant.http`: a refresh token Google revoked or expired
  (a client in "Testing" status after seven days answers the same).
- `token-invalid-client.http`: a client id or secret Google does not know.
- `revoke-ok.http`, `revoke-invalid-token.http`: `oauth2.googleapis.com/revoke`
  for a live token and for one already revoked.
- `channels-mine.http`: `channels.list` with `part=snippet&mine=true` and
  the `fields` filter `items(id,snippet/title)`.
- `channels-none.http`: the signed-in Google account has no channel; with a
  `fields` filter the API answers an empty object.
- `channels-api-disabled.http`: YouTube Data API v3 is not enabled on the
  user's Google Cloud project.
- `channels-quota.http`: the project's daily quota is spent.

Synthetic, for edge cases: `token-exchange-no-refresh.http` (an exchange
without a refresh token), `token-server-error.http` (Google failing on its
side).
