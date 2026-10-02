# Market data fixtures

Raw HTTP responses (status line, headers, body) that the YouTube market
data tests replay instead of calling the API. Noise headers (cookies,
dates, request ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `search-key-rejected.http`.

Written from the documented response shapes of `search.list`,
`videos.list` and `channels.list` with the `fields` filters the adapter
sends, because recording them needs a real key and spends quota (re-record
when research misbehaves with a real key):

- `search-space-history.http`, `videos-space-history.http`,
  `channels-space-history.http`: five hits from four channels; one video
  is missing from `videos.list` (removed between calls) and one channel
  hides its subscriber count.
- `search-empty.http`, `search-quota.http`, `search-api-disabled.http`,
  `search-backend-error.http`.

Synthetic, for edge cases: `search-html.http` (a proxy page answering 200).
