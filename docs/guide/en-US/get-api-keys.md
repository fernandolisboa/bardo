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

<a id="byteplus"></a>
## BytePlus ModelArk

You need this only for performed clips with Seedance 2.5 straight from BytePlus ModelArk, which makes a cheap 480p draft and then finishes that same draft at 1080p. ModelArk reads a take's control video only from a link, so Bardo also puts the take's files for a while in an object storage (TOS) bucket of yours. The card has five fields: the ModelArk API key, an access key ID and its secret, the bucket and the region.

1. Open the [BytePlus console](https://console.byteplus.com/) and sign in, or create an account. Complete the account's verification when the console asks: ModelArk's Seedance models and TOS need a verified account.
2. Add funds on the [Billing overview](https://console.byteplus.com/finance/overview). ModelArk turns Seedance 2.5 on only for a verified account with at least US$ 30 in Credits, an AI Savings Plan of US$ 30 or more, or a resource pack for the model. Each take is billed in tokens: about US$ 3.70 for a 30-second draft and US$ 20 for its final.
3. Open ModelArk's [Model activation](https://ai.byteplus.com/ark/region:ap-southeast-1/openManagement?LLM=%7B%7D&advancedActiveKey=model) and activate **Dreamina Seedance 2.5**. Stay in the region `ap-southeast-1`, the one in the page's address.
4. Open [API key management](https://ai.byteplus.com/ark/region:ap-southeast-1/apiKey?apikey=%7B%7D), create an API key and name it (for example `Bardo`). Copy the key.
5. Create the bucket. Open the [TOS console](https://console.byteplus.com/tos/bucket), select **Buckets** on the left and then **Create bucket**. Set:
   - **Name**: any free name, such as `bardo-takes-` followed by a few letters of your own (lowercase letters, digits and hyphens);
   - **Region**: `ap-southeast-1`, the same as ModelArk;
   - **Versioning**: off;
   - **Bucket policy**: **Private**.

   Leave the rest as it is and select **Create**.
6. Add a lifecycle rule, so that nothing stays in the bucket if Bardo cannot delete it (for example, while the computer is off for weeks). In the bucket, select **Basic configurations › Lifecycle**, then **Create rule**, and set:
   - **Rule name**: `bardo-takes`;
   - **Status**: **Enable**;
   - **Impact Scope**: **Specify prefix**, `bardo/takes/`;
   - **Effective policy**: **Last modified time**;
   - **Latest file version expiration policy**: on, **Specify days**, `8`, and delete.

   Select **Create** and then **OK**. Bardo deletes each take's files itself as soon as it no longer needs them; this rule is only the backstop.
7. Create an access key that reaches only that bucket. Open **IAM** from your user name at the top of the console, select **Permission policy** and then **Create policy**, name it `bardo-takes` and paste this as its content, with your bucket's name in place of `BUCKET` (twice):

   ```json
   {
     "Statement": [
       {
         "Effect": "Allow",
         "Action": ["tos:HeadBucket"],
         "Resource": ["trn:tos:::BUCKET"]
       },
       {
         "Effect": "Allow",
         "Action": ["tos:PutObject", "tos:GetObject", "tos:DeleteObject"],
         "Resource": ["trn:tos:::BUCKET/bardo/takes/*"]
       }
     ]
   }
   ```

   Select **Submit**. Then select **User management › User** and **Create new user**, name it `bardo`, turn on **Programmatic access** and leave **Console access** off, select **Next**, add the `bardo-takes` policy, select **Next** and **Submit**. On the user's **Access key** tab, select **Create access key** and copy the access key ID and the secret access key. The secret shows only this once.
8. Fill in the **BytePlus ModelArk** card in [Settings › API keys](bardo:go/settings/keys): the API key, the access key ID, the secret access key, the bucket's name and the region (`ap-southeast-1`, unless you made the bucket elsewhere). Select **Save key**, then **Test key**. The test asks ModelArk for a task that cannot exist and checks the bucket, which costs nothing worth counting.

Seedance 2.5 refuses real faces in what it is sent, so a photo of you goes through ModelArk's asset library instead, by its id on the persona's look (Personas › The look). A drawn look can go as its image, or by an asset id too. In ModelArk's [Model Playground](https://ai.byteplus.com/ark/region:ap-southeast-1/experience/gen_video?model=dreamina-seedance-2-5-260628):

- **Your own likeness**: select the **My assets** tab below the input box, then **Real-human** and **Add real-human assets**, and follow the steps: you verify yourself on your phone and upload clear, front-facing pictures. Accept the asset when it shows as authorized, open it and copy its asset ID once its status is **Active**.
- **A preset character**: select the **Digital Character Library** tab, pick a character and copy its asset ID. It replaces a drawn look's face with ModelArk's character.
- **A virtual portrait**: if ModelArk offers you a private virtual portrait under **My assets**, made from your drawn look, copy its asset ID the same way. ModelArk requires it not to resemble any real person.

<a id="claude"></a>
## Claude

1. Open the [Claude Console](https://platform.claude.com/) and sign in, or create an account.
2. Add credits: open [Settings › Billing](https://platform.claude.com/settings/billing) and select **Buy credits**. The API does not answer without a balance. You can turn on **Auto-reload** on the same page.
3. Open [Settings › API keys](https://platform.claude.com/settings/keys) and select **Create key**. Give it a name (for example `Bardo`), pick an expiration, and leave **Linked account** as yourself.
4. Copy the key. It starts with `sk-ant-` and shows only this once.
5. Paste it into the **Claude** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**.

<a id="elevenlabs"></a>
## ElevenLabs

1. Open [ElevenLabs](https://elevenlabs.io/app/sign-up) and sign in, or create an account. Narration and music spend the credits of your plan. Music over the API needs a paid plan (Starter or above); paid plans also carry the commercial license for the music.
2. Open [Developers › API Keys](https://elevenlabs.io/app/developers/api-keys) and select **Create key**. Name it (for example `Bardo`).
3. Leave **Restrict Key** on, give the key access to the four features Bardo calls, and leave the rest at **No Access** (you can also set a credit limit for the key):
   - **Text to Speech**: Access (the narration);
   - **Voices**: Read (your voices, clones included);
   - **Forced Alignment**: Access (word timings for the captions);
   - **Music**: Access (the video's music, if you generate it in Bardo).
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

<a id="kling"></a>
## Kling

You need this key only for video clips straight from Kling. Kling's models are also offered through Higgsfield; going direct costs the same at 1080p and less at 720p, paid from Kling's own resource packages.

1. Open the [Kling API platform](https://kling.ai/dev) and sign in. Your developer account is the same as your Kling AI web account.
2. Buy a video resource package on [Pricing](https://kling.ai/dev/pricing?scrollTo=video). Each clip spends units from it by the second (1 unit is US$ 0.14; Kling 3.0 at 720p spends 0.6 units a second). There is a trial package for a first try. Packages expire, and without units Kling refuses the call.
3. Open [API Keys](https://kling.ai/dev/api-key) and select **+ Create a new API Key**. Name it, for example Bardo, and confirm.
4. Copy the key. It shows only this once. Use this API key, not an **Access Key** and **Secret Key** pair: Kling's newer endpoints, the ones Bardo calls, do not take the pair.
5. Paste it into the **Kling** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**.

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
6. Paste it into the **YouTube Data API** card in [Settings › API keys](bardo:go/settings/keys), select **Save key**, then **Test key**. The test uses 1 unit of the day's quota. The card asks you to accept Bardo's [privacy policy](https://fernandolisboa.github.io/bardo/en-US/privacy.html) first, once.
