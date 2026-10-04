# Meta Graph API fixtures

Raw HTTP responses (status line, headers, body) that the Instagram sign-in
tests replay instead of calling Meta. Noise headers (trace ids, dates,
proxy and debug headers) were dropped. No fixture holds a real token or app
secret.

Recorded on 2026-10-04 from the live Graph API (v25.0) with made-up
credentials:

- `token-invalid-client.http`: `oauth/access_token` with
  `grant_type=fb_exchange_token` and an app id Meta does not know.
- `token-bad-secret.http`: the same with a real public app id and a wrong
  secret. Meta answers code 1, which otherwise means a passing failure.
- `accounts-invalid-token.http`: `me/accounts` with a token Meta cannot read.
- `revoke-invalid-token.http`: `DELETE me/permissions` with a token Meta
  cannot read.

Written from the documented response shapes, because recording them needs
a real Meta app, a Facebook account and an Instagram professional account
linked to a Page (re-record during the publishing regression pass if a
sign-in misbehaves):

- `token-exchange.http`, `token-exchange-no-lifetime.http`: the long-lived
  user token of
  [Get Long-Lived Tokens](https://developers.facebook.com/docs/facebook-login/guides/access-tokens/get-long-lived),
  with and without `expires_in`.
- `token-expired.http`: an expired token (code 190, subcode 463) from
  [Graph API error handling](https://developers.facebook.com/docs/graph-api/guides/error-handling).
- `permissions-all.http`, `permissions-declined.http`: `me/permissions`.
- `accounts-*.http`: `me/accounts` with
  `fields=id,name,access_token,instagram_business_account{id,username}`, as
  in [Facebook Login for Business for Instagram](https://developers.facebook.com/docs/instagram-platform/instagram-api-with-facebook-login/business-login-for-instagram):
  several Pages, two result pages, none, a Page without Instagram, and the
  Page token read again after a renewal.
- `page-instagram.http`, `page-no-instagram.http`: `me` with a Page token
  and `fields=instagram_business_account{id,username}`.
- `revoke-ok.http`: `DELETE me/permissions`.
- `rate-limited.http`, `permission-missing.http`, `server-error.http`: codes
  4, 10 and 2 of the error handling guide.
