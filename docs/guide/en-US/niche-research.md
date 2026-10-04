---
id: niche-research
title: Niche research
group: strategy
place: research
tour: research
---

# Niche research

[Research](bardo:go/research) tells you how crowded a niche is on YouTube and how fast its recent videos gather views, so you pick niches with room left. It reads public YouTube data with your YouTube Data API key ([Your API keys](api-keys.md#providers)). [Show me the screen](bardo:tour/research).

<a id="market"></a>
## Channel and market

Research runs for one channel at a time. The channel's **target country** and **content language** set the market: every search asks YouTube for uploads in that country and language, so the same niche can score differently for a channel in English for the US and one in Portuguese for Brazil. Change them on [Channels](bardo:go/channels); the results of each market are kept apart.

<a id="seeds"></a>
## Niches or keywords

Type one niche or keyword per line, up to 20 per run and 100 characters each. Each line becomes a YouTube search, so write it the way viewers search ("cold war mysteries", not "history content"). Lines that differ only in capital letters or spaces count once. The list from your last run stays with the channel for next time.

<a id="run"></a>
## Run, refresh and quota

**Run research** starts a job that looks up the niches. Each search costs about 1% of the YouTube Data API's default daily quota (10,000 units), and the ⓘ next to the buttons says what this run will cost before you start it.

Each result is kept for **7 days** per niche, country and language. Running again within a week reuses it and costs no quota; a result older than that says so on its card and is fetched again on the next run. **Refresh all** fetches every niche again now, whatever its age, and says its cost on the button.

The job runs in the background: keep working, and follow or cancel it in [Jobs](bardo:go/jobs).

<a id="results"></a>
## Reading the results

Niches are listed by **opportunity**, highest first. Each card shows three scores from 0 to 100 and the numbers behind them, all from uploads of the last **30 days** in the market:

- **Uploads, 30 days**: how many videos YouTube estimates were posted for the search.
- **Median views** and **views per day**: how much the most relevant recent uploads (up to 50) were watched, and how fast.
- **Median subscribers** and **small channels**: how big the channels behind those uploads are, and how many have fewer than 10,000 subscribers.
- **Fetched**: when the numbers were read. A result older than 7 days says so; refresh it for current numbers.

A niche with no uploads in the window has no scores, since there is nothing to judge it by.

<a id="scores"></a>
## How the scores are computed

Each number goes through a scale that grows with orders of magnitude (going from 1,000 to 10,000 uploads counts as much as from 10,000 to 100,000), so huge niches do not flatten the rest.

- **Competition** (higher is harder): 40% from how many uploads there are, 40% from how big the uploading channels are and how few small channels get seen, and 20% from how few views a video gets.
- **Trend** (higher is hotter): how many views per day recent uploads gather.
- **Opportunity**: the average of the trend and the room competition leaves (100 minus competition).

The scores compare niches in the same market. They do not predict views; your own videos' numbers do that over time, on [Performance](performance-metrics.md).

<a id="next"></a>
## Next step

Take the niches with the best opportunity to [Themes](bardo:go/themes): Claude suggests video ideas for one of them and the decision engine ranks them for your channel. See [Themes and ranking](themes-ranking.md).
