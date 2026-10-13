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
- **ElevenLabs** narrates and lists your voices, clones included. You need it for the narration and the persona voices.
- **Gemini** makes scene images (Nano Banana) and video clips (Veo, Gemini Omni Flash). You need it for the scene images, and for clips only if you use Google's models.
- **Higgsfield** makes video clips through the models it offers. You need it only for clips, if you pick its models.
- **TypeSafe (JEV)** is the decision engine: it ranks video ideas and scores cut suggestions. You need it to rank ideas and suggest cuts.
- **YouTube Data API** gives niche research and public video statistics. You need it for research and your posts' public numbers.

You do not need every key on day one. A screen that needs a missing key says so. Clips are optional: a scene without a clip keeps its still image.

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
