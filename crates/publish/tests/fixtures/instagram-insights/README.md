# Instagram insights fixtures

Raw HTTP responses (status line, headers, body) that the Instagram post
insights tests replay instead of calling Meta. Noise headers (dates, trace
and request ids, proxy headers) were dropped. No fixture holds a real token,
account or post.

Recorded on 2026-10-04 from the live Graph API with an access token Meta
does not know:

- `invalid-token.http`: `GET /<media>/insights` and
  `GET /<ig-user>/media` answer the same 401 with code 190.

Written from the documented shapes of
[media insights](https://developers.facebook.com/docs/instagram-platform/reference/instagram-media/insights),
the [media listing](https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/ig-user/media)
and the [error codes](https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/error-codes),
because recording them needs a real professional account (re-record during
the publishing regression pass if an answer misbehaves):

- `reel-insights.http`: every metric Bardo asks of a Reel, with a reported
  zero (`saved`). The watch times are in milliseconds.
- `reel-insights-partial.http`: a young Reel whose other metrics have not
  arrived yet: Meta leaves them out instead of answering zero.
- `insights-empty.http`: no data for the media yet.
- `feed-metric-refused.http`: the 400 (code 100) for a Reel-only metric
  asked of a feed post; `feed-insights.http` is the answer without them.
- `media-gone.http`: a media Meta does not have (code 100, subcode 33).
- `not-enough-viewers.http`: insights refused for one media (code 10).
- `rate-limited.http`: the app's request limit (code 4).
- `media-page-1.http`, `media-page-2.http`: the account's media, newest
  first, over two pages (`paging.next` with the `after` cursor, then none).
