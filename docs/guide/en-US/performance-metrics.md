---
id: performance-metrics
title: Performance and metrics
group: strategy
place: performance
tour: performance
---

# Performance and metrics

[Performance](bardo:go/performance) follows how a channel's published posts do: their views, likes, comments and, where the network gives them, watch time, retention and revenue. Those numbers also teach the [theme ranking](themes-ranking.md#reasons) what works on your channel. [Show me the screen](bardo:tour/performance).

<a id="channel"></a>
## One channel at a time

Pick the channel in the header. Its figures add up the latest numbers of all its linked posts: views, likes and comments (shares too where Instagram or TikTok report them), and the number of posts. When the channel's YouTube account is connected on [Accounts](bardo:go/accounts), **engaged views**, **watch time** and **estimated revenue** lead instead. The chart of the channel's views over the syncs starts after two of them.

<a id="posts"></a>
## Linked posts

The list holds every post linked to one of the channel's projects, newest first, with its network and views. A post marked **Not found** was missing at the last sync (removed or made private); its numbers stay as they were. A post Bardo uploaded also shows how its upload went (still processing, sent to TikTok as a draft, stopped), and its ⓘ says what to do next.

<a id="numbers"></a>
## A post's numbers

Pick a post to open it beside the list: its link, its numbers, how each moved since the sync before, and its history over the syncs. Where they come from depends on the network:

- **YouTube**: public views, likes and comments, read with your YouTube Data API key. With the channel's YouTube account connected, YouTube Analytics adds engaged views (views past the first seconds), watch time, average view, audience retention and estimated revenue with RPM and CPM. Those arrive 2 to 3 days late.
- **Instagram** and **TikTok**: the post's numbers, once the channel's account on that network is connected; there is no public count to read without it.
- **X** and **Kick**: Bardo keeps the link and reads no numbers.

<a id="sync"></a>
## Syncing

**Sync now** reads the latest numbers of every linked post as a job, and the line beside it says when the last sync ran. YouTube's public numbers cost one quota unit per 50 posts. Bardo also syncs by itself when it opens, if the last check is older than the time set in [Settings › Metrics](bardo:go/settings/metrics) (6 hours unless you change it, or never).

<a id="link"></a>
## Linking and unlinking a post

Posts are linked at a project's [Publish](bardo:go/projects/publish) stage. A video Bardo uploads is linked by itself. For an exported file, post it by hand, then paste the post's link with **Mark as posted**; Bardo checks that the link is a post on that network. **Unlink** removes the post and its numbers from Bardo; the post stays on the network.

<a id="ranking"></a>
## How your numbers rank ideas

Each post's views seven days after it went live, its **first week**, are what the theme ranking compares. Bardo reads them from the syncs around day seven, or projects them from the first days. Once a published video is two days old and synced, past performance joins the ranking of new ideas on [Themes](themes-ranking.md#reasons), and ideas ranked before can be ranked again to include it.
