# ADR-0045: An ad's post carries its commercial disclosure and an acknowledged review

- Status: Accepted
- Date: 2026-11-26
- Issue: #282
- Builds on: the upload review (ADR-0008), the claims check (ADR-0039), the synthetic-content disclosure (#145, ADR-0012), concept products (ADR-0043)

## Context

An ad promotes a product: the user's own, or a client's. Each network asks a post that promotes something to say so, apart from the synthetic-content label, and each takes it differently; some take it from an upload and some only in their own app. An ad also says things about its product, in the video and in each network's text, and some of those need the user's care before anything goes out: a sensitive claim, a statement no claim backs, a claim the product no longer confirms, a product in a category with its own ad rules, or a product that is only an idea. Bardo is AI-assisted, not automatic: it sets defaults and reminds, and the user decides.

## Decision

1. **A commercial disclosure per post, from whose the product is.** *None*, *your brand* or *paid partnership*. An ad's posts start at *your brand* for the user's own product and *paid partnership* for a client's. The upload review shows it and the user can change it there; the choice applies to that upload and is kept with its publication. A project that is not an ad has none, and its payloads, stamps and files are as before.
2. **Each network gets what it documents** (checked against each network's documentation on 2026-11-26):
   - YouTube: nothing for *your brand*. For *paid partnership*, the upload sends the part `paidProductPlacementDetails` with `hasPaidProductPlacement: true`, which YouTube shows as *Includes paid promotion*. The `videos.insert` reference does not list the field among those an upload sets, but the API's revision history (November 16, 2026) says `videos.insert` and `videos.update` take it and that it no longer defaults to false; the spec asks Bardo to set it once documented. Once the video is up, the review links its Studio page to check it. A real upload confirms it (a check on the real-key issue).
   - Instagram Reels: nothing for *your brand*. For *paid partnership*, the container carries `is_paid_partnership`, with up to two brand accounts the user types (`branded_content_sponsor_ids`, Instagram account ids). A refusal fails the upload with Meta's reason, and nothing is ever posted without the label.
   - X: nothing for *your brand*; `paid_partnership` on the post for *paid partnership*.
   - TikTok: the inbox upload has no disclosure field, so for either the review says to turn on *Disclose post content* in the app before posting the draft, with *Your brand* or *Branded content* (which TikTok does not allow on a private post).
   - Kick, and every export: a reminder line in `metadata.txt` naming the disclosure and what to turn on when posting by hand (the network's label, TikTok's *Disclose post content*, or, on Kick, to say so in the post).
   - A refusal of the label (Instagram's branded content) fails the upload with the network's own reason, shown on the post.
3. **The synthetic default for an ad.** An ad's posts also start with the synthetic-content disclosure on when its scene plan has a *product in use* scene drawn in a photographic style, or in a style Bardo cannot tell (the channel's own notes): a realistic picture of a real thing that was not filmed.
4. **The post texts are checked like the script.** An ad's metadata comes from its own template, *Ad metadata*, which reads the brief (claims, offer, call to action and link) and always gets the ad rules and, for an idea, the concept rules. The same job then checks the post texts against the brief's claims, as the claims check does for a script: every network's title and text, each part a sentence prefixed by its network, with their own fixed instructions. The check follows the texts it read, is out of date once they or the brief change, and can be run again on request with its estimate. It works on the project's metadata, so it never runs beside a metadata job, and it leaves script work free. A job run again skips a check it already saved, and texts changed while Claude read them take no check.
5. **Items acknowledged before anything goes out.** An ad's upload review and export list each sensitive claim the script or the texts use, each unbacked statement, a script or texts not checked as they read now, each claim the product no longer confirms, a restricted category, and a concept product, each once. Every item has a key built from what it names, so a changed item needs a new acknowledgement. Nothing starts until every item is acknowledged; Bardo refuses an upload or export that leaves one out. The acknowledgements are one list on the Publish stage, shared by the export and every upload review.
6. **The review is kept with what went out.** The disclosure, the brand accounts and the acknowledged keys are saved with the publication of an upload, and the disclosure and keys with each network's export. An upload whose review cannot be saved is not started. A change to the default disclosure or to the items changes the upload review's stamp, so an open review closes and asks again.

## Consequences

- Migration 0077 adds the tables `publication_ad_review` (one row per publication, deleted with it) and `export_ad_review` (one row per project and network, deleted with the project).
- The upload payload carries the paid-partnership flag and the brand accounts only when they are set, so a payload of any other post is unchanged byte for byte.
- Every TikTok ad leaves a step to the user in the app; Bardo says which and cannot tell whether it was done.
- Non-ad uploads, and ads that are not paid partnerships, send YouTube nothing about paid placement, as before; since YouTube no longer reads an omitted value as false, a later slice may declare *false* for them.
- Instagram may refuse the label when the brand has not approved the account as a partner; the user then fixes it with the brand or posts with another disclosure.
- The check of the post texts is one more small paid call in the metadata job of an ad, counted in its estimate.
