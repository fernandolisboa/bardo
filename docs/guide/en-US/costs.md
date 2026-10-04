---
id: costs
title: Costs and budgets
group: costs
place: costs
tour: costs
---

# Costs and budgets

Every paid call a generation makes is recorded with what it cost: what the provider said it charged, or the price in Bardo's rate table. [Costs](bardo:go/costs) shows a month of it, by provider, by channel and by video, and keeps a monthly budget per provider. [Show me the screen](bardo:tour/costs).

<a id="month"></a>
## The month

Costs opens on this month. The arrows beside its name go back a month at a time, and forward again up to this one. Months follow UTC, the way providers bill, so a call made late on the last evening of a month in Brazil may count in the next one.

<a id="spent"></a>
## What the month spent

The figures at the top add it up: what was spent, across how many providers; how many budgets are near or past their limit; and how many models ran without a price. In Workspace they are cards; in Studio, a line in the header.

<a id="budgets"></a>
## Budgets

**Budgets** lists each paid provider: what was used, what was spent, and its budget. **Set budget** gives a provider a monthly limit in US dollars; **Change** and **Remove** edit it.

- **From 80%**: before a generation that would take a provider to 80% of its budget or more, the estimate warns you, and the provider shows **Near budget**.
- **At 100%**: a generation that would reach the budget, or any one after it was reached, asks **Generate anyway?** before it starts. Nothing starts past a budget without your confirmation, and nothing running is ever cut off midway.

Free providers, like the YouTube Data API, have a daily quota instead of a price, so they take no budget. A budget is per month; the next month starts from zero.

<a id="rates"></a>
## Rates and models without a price

**Rates** is the price list Bardo uses when a provider does not say what a call cost: per million tokens, per thousand characters, per second of video or per hour of audio. A model name covers every model that starts with it. **Edit** changes a price, **Restore** brings Bardo's own price back, and **Add a rate** prices a model the list does not have. A changed price applies to new calls; recorded costs keep theirs.

When a model ran without any price, its calls count as nothing, and the figure **Calls without a price** says which models. **Add price** opens the rate form with that model filled in.

<a id="channels"></a>
## Spend by channel

**By channel** shows what each channel's videos cost this month, with a bar against the channel that spent the most. Calls made for no channel, like a persona's voice sample, count as **Unknown channel**.

<a id="videos"></a>
## The most expensive videos

**Most expensive videos** lists the video projects that cost the most this month, with their channel. A video you removed stays here as **Removed video**, because its calls were paid.
