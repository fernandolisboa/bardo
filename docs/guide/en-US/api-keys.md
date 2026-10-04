---
id: api-keys
title: Your API keys
group: getting-started
place: settings/keys
---

# Your API keys

Bardo has no AI of its own and no account to sign up for. It works with your accounts at each provider, through API keys you create there and save once in [Settings › API keys](bardo:go/settings/keys). You pay each provider directly, at its own prices.

<a id="providers"></a>
## Which provider does what

- **Claude** writes scripts, titles, descriptions, video ideas, scene plans and the prompts for image and video models. You need it for the script, the scenes, the ideas and the post texts.
- **ElevenLabs** narrates and lists your voices, clones included. You need it for the narration and the persona voices.
- **Gemini** makes scene images (Nano Banana) and video clips (Veo, Gemini Omni Flash). You need it for the scene images, and for clips only if you use Google's models.
- **Higgsfield** makes video clips through the models it offers. You need it only for clips, if you pick its models.
- **TypeSafe (JEV)** is the decision engine: it ranks video ideas and scores cut suggestions. You need it to rank ideas and suggest cuts.
- **YouTube Data API** gives niche research and public video statistics. You need it for research and your posts' public numbers.

You do not need every key on day one. A screen that needs a missing key says so. Clips are optional: a scene without a clip keeps its still image.

<a id="where-kept"></a>
## Where your keys are kept

Keys are kept in Windows Credential Manager, under your Windows account, never in Bardo's database. Wherever text leaves the app (logs, error messages, the screen) a key is masked, and Settings shows only its last characters.

Removing a key in Settings deletes it from Credential Manager. To stop a key for good, also revoke it in the provider's own dashboard.

<a id="testing"></a>
## Testing a key

**Test key** makes the cheapest call the provider offers that needs a valid key, and says what it found: the key works, the provider rejected it, it lacks a permission or an enabled API, or a quota or credit balance is blocking it right now. A YouTube Data API test uses 1 unit of your daily quota.

<a id="budgets"></a>
## Costs and budgets

Every generation records what it cost, as the provider reported it, or estimated from the provider's published rate. [Costs](bardo:go/costs) shows what you spent this month, by provider, by channel and by video.

Set a monthly **budget** per provider on the Costs screen. At 80% the navigation warns you, and once a budget is reached, each new job for that provider asks before it starts. Nothing is ever cut off midway.
