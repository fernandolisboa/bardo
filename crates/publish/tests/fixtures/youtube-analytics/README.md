# YouTube Analytics fixtures

Raw HTTP responses (status line, headers, body) that the owner metrics
tests replay instead of calling Google. Noise headers (dates, request ids)
were dropped. No fixture holds a real token, channel or video.

Recorded on 2026-10-03 from the live API with an invalid access token:

- `invalid-token.http`: `reports.query` with a bearer token Google does not
  know.

Written from the documented shapes of
[reports.query](https://developers.google.com/youtube/analytics/reference/reports/query),
the [channel reports](https://developers.google.com/youtube/analytics/channel_reports)
and the [metrics](https://developers.google.com/youtube/analytics/metrics),
because recording them needs a real channel and OAuth client (re-record
during the publishing regression pass if a report misbehaves):

- `video-monetized.http`: one video's basic report with the money metrics,
  for a channel in the Partner Program.
- `video-numbers.http`: the same report without the money metrics.
- `money-forbidden.http`: the 403 a channel outside the Partner Program
  gets for money metrics (revision history, 2026-09-09).
- `video-empty.http`: a report with no row: no data for the video yet.
- `retention.http`: the audience retention report, a hundred points of
  `elapsedVideoTimeRatio` with `audienceWatchRatio` (above one where the
  start is rewatched) and `relativeRetentionPerformance`.
- `quota-exceeded.http`: the project's quota, a 403 with `quotaExceeded`.
