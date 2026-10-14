# ADR-0012: YouTube data retention and the privacy policy

- Status: Accepted
- Date: 2026-10-14
- Amends: ADR-0004 (research results are deleted 30 days after they are fetched), ADR-0010 (backups leave out the YouTube data that expires)

## Context

Bardo uses the YouTube Data API and the YouTube Analytics API. The [YouTube API Services Developer Policies](https://developers.google.com/youtube/terms/developer-policies) (checked 2026-10-14, last updated 2026-09-14) ask every API client, audited or not, for:

- **III.A.1–2:** a link to the YouTube Terms of Service, terms that bind users to them, and a privacy policy users accept before using the features: always reachable, naming the YouTube API Services, linking the Google Privacy Policy, saying what is read, kept, used and shared, whether cookies are used, how to revoke access (Google's security settings page), and a contact.
- **III.D.1:** consent again before data is used in a way the accepted policy did not describe.
- **III.D.2:** revoking access in the client revokes the token and deletes the Authorized Data within 7 days; access revoked on Google's page is noticed by checking tokens periodically, and its data deleted within 30 days.
- **III.E.4:** no API data stored more than 30 days, except a channel's own statistics read with its authorization, while that authorization and the video's existence are confirmed every 30 days; derived data goes with what it came from; a way to delete stored data on request.

Bardo had none of this (issue #145). III.C and III.E.4.h (aggregated insights and derived metrics, which niche research and theme ranking produce) are a product question, not a retention one, and are tracked in issue #158.

## Decision

1. **The privacy policy is a guide page** (`docs/guide/<language>/privacy.md`, id `privacy`), shown in the app's Guide and published on the guide's website, so `https://fernandolisboa.github.io/bardo/<language>/privacy.html` is the address Google's forms ask for. Its version is `PRIVACY_POLICY_VERSION`; raise it when Bardo starts using data in a way the policy did not describe.
2. **Consent before YouTube.** The profile keeps the version accepted and when (`user_profile.privacy_version`, `privacy_accepted_at`). Until the current version is accepted, Bardo refuses to save the YouTube Data API key or YouTube app credentials, to test the key, to connect or check a YouTube account, to research niches, to read YouTube posts' numbers, to upload to YouTube and to change a YouTube schedule. A profile that already had YouTube set up (or accepted an older version) sees a bar over the screens until it accepts. The YouTube key card, the YouTube app card and an unconnected YouTube account ask for it in place; Settings › About shows the state, the policy, the YouTube Terms of Service, the Google Privacy Policy and Google's access page.
3. **The retention rule:** nothing read from YouTube is kept more than 30 days, except the numbers of a channel's own posts while Bardo holds that channel's authorization and the network confirmed it within the last 30 days. A connection is confirmed when it is made or its tokens are refreshed (`confirmed_at` = the later of the two).
4. **Own posts are read as the channel.** A metrics sync reads the public numbers of a connected channel's posts with its access token (`videos.list` with OAuth), so they are Authorized Data; posts of a channel that is not connected are read with the API key and are kept 30 days at most. The owner's numbers (YouTube Analytics) and retention curves already come through the token.
5. **A daily upkeep** runs when the window opens (after 20 seconds) and every day after, and in the background agent's daily step. It first confirms each YouTube connection, when the policy is accepted: it renews the tokens when due and reads the channel again. Then it forgets each connection not confirmed since the cutoff, tokens and state included, with what was read through it, and deletes the owner's YouTube data the rule no longer allows. The cutoff is 29 days ago, so nothing kept is older than 30 days at the next upkeep. It deletes niche research fetched and theme rankings made before the cutoff; numbers of posts not authorized taken before it; the owner's numbers and retention curves of posts whose account is not authorized, whatever their age; and every number of a post YouTube has not shown since before the cutoff (III.E.4.b's existence check). Deleting needs no call to YouTube, so it runs without the policy accepted too.
6. **Refresh rather than lose.** When Bardo opens, a YouTube post last read 25 days ago or more is synced whatever *Sync on start* says.
7. **Revocation.** Disconnecting YouTube revokes the token at Google (as before) and deletes at once the owner's numbers and retention curves of the account's posts and its numbers taken before the cutoff. Access revoked on Google's page makes the next renewal fail (*Reconnect needed*); the connection then goes unconfirmed and the upkeep forgets it with its data within 30 days.
8. **Deletion on request:** *Delete YouTube data…* in Settings › About deletes at once all niche research, theme rankings and the numbers and retention curves of the owner's YouTube posts. The channels, themes, projects, connections and each post's link stay, and nothing on YouTube changes.
9. **Backups** leave out what expires: niche research, theme rankings, and the numbers of YouTube posts whose channel has no connection in the copy. A connected channel's numbers stay; restored, the upkeep checks that connection again before anything else.

## Consequences

- Research is fetched again after 30 days at most, and ranking a theme again costs a decision engine call; the reuse of a cached result (ADR-0004, 7 days) still applies within those 30 days.
- A channel that is not connected keeps at most 30 days of public numbers, so its Performance history and the first-week views theme ranking reads (ADR-0004) fade after a month. Connecting the channel keeps its history.
- A connected channel's history survives as long as Bardo opens at least once every 30 days (or the background agent runs). A month away from Bardo without the agent forgets the connection and its numbers; reconnecting reads current numbers again.
- An old backup restored after a disconnection brings back the channel's numbers with a connection whose tokens are not on the computer; it shows *Reconnect needed* (ADR-0010) and the upkeep forgets it 30 days after it was last confirmed.
- The contact the policy names is the repository's issue tracker.
