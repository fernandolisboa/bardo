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

A call that gave you nothing can still cost money, and it is counted. An answer the provider cut short or left empty is counted from what the provider says it used. A request that was sent and never answered is counted as an estimate of what such a call usually costs, because the provider may have charged for it; see [A paid request got no answer](troubleshooting.md#answer-lost).

<a id="budgets"></a>
## Budgets

**Budgets** lists each paid provider: what was used, what was spent, and its budget. **Set budget** gives a provider a monthly limit in US dollars; **Change** and **Remove** edit it.

- **From 80%**: before a generation that would take a provider to 80% of its budget or more, the estimate warns you, and the provider shows **Near budget**.
- **At 100%**: a generation that would reach the budget, or any one after it was reached, asks **Generate anyway?** before it starts. Nothing starts past a budget without your confirmation, and nothing running is ever cut off midway.

Free providers, like the YouTube Data API, have a daily quota instead of a price, so they take no budget. A budget is per month; the next month starts from zero.

The X API is a provider too, with no key: each post Bardo makes on X is billed from your X app's credits and recorded here under X (see [Connecting X](connect-x.md#costs)). A budget for X makes a post that would reach it ask first.

<a id="project-keys"></a>
## Projects' own keys

A project can pay a provider with its [own key](project-keys.md) and keep its own monthly budget for it. What a project's key pays shows under the provider in **Budgets** as the part paid by projects' own keys, and does not count against the app's budget there. **Projects' own keys** lists, for each project with its own key (only the filtered one under a filter), what it paid this month against the project's budget, with the same warnings from 80% and the same question at 100%. Each call records which key paid it, so a key removed later still shows what it paid this month.

<a id="rates"></a>
## Rates and models without a price

**Rates** is the price list Bardo uses when a provider does not say what a call cost: per million tokens, per thousand characters, per second of video, per hour of audio, per thousand web searches or per post. A [product research](products.md#research) is billed in four token meters and one more: besides input and output tokens, Claude charges the tokens it writes to its cache and the ones it reads back from it between steps, each at its own rate, and each web search. A model name covers every model that starts with it. **Edit** changes a price, **Restore** brings Bardo's own price back, and **Add a rate** prices a model the list does not have. A changed price applies to new calls; recorded costs keep theirs. Clips on the [Higgsfield plan](api-keys.md#higgsfield-plan) are paid in credits: their rates are dollars at US$ 0.049 a credit, the Plus plan's price, so on another plan edit them to match yours.

A generation that would call a model without a price asks before it starts and names the model, under **Price unknown** or, when a budget or the video's cap is reached as well, in that question: what it costs cannot be estimated or held against a budget or a video's cap. When a model ran without any price, its calls count as nothing, and the figure **Calls without a price** says which models. **Add price** opens the rate form with that model filled in.

<a id="channels"></a>
## Spend by channel

**By channel** shows what each channel's videos cost this month, with a bar against the channel that spent the most. Calls made for no channel, like a persona's voice sample or look, count as **Unknown channel**.

<a id="videos"></a>
## The most expensive videos

**Most expensive videos** lists the video projects that cost the most this month, with their channel. A video you removed stays here as **Removed video**, because its calls were paid.

<a id="projects"></a>
## Spend by project

**By project** shows what each [project](your-projects.md) spent this month, with its color; calls made for no project, like a persona's voice sample, count as **Not for a project**. The **Project** filter under it picks whose channels and videos **By channel** and **Most expensive videos** show. It starts on the open project; the figures at the top and the budgets stay the whole profile's, since providers bill the profile. A cost keeps the names of the project, channel and video it was for, as they were when it was paid.

<a id="deleted"></a>
## Deleted projects and videos

A project or video you [deleted](your-projects.md#delete) keeps its costs, under the name it had: **<name> (deleted on <date>)**. Deleted projects are grouped in one line, **Deleted projects**, that opens to list them; a deleted video shows among **Most expensive videos**. **Forget the name** shows it as **Deleted project #<number>** (or **Deleted video #<number>**) from then on. The amounts never change, so the month's total is the same before and after a deletion.
