# GitHub releases fixtures

Raw HTTP responses (status line, headers, body) that the release feed
tests replay instead of calling GitHub. Noise headers (CORS, CSP, request
ids, dates) were dropped.

Recorded on 2026-10-14 from the live API, with no release published yet:

- `none.http`: `GET /repos/fernandolisboa/bardo/releases/latest` answers
  404 while only drafts and pre-releases (the `ffmpeg-*` mirrors) exist.

Written from the recorded `ffmpeg-8.1.3` release of this repository (same
fields, same order, the asset `digest` as GitHub gives it), renamed to the
first update release as `.github/workflows/release.yml` would publish it,
because no such release exists yet (re-record once one does):

- `latest.http`: `v0.2.0` with the installer and its `.sha256`, and notes
  as GitHub generates them.
- `latest-no-installer.http`: the same release without files.
- `latest-not-a-version.http`: the same release tagged `nightly`.

Written from GitHub's documentation on rate limits:

- `rate-limited.http`: the hourly limit for calls without a token is spent
  (403, `x-ratelimit-remaining: 0`, the reset time in Unix seconds).
- `secondary-rate-limit.http`: a secondary limit (429 with `retry-after`).

Synthetic, for edge cases: `forbidden.http` (a 403 that is not a rate
limit), `unavailable.http` (a 503 from GitHub's front end).
