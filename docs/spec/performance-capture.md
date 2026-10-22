# Performance capture spec

- Status: Proposed (owner grill opened 2026-10-22; the defaults below stand where the owner did not answer)
- Date: 2026-10-22
- Parent issue: #186 (a sub-issue of the PRD, #4)
- Builds on: personas and cloned voices (ADR-0005), imported narration, video clips (ADR-0017), the job queue, costs and budgets

## Goal

The creator of a faceless channel acts a take once, on their own recording, and one of their personas performs it on screen: the persona's look, the creator's motion, face and timing, the persona's voice. The result is a clip that goes on the cut like any other, and every paid step sits behind a review.

## Principles

- **Own performances only.** A performance is a recording of the user themselves, or one they hold the rights to with the consent of everyone in it. Importing one asks the user to confirm that, every time, and the confirmation is kept with the performance. Performances come from local files only: no download from a link, no library of template videos, no way to put a persona or the user into someone else's video.
- **Motion, not identity.** What Bardo sends the provider for a take carries how the performer moves and speaks. Who appears in the result comes only from the persona's look.
- **Provider policies are product requirements.** Every input goes to the provider exactly as Bardo made it for the take. Bardo never alters a look, a control video or any other input to get past a refusal, never resends a refused request in another mode on its own, and never suggests a mode as a way past a refusal. A refusal ends that try and shows the provider's reason.
- **The take's voice is the only sound.** Background sound in a recording is removed and never restored; music comes from the music step (generated or imported). Bardo does not pitch-shift or otherwise disguise voices or music.
- **Free before paid, draft before final.** Everything that runs on the computer (analysis, control video) is free and reviewed before the first paid call. A cheap draft comes before the final, each paid call shows its estimate first and goes through the provider's budget.
- **Disclosure.** A project that uses a performed clip of a realistic look carries the synthetic-content disclosure, as one narrated with a realistic voice does today.

## Vocabulary

| Term | Meaning |
| --- | --- |
| **Performance** | A recording the user made of themselves performing the project's script (or part of it), imported into a video project with the user's confirmation that it is theirs to use. One per project in the first version. |
| **Take** | A stretch of a performance, 4 to 30 seconds, that starts and ends on sentence boundaries (a sentence over 30 seconds is cut at its longest pause). One take makes one performed clip. |
| **Persona look** | The image of the character a persona appears as: drawn by the image generator from a look prompt, or a photo of the user. Marked realistic or stylized. |
| **Face mesh** | Per frame of a take, the face's 478 three-dimensional landmarks and 52 expression (blendshape) scores, from MediaPipe Face Landmarker. Saved per take as `face_landmarks.json`. |
| **Depth map** | Per frame, how far each pixel is from the camera, from Depth Anything. |
| **Person mask** | Per frame, which pixels belong to the performer. |
| **Control video** | What the provider receives for a take, in one of three modes: *Recording*, *Mesh* or *Depth* (below), with the take's voice as its sound. |
| **Set** | Where the persona appears in Depth mode: the recording's own background, or a set image (a project scene image, an imported image or one drawn from a set prompt). |
| **Performed clip** | The provider's result for a take: the persona performing it. First a *draft* at the model's cheap tier, then a *final* at 1080p. Its sound is the take's voice; the provider's audio is dropped. |

## The flow

1. **Import.** On the project's Narration stage the user picks *Perform it*, chooses the recording (MP4 or MOV) and confirms it is theirs to use. Bardo reads it with ffmpeg and refuses what it cannot play.
2. **Voice.** The recording's speech becomes the project's narration: by default converted into the persona's voice with the ElevenLabs Voice Changer (background noise removed in the same call), which keeps the performer's timing, so lips and words stay in sync; or kept in the user's own voice, cleaned on the computer. Word timings come from aligning it with the script, as for imported narration today.
3. **Takes.** Bardo splits the performance into takes of 4 to 30 seconds at sentence ends, from the word timings. The user can move a boundary to another sentence end.
4. **Analysis (local, free).** For each take, run on the computer: the face mesh, and the depth map and person mask when the take uses Depth mode. Checks flag frames with no face, frames with more than one face (refused in the first version), a face too small or turned away, and sudden jumps in the mask.
5. **Control video (local, free).** The user picks the mode the model needs and, for Depth, the set. Bardo builds the control video with the take's voice inside it.
6. **First review (free).** The user watches the control video with its checks and approves it. Nothing paid happens before that; a change to the take, mode, set or voice asks again.
7. **Prompt.** Claude writes the prompt from a new template: the look, the set, the words of the take and the instruction the model needs. The user edits it.
8. **Draft (paid).** The provider makes the take at the model's draft tier.
9. **Second review.** The user watches the draft with the voice: regenerate (a new draft) or approve.
10. **Final (paid).** The provider makes it at 1080p. Where the model can finish the approved draft itself (Seedance 2.5 on BytePlus ModelArk) the final is the same video; elsewhere it is a new take, so the user reviews it again before it is used.
11. **On the cut.** A performed clip covers its take's stretch of narration on the video track, so the persona speaks when the voice does. The cut can go back to scene pictures between takes.

## Control video modes

| Mode | What leaves the computer | Built from | For |
| --- | --- | --- | --- |
| **Recording** | The take as recorded (face, body, room), with the take's voice | ffmpeg only | Motion-control models made to take a real person's motion (Kling 3.0 Motion Control). |
| **Mesh** (fast) | The take with the performer's face covered by its drawn mesh; body and room as recorded | Face mesh | Video-edit models (Seedance 2.5), one person facing the camera. No depth or mask, so it is quick. |
| **Depth** | The performer as a colored depth figure with the face mesh drawn on it, over the set | Depth map, person mask, face mesh | Video-edit models. Nothing of the performer's appearance or room leaves the computer, and the model follows the persona's look instead of copying the performer's hair, clothes and build. |

Each model lists the modes it takes; the default is its first. Mesh and Depth exist so the performer's face (and, in Depth, everything about them) stays on the computer and the model takes the persona's look rather than the performer's; Bardo does not offer them as a way past a refusal.

## Providers

A new domain port, the **performance generator**, sits beside the clip generator: its models say which modes they take, their draft and final tiers, whether a final finishes the draft, and the longest take. It stages the inputs, submits, polls, downloads and quotes, and the job saves the provider's ids before waiting, as clips do.

- **First: Higgsfield**, on the key and presigned uploads Bardo already uses (uploads take `video/mp4`, `audio/wav` and images, so no other storage is needed):
  - Seedance 2.5 Video Edit (`bytedance/seedance-2.5/video-edit`) for Mesh and Depth: control video as `video_url`, the look in `image_urls`, the voice in `audio_urls`, its own sound off; draft at 480p, final at 1080p.
  - Kling 3.0 Motion Control for Recording: the look as `image_url`, the take as `video_url`, `character_orientation: video` (up to 30 s), the reference's sound off; Standard (`kling-video/v3/motion-control/std`) for drafts, Pro (`…/pro`) for finals, which the slice checks against Higgsfield's estimate.
  - Higgsfield exposes no draft task or seed for these, so its final is a new take, reviewed again.
- **Later: Seedance 2.5 straight from BytePlus ModelArk** (`dreamina-seedance-2-5-260628`), for its draft mode: `draft: true` makes a 480p draft, and a second request naming the draft task makes the same video at 1080p, within seven days. ModelArk reads reference videos only from a public URL or its asset library, so Bardo stages the control video in the user's own BytePlus object storage bucket under a presigned link (at most seven days) and deletes it once the final is made or the draft expires. Seedance refuses references showing real human faces; a look it refuses can be given as an `asset://` id from the user's ModelArk asset library (its preset digital characters, a private virtual portrait, or the user's own verified likeness), never as an altered image.
- **Not used:** temporary public file hosts (they publish the user's recording); preprocessing in the cloud (local analysis is free, works offline and keeps the face on the computer); Higgsfield's Genjutsu workflows (their processing is undocumented, and they are built around swapping people into other videos).

Prices change; each model is priced from the provider's quote when it gives one, else from the rate table. For scale, Kling lists Kling 3.0 Motion Control at US$ 0.126 a second at 720p and US$ 0.168 at 1080p (checked 2026-10-22), so a 30-second final costs about US$ 5.

## Local analysis

- A new crate, `vision`, depending only on `domain`, holds adapters behind three ports (face tracker, depth estimator, person segmenter) that run ONNX Runtime through the `ort` crate: DirectML on Windows graphics cards, CPU elsewhere.
- Models, all Apache-2.0: MediaPipe Face Landmarker (BlazeFace short-range detector, the 478-point face mesh, the blendshape model) converted to ONNX; Depth Anything V2 Small (Video Depth Anything Small if per-frame depth flickers); MediaPipe's multiclass selfie segmenter for the person mask, since a take has one performer near the camera (a SAM-class video segmenter only if reviews show masks failing). Their sources, SHA-256 and licenses are pinned in a manifest like `crates/media/ffmpeg.toml`, fetched by an xtask, mirrored as release assets and bundled with the installer. A spike confirms the choice and measures the speed first.
- `face_landmarks.json`, per take: the format version; the source (file, width, height, frame rate, frame count); the model (name, version, SHA-256); and per frame its time in milliseconds and each face found, with 478 `[x, y, z]` landmarks (x and y from 0 to 1 across the frame) and the 52 blendshape scores by name. Other features can read it later, such as keeping the face in frame when a 16:9 take is cropped to 9:16.
- Analysis is a job per take with progress, cancel and resume; its outputs live in the project folder.

## Persona look

A persona gains an optional look: an image kept in the profile's data folder, its prompt, where it came from (drawn or imported) and whether it is realistic. A drawn look comes from the image generator with a look template that never names real people. An imported look must be the user's own likeness or a character they own, and importing asks for that confirmation. The persona package carries the look image (never provider asset ids, which belong to one account). A persona with no look cannot perform.

## Not in this module

- Putting the user, a persona or anyone into videos of other people, celebrities or films; template libraries; importing from links.
- Pitch-shifting or otherwise disguising voices or music; restoring a soundtrack over a result.
- Changing any input to get it past a provider's likeness or copyright filter.
- More than one performer in a take, recording from the webcam inside Bardo, and a live preview of the persona (possible later).

## Acceptance

- From a recording of themselves reading the script, the user reaches a project whose narration is their performance in the persona's voice and whose cut shows the persona performing each take.
- No paid call happens before the take's control video was approved, and no final before its draft was approved.
- In Depth mode nothing of the performer's appearance leaves the computer; in Mesh mode their face does not.
- Every analyzed take has a `face_landmarks.json` in the format above.
- A provider's refusal shows its reason and nothing is resent in another mode.
- Every paid call is recorded in Costs and passes the budget check.
- A project using a performed clip of a realistic look shows the disclosure in the upload review and export.

## Slices

In order (each is a sub-issue of #186):

1. #187 Spike: face mesh, depth and person mask from Rust
2. #188 Persona look
3. #189 Performances: import, takes and voice
4. #190 Face mesh, depth and person mask on each take (after 1)
5. #191 Control video and its review (after 3 and 4)
6. #192 Performed clips through Higgsfield: draft, review, final (after 2 and 5)
7. #193 Performed clips on the cut (after 6)
8. #194 Seedance 2.5 on BytePlus ModelArk with draft mode (after 6)
