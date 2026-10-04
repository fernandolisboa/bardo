---
id: themes-ranking
title: Themes and ranking
group: strategy
place: themes
tour: themes
---

# Themes and ranking

[Themes](bardo:go/themes) turns a niche into video ideas. Claude writes the ideas; the decision engine (TypeSafe) ranks them for your channel with its reasons; you decide which become videos. You need a Claude key to have ideas written and a TypeSafe key to have them ranked ([Your API keys](api-keys.md#providers)). [Show me the screen](bardo:tour/themes).

<a id="pick"></a>
## Channel and niche

Ideas are written for one channel and one niche. The niches to choose from are the ones you last researched for the channel on [Research](niche-research.md); before any research, it is the channel's own niche. When the niche has research results, its opportunity, competition and trend show under the pickers, and a line says how the channel's published videos did, once they have numbers.

<a id="suggest"></a>
## Suggest themes and rank again

**Suggest themes** asks Claude for 10 video ideas, each a title and an angle, written for the channel's niche, audience, style and language. The decision engine then ranks every idea of the niche that has no ranking yet. Both run as a job; the estimated cost shows before you start, and a budget you set on [Costs](bardo:go/costs) asks first when the run would pass it.

**Rank again** shows when ideas are waiting: ones you edited, ones whose ranking failed, or ones ranked before your videos had numbers. It ranks only those, without asking for new ideas.

<a id="ranking"></a>
## Priority and confidence

Ideas are listed by **priority**, from 0 to 100, highest first. **Confidence** is how sure the decision engine was of its least certain answer: a high priority with low confidence is a good bet to check by hand.

<a id="reasons"></a>
## How an idea is ranked

The engine scores four reasons, each from 0 to 100 with its own confidence:

- **Fit**: how well the idea suits the channel, its niche and its audience.
- **Trend**: how much demand there is for the idea now.
- **Competition**: how crowded the angle is; less is better.
- **Past performance**: how ideas like this one did on your channel, judged from your videos' first week of views.

Priority weighs fit 40%, trend 35% and the room competition leaves 25%. Past performance joins once a published video is two days old and its numbers are synced: it takes 5% per such video, up to 25% from five videos, and the other three share the rest in the same proportions. **Details** shows the numbers the engine read and which model ranked the idea, when.

<a id="review"></a>
## Approve, edit or discard

- **Approve** starts a video project from the idea, under the channel. The idea stays in the list, marked approved.
- **Edit** changes the title or the angle. An edited idea loses its ranking, since the reasons were about the old text; use **Rank again**.
- **Discard** takes the idea out of the list. The list counts how many were discarded.

Nothing is approved for you: an idea becomes a video only when you approve it.

<a id="projects"></a>
## Projects started here

The **Projects** list under the controls shows the projects started from the channel's ideas. Open them on [Projects](bardo:go/projects) to write the script, then narration, scenes and the rest ([Your first video](first-video.md#project)).

<a id="next"></a>
## Next step

After you publish, link each post at the project's Publish stage. Its numbers then show on [Performance](performance-metrics.md) and feed the past performance of the next rankings.
