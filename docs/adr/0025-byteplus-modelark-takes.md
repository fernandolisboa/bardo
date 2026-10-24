# ADR-0025: Seedance 2.5 takes straight from BytePlus ModelArk

- Status: Accepted
- Date: 2026-10-23
- Issue: #194
- Builds on: the performance capture spec (`docs/spec/performance-capture.md`), ADR-0008 (keys per provider in Windows Credential Manager)

## Context

Seedance 2.5 Video Edit already runs through Higgsfield (#192), whose finals are new takes: Higgsfield exposes no draft task or seed, so an approved draft only says what the final may look like. ModelArk's own API for `dreamina-seedance-2-5-260628` has a draft mode. ModelArk's docs, checked on 2026-10-23, describe:

- **Draft and final.** `draft: true` makes a 480p draft. A second request whose `content` is only the `draft_task` id makes the final, reusing the draft's prompt, inputs, seed and settings; a few output parameters (resolution, watermark) can be set again. The draft task can be finished for seven days.
- **No seed, no idempotency key.** Seedance 2.5 takes no `seed` parameter; ModelArk reports the seed it used. Nothing makes a repeated submission the same task, so a submission whose answer was lost cannot simply be sent again.
- **Inputs by link.** Reference videos and images are read from a public link or from the account's asset library (`asset://<id>`), and Seedance refuses real faces in what it is sent, except for assets the account verified (real-person verification) or ModelArk's own preset characters.
- **Task records.** Tasks can be listed per model (one query a second) for seven days.
- **Billing.** In tokens, (input + output seconds) × width × height × frame rate ÷ 1024, at US$ 6.40 per million tokens up to 720p and US$ 7.00 at 1080p when the input has video, reported as `usage.completion_tokens`.

## Decision

1. **A BytePlus provider and take generator.** `crates/ai/src/byteplus.rs` implements the existing take generator for one model, Seedance 2.5 Video Edit (Mesh and Depth, 4 to 30 s), with a 480p draft and a 1080p final that finishes it. The take model says so (`finishes_draft`, `draft_lifetime`), and the app sends a final as only the accepted draft's task.
2. **One key, five fields.** BytePlus has one card in Settings › API keys with the ModelArk API key, a TOS access key ID and secret, the bucket and the region (`ap-southeast-1` by default). They are saved as one compound Credential Manager entry, so the settings file, key state and log masking treat them like any other key. Testing asks ModelArk for a task id that cannot exist and writes, reads and deletes a small file under `bardo/takes/`, the only prefix the guide's access policy allows.
3. **Staging in the user's bucket.** The control video and the look go to the user's own TOS bucket under random names in `bardo/takes/`, uploaded with 15-minute `TOS4-HMAC-SHA256` presigned links and read by ModelArk through 7-day ones (TOS's longest), which cover the draft's deadline. Stored files keep only their plain address; the read links are signed when a request is sent, so no signature or access key id is saved. A `take_staging` table (migration 0057) records each file with its job and when it is due for deletion. Files are released when a final is made (the draft's files), when a new draft replaces one, when a draft fails or is refused, and when the capture is removed, and expire at the latest after the draft's seven days. A sweep deletes due files at the start and end of every BytePlus take job and in a cleanup job queued when Bardo opens and after a capture is removed; a failed delete stays due. The guide's 8-day lifecycle rule on the prefix is the backstop for a file Bardo never got to record.
4. **The draft's deadline.** It is counted from when Bardo staged the draft's files (never later than ModelArk's own `created_at`) and kept on the result. Past it, a final is refused before anything is sent, the draft can no longer be approved and the take offers only a new draft. A draft makes one final: once its final is made, the next step is a new draft. Files staged but not sent within an hour are staged again, so a draft's deadline never outlives its links.
5. **A lost answer.** Bardo saves the time right before it submits. A retry with no saved task id lists the model's tasks since then (with two minutes of clock slack): one task of the same kind (and, for a final, of the same draft) that no other take or job knows is adopted and polled; none means the request is sent; several mean nothing is sent and the take says to look at ModelArk's task list.
6. **Look assets.** A persona's look can hold one asset per provider, a kind and an id, in its own table (migration 0057), tied to the look image's fingerprint so a new image clears it. Drawn looks take a preset character or a virtual portrait; a photo of the user takes only their verified likeness. A take on BytePlus whose persona has a photo look without a verified likeness is blocked before anything is sent. The asset never goes in a persona package or the settings file, and duplicating a persona does not copy it.
7. **Refusals and costs.** ModelArk's error code and message are shown with the provider's name, and the job's staged files are released. Costs use the existing output-token meter at the two rates above, the estimate counting the take's seconds in and out at the output's size and 24 fps, and the record uses the tokens ModelArk reports. Seeds ModelArk reports are saved on each result.

## Consequences

- The final keeps what the user approved in the draft, at about US$ 3.70 for a 30-second draft and US$ 20 for its final.
- Setting up BytePlus takes a verified account, US$ 30 of credits or a plan, a bucket with a lifecycle rule and a scoped access key; the guide's setup steps walk through each.
- The access key never leaves the computer: it only signs links. The bucket holds a take's files for at most the draft's seven days, plus the lifecycle backstop.
- Whether ModelArk's draft mode works with the video edit task type, the exact refusal codes and the token minimum for short videos are not confirmed against a live account; they are the first BytePlus checks of the regression pass. If ModelArk refuses draft mode for edits, a follow-up makes this model final only.
