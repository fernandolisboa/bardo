# ADR-0023: Localized versions carry the scenes and the cut, never the narration

- Status: Accepted
- Date: 2026-10-22
- Amends: the production stages of the MVP spec (Script, Scenes)

## Context

Channels start in the United States and then Brazil, and a video made for one audience often suits another. Images and clips are the expensive part of a video and do not depend on its language, but a second-language version used to mean every step again. YouTube's monetization policies treat reused and mass-produced content as inauthentic, so a version must be its own video for its own audience: its own script, narration, captions, metadata and edit, made one at a time at the user's request.

## Decision

1. **A version lives in another channel.** It is a new video project in a channel of the profile whose language or country differs from the source channel's; the channel brings the language, country and narrator. One version per channel; the same channel is never offered. The version keeps the source's video format.
2. **One job, then everything at once.** Making a version is one job: Claude adapts the source narration's sentences (the ones scenes start at) with the new **Localization** template and answers with a title and exactly one sentence per source sentence (a JSON schema); any other count fails the job as an unusable answer. Only then are the project (an approved theme with the adapted title), the script, the copies and the carried cut saved in one transaction; a failed or cancelled job leaves no project or file.
3. **Copies, not links.** The scene plan (current images and clips, prompts, video model picks) and the project's media and chosen generated tracks are copied with new ids and new generation records, and their files copied into the version's folder under the same names. Narration, captions, metadata, thumbnail, chapters, renders, exports, publications, cut suggestions and retention are not.
4. **What the cut did is snapshotted.** When the job runs, the source's cut as the editor would open it is reduced to what survives a new narration: per scene the framing, motion and transition of its first piece (or that it was left out), the captions' style, highlight and visibility, the mix and the frame shape, and each music and SFX item in source narration time. Splits, trims, footage placement and edited caption text are not carried.
5. **Sentences are found by words.** Each adaptation stores where its sentences start in its text. Placing the scenes matches the adaptation's words with the narration's in order (longest common run of words, with a proportional fallback for very long texts), so a script edited before narrating still places: a sentence whose first words are gone starts at the next shared word.
6. **Placing is a user action.** Once the version has a narration and still has the carried plan, the Scenes stage offers **Place scenes on this narration**: scenes move to their sentences' new starts, keep their pictures, take the new words, and the cut is built from the snapshot, music and SFX mapped through the sentence starts. It is offered again after a new narration; planning the scenes again replaces the carried plan. Scenes past their clip, scenes that changed length by more than ×1.5 or less than ×0.67, and scenes no longer narrated are pointed out.
7. **Costs.** Only the adaptation is billed, under Script, on the new project and its channel. Copies bill nothing.

## Consequences

- Migration 0050 adds the version's source and carried cut (`localization` and its sentence, scene, lane and audio tables), the sentence starts of each adaptation (`adapted_sentence`) and the `localization` template kind. Removing the source keeps the version and clears the link.
- A version takes as much disk as the source's images, clips and media; removing either project never touches the other's files.
- **Generate again** on a version adapts the stored source sentences again rather than writing a new script; the new adaptation waits for review like any script.
- Versions of versions are allowed, never back to the channel the version came from.
