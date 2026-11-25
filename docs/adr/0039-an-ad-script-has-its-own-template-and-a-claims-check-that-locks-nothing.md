# ADR-0039: An ad script has its own template, and a claims check that flags without locking

- Status: Accepted
- Date: 2026-11-25
- Issue: #276
- Builds on: ad angles, ad projects and the ad brief (ADR-0038), the product sheet and its claims (ADR-0037), the storyboard script (#258), project lengths, paid calls sent once (ADR-0030), text jobs waiting for each other (ADR-0036)

## Context

Since ADR-0038 an ad project's script was written from the script template with the brief appended, as a bridge. That template asks for a story or an explanation of 45 seconds or more; an ad is a hook, a few steps and a call to action in 15 or 30 seconds, and the only things it may state about its product are the claims the user confirmed.

Telling a model to state only the listed claims is not the same as it doing so. Nothing read the script back, so a superlative or a number the brief does not hold reached the narration unnoticed, and the user's own edits were never read at all. The slices that follow (disclosure and ad metadata) also need to know which claims a script actually uses.

## Decision

1. **A fifteen-second length for every project.** `VideoLength::Seconds15` (about 30 to 38 words) joins 30 seconds, 1 minute and 3 minutes. No network refuses it, and it sets the script's target and the whole-video estimate like the others.
2. **An ad's script aims 2 seconds under its length.** The project decides its own target: an ad project asks for the ad variant of its length (15 seconds: about 26 to 32 words), so the video ends on time once the end of the ad is added. Every other project keeps the targets it had.
3. **An ad project is written from the *Ad script* template; every other project from the script template, exactly as before.** The new kind has its own variables (the product, whether it is a concept, the segment, the hook, the framework with its steps, the brief's claims numbered, the offer, the call to action, the must-say and never-say notes) beside the channel's, the theme's and the target length. The bridge of ADR-0038 (the brief appended to the script template) is gone. Every value that came from the user, a page or a model is sent inside tags with `<` written as `&lt;`.
4. **The fixed blocks follow the template.** The ad rules of ADR-0038 come after the template's instructions, and for a product of kind *idea* the concept rules after those: speak of it as an idea, never say it can be bought, ordered, downloaded or used today, and state no price and no "buy now" unless the user's own call to action does. The template also carries a paragraph on concepts and the `{{product_concept}}` variable, which an edit may remove; the fixed block is what cannot be edited away. Whether a product is a concept is its kind, read when the script is asked for.
5. **The claims check is a call of its own, not a template.** Fixed instructions, the everyday effort and a fixed JSON schema: the brief's claims and the script's sentences go numbered, and the answer lists statements as a sentence number, what the sentence states and a claim number, 0 for none. The domain parses it: a claim number the brief does not hold makes the statement unbacked, so a model cannot back a statement with a claim that does not exist, and a sentence number the script does not have keeps the statement without its sentence instead of dropping the flag. The claims, the advertiser's words and the script are data to the checker, never instructions.
6. **One job writes and checks.** The script job of an ad project carries what the check reads of the brief (the claims in order and the advertiser's words) as it was when the job was queued, and checks the script right after saving it. A job taken up again finds the script it already saved, by its job id, and pays only for the check; a check it already saved is not paid for twice (ADR-0030). A check on request is a second job kind handled by the same handler, with the script's text as it was when asked. Both are the same `Work` as writing the script (ADR-0036), so a check never runs beside a rewrite of the script it reads.
7. **A project keeps two checks at most.** `claims_check` holds one row for the current script and one for a new script waiting for review, each with the text and the brief's `changed_at` it read, the statements in order, the model and the job. Accepting the new script gives it its check in the same step as the script changes hands; rejecting drops it. After the paid call the handler looks at where the text is now (accepted or put aside while Claude read it) before choosing the row. A check left from an earlier pending script is shown only when it reads the pending text.
8. **Out of date is computed, not stored.** A check is out of date when its text is no longer the script's or the brief changed after the `changed_at` it read. Checking again replaces the row whole.
9. **Flags lock nothing.** The Script stage lists each flagged sentence with what it states, and the claims the script uses with the sensitive ones apart (read from the sheet as it is now, as in the brief). Stage states do not change with flags. Starting a narration says beside the estimate that the script has unbacked statements, or was not checked as it reads now; it still starts. The user decides whether a sentence stays.
10. **The check is priced and budgeted like any paid call.** A cost purpose of its own; the estimate of a new ad script and the whole-video estimate of an ad without a script count one check, the on-request check shows its own estimate from the request it would send, and each is checked against Claude's budget and the video's cap, and recorded under the project.

## Consequences

- A script template edited for a channel no longer shapes that channel's ads; the ad script template does. Ads written before this change keep their script, with no check until one is asked for.
- Every generated ad script costs one more small call. A failed check fails the job after the script is saved: the script is there, the stage says the generation stopped, and running the job again pays for the check only.
- The check is a model's reading. It can miss a statement or flag a fair one, which is why it informs and never blocks, and why its instructions are not a template.
- A length change on the Brief stage moves the brief's `changed_at`, so it shows the check as out of date although no claim changed. Checking again is cheap, and the script is marked to be read again in that case anyway.
- Later slices read the stored check: which claims a script uses, and which of them are sensitive, without calling a model again.
- An ad script no longer receives the link or handle: the call to action is spoken in the user's words, and the link belongs on the end card and in the post.
