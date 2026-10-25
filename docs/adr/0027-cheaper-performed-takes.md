# ADR-0027: Cheaper performed takes: Kling direct, a cap per video, a test take and preflight checks

- Status: Accepted
- Date: 2026-10-25
- Issue: #243
- Builds on: the performance capture spec (`docs/spec/performance-capture.md`), ADR-0025 (BytePlus ModelArk takes and its bucket)

## Context

Performed takes through Higgsfield cost far more than one video is worth to make: a user spent US$ 25 on takes and did not get one whole video. Three things drove it: every take went to the most expensive tier, nothing showed what the whole video would cost before it was spent, and a take whose recording or look did not suit the model was only found out after it was paid for. Kling's own API, checked on 2026-10-25, offers:

- **Motion Control straight from Kling.** `POST /motion-control/kling-3.0` and `/motion-control/kling-2.6` take a prompt, the character image (a URL or raw Base64) and the motion video (a URL only), with `character_orientation: video` (takes up to 30 s), `resolution` 720p or 1080p, `audio: off` and an `external_task_id`. Kling has no upload endpoint and keeps no files.
- **Prices per output second.** Kling 3.0 at US$ 0.126 (720p) and US$ 0.168 (1080p), Kling 2.6 at US$ 0.07 (720p). Kling bills the seconds it returns, which can be fewer than it was sent.
- **Documented input needs.** One person, head and upper body in the frame, not too small, no very fast motion, and a character image framed like the motion video (half body against full body).

## Decision

1. **A Kling take generator.** `crates/ai/src/kling_takes.rs` offers Kling 3.0 Motion Control at 720p and 1080p for Recording takes of 3 to 30 s, with the Kling key already kept for clips. Each has an optional draft on Kling 2.6 at 720p, named by the draft tier's own priced model. The look goes inline as Base64; the request's custom task id is a hash of the endpoint and the request as staged (the video's staged name and the look's print, never the signed link or the re-encoded bytes), so a lost answer is found by that id instead of paying again. The cost is recorded from the seconds Kling returns, rounded to the nearest second as Kling bills them.
2. **The control video in storage the user already has.** Kling reads the video only from a link, and public temporary hosts stay out. A take generator says whether it stages the video itself; for one that does not, Bardo stages it with the user's BytePlus bucket when that key is saved (signed for seven days at most when the request is sent, released once the take is made), else with Higgsfield's free upload. With neither key the take is blocked and says what to add.
3. **Optional drafts where the final is a new take.** A model whose final does not finish its draft (Kling direct, Seedance on Higgsfield) offers **Make final without a draft** beside **Make draft**; ModelArk keeps its draft first. A draft is counted as still to come only where the model requires one.
4. **A cap per video.** The Capture stage shows what the video's performed takes cost so far (prompts, tests, drafts and finals, from Costs) and about what they still need: each take's prompt, a required draft and its final, until made. The cap is US$ 10 unless the user sets another (US$ 1 at least), kept on the video project (migration 0058). Before any paid take, the projection plus that take (unless the take is already in the projection) is checked against the cap; past it, the take waits for the user's word. The user's word skips the budget check too, as it already does for budgets.
5. **A test take per capture.** Before a capture's first draft or final, the user makes and approves a test: up to the first 5 s of the first ready take, cut on the computer from its control video and sent at the model's cheapest tier, recorded under its own cost purpose. A test holds for the persona's look it was made with; captures that already had a draft or final are exempt. One test is kept per capture; a new one replaces it.
6. **Free preflight checks.** Each take shows checks read from its analysis and the look before anything is paid: one person, in frame (no more than 10% of frames without a face, at an edge or without room below the chin), face large enough (at least 8% of the frame's height on average), steady motion (no more than 5% of frames moving faster than two face heights a second) and, for models that follow the look's framing, the same framing as the look (split at a face 16% of the picture high). The analysis gains these measures (migration 0058); the look's face is measured once per look image by a small free job. A failed check holds the take's paid steps until the user sends it anyway, which holds for the take's approved control video; a check that cannot be read yet holds nothing.

## Consequences

- A 30-second take straight from Kling costs about US$ 3.80 at 720p, against about US$ 5 at 1080p, and a test about US$ 0.35, so a failed look or recording costs cents instead of a whole final.
- The cap and the test are new steps before the first paid take; the cap can be raised per video, and the test is skipped for captures that already made takes.
- Kling direct needs a BytePlus or Higgsfield key for the control video; the guide says so where the take is blocked.
- Whether Kling's Motion Control accepts the staged links, the inline look and the 2.6 draft as documented is not confirmed against a live account; they are the first Kling checks of the regression pass.
