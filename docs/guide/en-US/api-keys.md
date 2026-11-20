---
id: api-keys
title: Your API keys
group: getting-started
place: settings/keys
tour: keys
---

# Your API keys

Bardo has no AI of its own and no account to sign up for. It works with your accounts at each provider, through API keys you create there and save once in [Settings › API keys](bardo:go/settings/keys). You pay each provider directly, at its own prices. [Show me the tab](bardo:tour/keys).

<a id="providers"></a>
## Which provider does what

- **Claude** writes scripts, titles, descriptions, video ideas, scene plans and the prompts for image and video models. You need it for the script, the scenes, the ideas and the post texts.
- **ElevenLabs** narrates, lists your voices (clones included) and composes the video's music. You need it for the narration and the persona voices, and for music only if you generate it in Bardo (a paid ElevenLabs plan).
- **Gemini** makes scene images (Nano Banana) and video clips (Veo, Gemini Omni Flash). You need it for the scene images and to draw a persona's [look](personas.md#look), and for clips only if you use Google's models.
- **Higgsfield** makes video clips through the models it offers. You need it only for clips, if you pick its models.
- **Higgsfield plan** makes video clips paid from the credits of your higgsfield.ai plan instead of an API key. You sign in to it in the browser ([below](#higgsfield-plan)).
- **Kling** makes video clips straight from Kling's own models (Kling 3.0, 2.6 and 2.5 Turbo), and performed clips with Kling 3.0 Motion Control ([Performance capture](capture.md#kling)). You need it only if you pick its models.
- **BytePlus ModelArk** makes performed clips with Seedance 2.5 straight from ModelArk, whose final finishes the approved draft. You need it only for performed clips, if you pick that take model. Its card takes the ModelArk key and an object storage bucket of yours for the take's files.
- **TypeSafe (JEV)** is the decision engine: it ranks video ideas and scores cut suggestions. You need it to rank ideas and suggest cuts.
- **YouTube Data API** gives niche research and public video statistics. You need it for research and your posts' public numbers.

You do not need every key on day one. A screen that needs a missing key says so. Clips are optional: a scene without a clip keeps its still image.

<a id="higgsfield-plan"></a>
## The Higgsfield plan

Higgsfield sells two separate things: its API, with a dollar balance and a key (the **Higgsfield** card), and the plans on higgsfield.ai, with monthly credits. Bardo can spend the plan's credits on clips through Higgsfield's official MCP server, which Higgsfield's terms open to apps like Bardo. Its card is under **Plans you sign in to**, below the keys.

1. On the **Higgsfield plan** card, click **Sign in**. Bardo opens Higgsfield's page in your browser.
2. Sign in with your higgsfield.ai account and allow Bardo. The page says Bardo is connected; close the tab.
3. The card says **Signed in** and shows your plan and the credits left. After Bardo restarts, **Check credits** reads them again.

Then pick a model marked **Higgsfield plan** for your clips ([Video model](clips.md#model)). Before each clip, Bardo asks Higgsfield what it costs in credits and refuses it when the plan cannot pay. Each clip is sent once: if its answer is lost on the way, Bardo looks for it in your Higgsfield library instead of sending it again, and when the library doesn't list it the scene says to check your library before animating it again. Each clip is recorded in [Costs](bardo:go/costs) at the per-second rates of the plan's models, priced at US$ 0.049 a credit (what a credit costs on the Plus plan). On another plan, edit those rates in Costs.

The sign-in is kept in Windows Credential Manager like a key, and renewed on its own. If Higgsfield stops accepting it, the card says **Sign-in expired**; click **Sign in again**, then retry any clip that stopped for it in [Jobs](jobs.md). **Sign out** forgets it in Bardo; to end Bardo's access at Higgsfield too, remove it in your Higgsfield account.

<a id="decision-engine"></a>
## Decision engine

The **Decision engine** at the top of the tab picks the engine that ranks video ideas on [Themes](themes-ranking.md) and scores [cut suggestions](editor.md) in the editor. The engine calls with its provider's key from this tab, its estimate counts against that provider's budget, and its costs are recorded under that provider.

For now there is one engine, **JEV (TypeSafe)**, so it uses your TypeSafe key. A new choice applies to the next ranking or scoring; a job already queued finishes with the engine it started with. The choice travels in a [settings file](backup.md#settings-file) with your other preferences.

<a id="step-by-step"></a>
## Step by step for each key

Each card's **Step by step** opens a screen that walks you through getting that provider's key: where to sign up, which page creates the key (with a link straight to it), what to tick, and what the key looks like. The last step is the card's own field, so you paste, save and test the key without leaving that screen. **Back** returns to the tab.

The same steps are in [Getting each API key](get-api-keys.md).

<a id="where-kept"></a>
## Where your keys are kept

Keys are kept in Windows Credential Manager, under your Windows account, never in Bardo's database. See [Where your data and keys live](data-and-keys.md#credentials).

Removing a key in Settings deletes it from Credential Manager. To stop a key for good, also revoke it in the provider's own dashboard.

<a id="masked"></a>
## What Bardo shows of a key

Once saved, a key never shows again: its card says **Key saved, ending in** its last four characters, so you can tell which key it is. Wherever text leaves the app (the log, error messages, the screen) a saved key is masked, even inside a provider's own message. To change a key, paste the new one and **Replace key**.

<a id="testing"></a>
## Testing a key

**Test key** makes the cheapest call the provider offers that needs a valid key, and says what it found: the key works, the provider rejected it, it lacks a permission or an enabled API, or a quota or credit balance is blocking it right now. A YouTube Data API test uses 1 unit of your daily quota.

<a id="budgets"></a>
## Costs and budgets

Every generation records what it cost, as the provider reported it, or estimated from the provider's published rate. [Costs](bardo:go/costs) shows what you spent this month, by provider, by channel and by video.

Set a monthly **budget** per provider on the Costs screen. From 80% a generation's estimate warns you, and once a budget is reached, each new job for that provider asks before it starts. Nothing is ever cut off midway. See [Costs and budgets](costs.md#budgets).
