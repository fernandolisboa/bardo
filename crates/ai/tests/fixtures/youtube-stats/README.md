# YouTube statistics fixtures

Raw HTTP responses (status line, headers, body) that the YouTube
statistics tests replay instead of calling the API. Noise headers
(cookies, dates, request ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `videos-key-rejected.http`.

Written from the documented response shape of `videos.list` with the
`fields` filter the adapter sends
(`items(id,snippet/publishedAt,statistics(viewCount,likeCount,commentCount))`),
because recording them needs a real key (re-record when a sync misbehaves
with a real key):

- `videos-two-of-three.http`: three ids asked, two returned (the third was
  removed or made private); the second hides its likes and has comments
  turned off, so `statistics` holds only `viewCount`.
- `videos-none.http`: no id found; with a `fields` filter the API answers
  an empty object.
- `videos-quota.http`: the daily quota is spent (same body as search's).

Synthetic, for edge cases: `videos-stranger.http` (an id that was not
asked for), `videos-no-views.http` (statistics without a view count).
