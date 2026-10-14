---
id: metrics-sync
title: Syncing metrics
group: strategy
place: settings/metrics
tour: metrics-sync
---

# Syncing metrics

Bardo reads your posts' numbers from each network as a job: a **sync**. [Settings › Metrics](bardo:go/settings/metrics) says when it does that by itself. [Show me the tab](bardo:tour/metrics-sync).

<a id="what"></a>
## What a sync reads

- **YouTube**: the public views, likes and comments of every linked post, at one unit of the daily quota per 50 posts: through the channel's connected account when it has one, otherwise with your YouTube Data API key. For a channel whose YouTube account is connected, also the owner's numbers from YouTube Analytics, at two requests per post.
- **Instagram** and **TikTok**: each post's numbers through the channel's connected account on that network, with no key.
- **X** and **Kick**: nothing; Bardo keeps the link only.

While an account needs to reconnect, its posts' numbers through that account are skipped; YouTube's public numbers still sync with the Data API key; see [Troubleshooting](troubleshooting.md#reconnect).

<a id="on-start"></a>
## Syncing when Bardo opens

**Sync on start** decides whether Bardo syncs when it opens: never, or when the oldest check is older than 1 hour, 6 hours (unless you change it), 12 hours or a day. Opening Bardo several times a day then costs no extra quota. **Sync now**, on Performance, always works, whatever this says.

YouTube's numbers are the exception: when a YouTube post was last read 25 days ago or more, Bardo syncs when it opens even with **Never**, because numbers read from YouTube may be kept only 30 days and would otherwise be deleted ([Privacy policy](privacy.md#kept)).

<a id="where"></a>
## Where the numbers show

The numbers show on [Performance](performance-metrics.md), per channel and per post, with the time of the last sync. Each post's first week of views also feeds the [ranking of new ideas](performance-metrics.md#ranking).
