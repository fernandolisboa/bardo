---
id: get-api-keys
title: Getting each API key
group: getting-started
---

# Getting each API key

How to get the key for each provider in [Settings › API keys](bardo:go/settings/keys), one section per provider. In Bardo, each card's **Step by step** shows the same steps on a screen of their own, with the card's field at the end to paste the key into. See [Your API keys](api-keys.md) for what each provider does.

A few things hold for every provider:

- **Copy the key as soon as it shows.** Most providers show a new key only once; if you lose it, create another one and replace the old one in Bardo.
- **Keep it to yourself.** Paste it only into Bardo; Bardo keeps it in Windows Credential Manager and masks it everywhere else.
- **Test it.** After saving, **Test key** makes the cheapest call that needs the key and says what is wrong, if anything.

<a id="claude"></a>
## Claude

1. Open the [Claude Console](https://platform.claude.com/) and sign in, or create an account.
2. Add credits: open [Settings › Billing](https://platform.claude.com/settings/billing) and select **Buy credits**. The API does not answer without a balance. You can turn on **Auto-reload** on the same page.
3. Open [Settings › API keys](https://platform.claude.com/settings/keys) and select **Create key**. Give it a name (for example `Bardo`), pick an expiration, and leave **Linked account** as yourself.
4. Copy the key. It starts with `sk-ant-` and shows only this once.
5. Paste it into the **Claude** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**.

<a id="elevenlabs"></a>
## ElevenLabs

1. Open [ElevenLabs](https://elevenlabs.io/app/sign-up) and sign in, or create an account. Narration spends the credits of your plan.
2. Open [Developers › API Keys](https://elevenlabs.io/app/developers/api-keys) and select **Create key**. Name it (for example `Bardo`).
3. Leave **Restrict Key** on, give the key access to the three features Bardo calls, and leave the rest at **No Access** (you can also set a credit limit for the key):
   - **Text to Speech**: Access (the narration);
   - **Voices**: Read (your voices, clones included);
   - **Forced Alignment**: Access (word timings for the captions).
4. Select **Create key** and copy the key. It shows only this once.
5. Paste it into the **ElevenLabs** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**. A key missing one of those features fails with a message naming it.

<a id="gemini"></a>
## Gemini

1. Open [Google AI Studio](https://aistudio.google.com/) with your Google account and accept the terms. A new account gets a Google Cloud project made for it; if you already use Google Cloud, bring a project in from [Projects](https://aistudio.google.com/projects) with **Import projects**.
2. Set up billing. Google's image models (Nano Banana) and video models (Veo) have no free tier, so the project needs a billing account. On [API Keys](https://aistudio.google.com/api-keys) or [Projects](https://aistudio.google.com/projects), select **Set up billing** next to the project and follow the steps; a prepaid account needs a balance, which you manage in [Billing](https://aistudio.google.com/billing). When a prepaid balance reaches zero, Google refuses the calls until you top it up.
3. On [API Keys](https://aistudio.google.com/api-keys), select **Create API key** and pick the project.
4. Copy the key. Depending on when it was made it starts with `AIza` or `AQ.`; copy all of it.
5. Paste it into the **Gemini** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**.

<a id="higgsfield"></a>
## Higgsfield

You need this key only for video clips from Higgsfield's models.

1. Open the [Higgsfield console](https://open.higgsfield.ai/auth/sign-up) and create an account, or sign in.
2. Add credits on [Credits](https://open.higgsfield.ai/credits). Each generation is paid from that balance; without credits Higgsfield refuses the call.
3. Open [API keys](https://open.higgsfield.ai/api-keys) and create a key. Higgsfield gives each key two parts: a **key ID** and a **secret**.
4. Copy both. Bardo takes them in one field, joined by a colon: `KEY_ID:KEY_SECRET` (the ID, a `:`, then the secret, with no spaces).
5. Paste that into the **Higgsfield** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**.

<a id="typesafe"></a>
## TypeSafe (JEV)

1. Open the [TypeSafe console](https://console.typesafe.ai/) and sign in, or create an account.
2. Open [API keys](https://console.typesafe.ai/keys) and create a key. Name it (for example `Bardo`).
3. Copy the key.
4. Paste it into the **TypeSafe (JEV)** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**.

The [TypeSafe documentation](https://docs.typesafe.ai/introduction/quickstart) has more on keys and the JEV models.

<a id="youtube-data"></a>
## YouTube Data API

This is a plain API key in a Google Cloud project, apart from the YouTube sign-in that Connecting YouTube sets up. The API is free within a daily quota of 10,000 units.

1. Open the [Google Cloud console](https://console.cloud.google.com/) and pick a project, or [create one](https://console.cloud.google.com/projectcreate) (for example `bardo`). The project you use for YouTube's sign-in will do.
2. Enable [YouTube Data API v3](https://console.cloud.google.com/apis/library/youtube.googleapis.com) with **Enable**.
3. Open [Credentials](https://console.cloud.google.com/apis/credentials) and select **Create credentials › API key**.
4. Under **API restrictions**, choose **Restrict key** and tick **YouTube Data API v3** (Google asks for a restriction before it makes the key). Leave **Authenticate API calls through a service account** off. Select **Create**.
5. Copy the key from **API key created**. It starts with `AIza`.
6. Paste it into the **YouTube Data API** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**. The test uses 1 unit of the day's quota.
