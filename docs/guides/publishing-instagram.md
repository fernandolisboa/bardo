# Connecting an Instagram account

Bardo signs in to Instagram through a Meta app you register in your own
Meta developer account (ADR-0008), with the Instagram API with Facebook
Login for Business. Bardo ships no app of its own, so the app, its access
level and any review belong to you. This guide sets the app up once; after
that, each Instagram account connects from its network account card.

## What you need

- An Instagram **professional** account (Business or Creator).
- A **Facebook Page** linked to that Instagram account. Uploading a local
  video file is documented only for apps that use Facebook Login for
  Business, and that login reaches Instagram through the linked Page.
- A Facebook account with a role on the Page (it can create content there)
  and on the Meta app below.
- Access to [Meta for Developers](https://developers.facebook.com/apps/).

To link the account to a Page: in Instagram, open **Settings › Accounts
Center** (or **Edit profile › Page**) and connect the Page, or do it from
the Page's settings in Meta Business Suite.

## 1. Create the Meta app

1. In **My Apps**, select **Create app**.
2. Use case: **Manage messaging & content on Instagram** (the Instagram
   use case). App type, if asked: **Business**. Do not set the app type to
   Native or Desktop: Facebook Login for Business needs a web login.
3. In the use case's settings, open **API setup with Facebook login** and
   add **Facebook Login for Business** if it is not there yet.
4. In **App settings › Basic**, copy the **App ID** (a number) and the
   **App secret** (select **Show**).

The app stays in **Development** mode with **Standard Access**: that serves
every account whose Facebook user has a role on the app, which is you. No
App Review and no Business Verification are needed for your own accounts.

## 2. Save the app in Bardo

1. Open **Settings › Networks**.
2. Paste the app ID and secret into **Instagram · Meta app** and select
   **Save**.

Both are kept in Windows Credential Manager under your Windows account, per
Bardo profile. They never reach Bardo's database, logs or error messages,
and the secret never shows on screen again; the card shows its last four
characters.

## 3. Generate a token

Facebook Login does not hand a desktop app its sign-in through the browser
the way Google does: it only returns to an exact HTTPS address or to an
embedded web view. So you sign in once in Meta's Graph API Explorer and
paste the token it gives into Bardo.

1. Open the [Graph API Explorer](https://developers.facebook.com/tools/explorer/)
   (the account card's **Open the Graph API Explorer** button goes there).
2. Under **Meta App**, pick your app. Under **User or Page**, choose
   **Get User Access Token**.
3. Tick these permissions:
   - `instagram_basic`
   - `instagram_content_publish`
   - `instagram_manage_insights`
   - `pages_show_list`
   - `pages_read_engagement`

   If the Page belongs to a business portfolio (Meta Business Suite) and
   your role on it comes from there, also tick `business_management`,
   `ads_management` and `ads_read`.
4. Select **Generate Access Token**, log in, and in the dialog choose the
   Page and the Instagram account Bardo may use. Allow every permission.
5. Copy the token from the **Access Token** field.

The token lasts about an hour, so paste it in Bardo right away.

## 4. Connect the account

1. Open **Accounts**, pick the channel and add (or open) its Instagram
   Reels account.
2. Select **Connect**, paste the token and select **Connect** again.
3. Bardo trades the token for a long-lived one, checks that every
   permission was allowed, and finds the Pages it reaches with their
   linked Instagram account:
   - one account: the card shows **Connected as @username**;
   - several: the card lists each one with its Page; pick the one for this
     channel with **Connect this one**;
   - no Page, or no Page with a linked Instagram professional account: the
     card says which, and nothing is kept.

Bardo never stores the token you pasted. It keeps, in Windows Credential
Manager per profile and network account, the long-lived user token and the
chosen Page's token, which is what publishes. Bardo's database keeps only
the Instagram account id and username, the permissions, the expiry and the
last renewal.

## Day to day

- **The long-lived token lasts about 60 days.** Bardo trades it for a fresh
  one when it is within a week of expiring, whenever it next uses the
  account (a check, an upload, a metrics sync), and reads the Page's token
  again. Opening Bardo at least once every seven weeks keeps the account
  connected without pasting again.
- **Check** does that renewal if it is due and reads the account's username
  again. If the Page is now linked to another Instagram account, the card
  turns to **Reconnect needed**.
- **Reconnect needed** means Meta refused the token: it expired, it was
  revoked (removing the app from your Facebook settings does this), your
  password changed, Meta asked you to re-authorize the app (data access
  expires 90 days after you last used it in a Meta login), or the Page is
  no longer linked. Generate a new token as in step 3 and select
  **Reconnect**.
- **Disconnect** removes Bardo's access at Meta and forgets the tokens.
  Meta revokes an app's access for the whole Facebook account at once, so
  when another account in the same Bardo profile is still connected through
  the same app, Bardo forgets this account's tokens and leaves the access in
  place, and says so. If Meta cannot be reached, Bardo still forgets the
  tokens and tells you to remove the app from your Facebook account's
  **Settings & privacy › Business integrations**.
- A connected account must be disconnected before it can be removed.

## If the Page needs publishing authorization

Meta may ask a Page's admins to complete **Page Publishing Authorization**
(an identity check in the Page's settings) before anything is published
through it. If an upload is refused for that reason, complete it in the
Page's settings, then try again.
