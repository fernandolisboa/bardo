# Performance capture spec

- Status: Proposed. Defaults chosen on 2026-10-22; each slice may revisit its own before it starts.
- Date: 2026-10-22
- Parent issue: #186 (a sub-issue of the PRD, #4)
- Builds on: personas and cloned voices (ADR-0005), imported narration, video clips (ADR-0017), the job queue, costs and budgets

## Goal

The creator of a faceless channel reads the script once on camera, and one of their personas performs it on screen: the persona's look; the creator's motion, face and timing; the persona's voice. The result is a set of clips that go on the cut like any other, and every paid result is reviewed before it is used.

## Principles

- **The user's own recordings only.** A capture is a recording of the user themselves, alone in frame. Each import asks the user to confirm that, and the confirmation is kept with the capture. The capture must follow the project's script (its speech has to align with it), and it comes from a local file. There is no import from links, no library of template videos, and no way to put a persona or the user into someone else's video.
- **Motion, not identity.** What Bardo sends the video model for a take carries how the performer moves and speaks. Who appears in the result comes only from the persona's look.
- **Provider policies are product requirements.** Every input goes to the provider exactly as Bardo made it for the take. Bardo never alters a look, a control video or any other input to get past a refusal. It never resends a refused request in another mode on its own, and never suggests a mode as a way past a refusal. A refusal ends that try and shows the provider's reason, or says that the provider refused the content when it gives none.
- **The take's voice is the only sound.** Any other sound in a recording, music included, is removed by voice isolation and never restored. The video's music comes from the music step (generated or imported). Bardo does not pitch-shift or otherwise disguise voices or music.
- **Free first; every paid result reviewed.** Local analysis and the control video are free, and no video-model call happens before the control video is approved. Every paid step shows its estimate first and passes the provider's budget: the voice change, a look drawing, drafts and finals. A cheap draft comes first where the model has a cheap tier, and no paid result goes on the cut before the user reviews it.
- **Disclosure.** The synthetic-content disclosure is on when the narration is a capture converted into a realistic persona voice, as for a narration read by one today. It is also on when the cut uses a performed clip of a realistic look.

## Vocabulary

Each slice adds its terms to `docs/CONTEXT.md` when it lands. They are named apart from **Performance**, which already names the metrics screen and past performance.

| Term | Meaning |
| --- | --- |
| **Capture** | A recording of the user reading the project's script on camera, alone in frame, imported into a video project with the user's confirmation. Its speech becomes the narration. One per project in the first version. |
| **Take** | A stretch of a capture, 4 to 30 seconds, that starts and ends on sentence boundaries (a sentence over 30 seconds is cut at its longest pause). One take makes one performed clip. |
| **Take model** | The video model that turns takes into performed clips, with the control video modes it takes. A channel has a default, and any take can pick another. |
| **Persona look** | The image of the character a persona appears as. It is drawn by the image generator as an original character, or it is a photo of the user. It is marked realistic or stylized. |
| **Face mesh** | Per frame of a take, the face's 478 three-dimensional landmarks and 52 expression (blendshape) scores from MediaPipe Face Landmarker. Saved per take as `face_landmarks.json`. |
| **Depth map** | Per frame, how far each pixel is from the camera, from Depth Anything. |
| **Person mask** | Per frame, which pixels belong to the performer. |
| **Control video** | What the take model receives for a take, in one of three modes (*Recording*, *Mesh* or *Depth*, below), with the take's voice as its sound. |
| **Set** | Where the persona appears in Depth mode: the recording's own background, or a set image (a project scene image or an imported image). |
| **Performed clip** | The take model's result for a take: the persona performing it. A *draft* at the model's cheap tier where it has one, then a *final* at 1080p. Its picture is used; its sound is the take's voice, and the provider's audio is dropped. |

## The flow

1. **Capture.** On the Narration stage the user picks *Perform it*, chooses the recording (MP4 or MOV) and confirms it shows only them. Bardo reads it with ffmpeg and refuses what it cannot play.
2. **Voice.** The recording's speech becomes the project's narration.
   - By default it is converted into the persona's voice with the ElevenLabs Voice Changer, which removes background sound in the same call. It keeps the performer's timing, so lips and words stay in sync.
   - The other choice keeps the user's own voice, cleaned by the ElevenLabs Voice Isolator so that music and room sound go.
   - Word timings come from aligning the speech with the script, as for imported narration. A capture that does not follow the script is refused.
3. **Takes.** Bardo splits the capture into takes of 4 to 30 seconds at sentence ends, from the word timings. The user can move a boundary to another sentence end.
4. **Model and mode.** Each take uses the channel's take model unless the user picks another. The mode is the model's first unless the user picks another the model takes.
5. **Analysis (local, free).** For each take, Bardo runs the face mesh on the computer. It also runs the depth map and person mask when the take's mode is Depth. Checks flag:
   - frames with no face;
   - frames with more than one face (the take cannot go on);
   - a face too small or turned away;
   - sudden jumps in the mask.
6. **Control video (local, free).** Bardo builds the control video for the take's mode (and, for Depth, its set), with the take's voice inside.
7. **First review (free).** The user watches the control video with its checks, reads what will leave the computer, and approves it. A change to the take, model, mode, set or voice asks again.
8. **Prompt.** Claude writes the prompt from a new template: the look, the set, the words of the take and the instruction the model needs. The user edits it.
9. **Draft (paid, where the model has a cheap tier).** The take model makes the take at its draft tier. The user watches it with the voice and either regenerates or approves.
10. **Final (paid).** The take model makes it at 1080p.
    - Where it can finish the approved draft from the draft's own parameters (Seedance 2.5 on BytePlus ModelArk), the final stays consistent with the draft. Elsewhere it is a new take.
    - Either way the user reviews it before it is used.
11. **On the cut.** A performed clip covers its take's stretch of narration on the video track, so the persona speaks when the voice does. The cut can go back to scene pictures between takes.

## Control video modes

| Mode | What leaves the computer | Built from | For |
| --- | --- | --- | --- |
| **Recording** | The take as recorded (face, body, room), with the take's voice | ffmpeg only | Motion-control models made to take a real person's motion (Kling 3.0 Motion Control). |
| **Mesh** (fast) | The take with the performer's face covered by its drawn mesh (facial geometry, not the face); body and room as recorded | Face mesh | Video-edit models (Seedance 2.5). One person facing the camera; no depth or mask, so it is quick. |
| **Depth** | The performer as a colored depth figure (build and outline) with the face mesh drawn on it, over the set. With a set image, nothing of the room; with the recording's background, the room | Depth map, person mask, face mesh | Video-edit models. The performer's looks stay on the computer, and the model follows the persona's look instead of copying the performer's hair and clothes. |

Seedance 2.5 refuses reference images and videos that show real human faces, because they could be used to show a real person without consent. Its mode list is therefore Mesh and Depth. Those control videos show no face, and the only identity in the result is the persona's look, which goes to the provider as is and passes its checks on its own.

This is why only a recording of the user alone, following the project's script, can be a capture. Once the face is covered, the provider can no longer tell whose motion it is. Mesh and Depth exist so the performer's face stays on the computer and the model takes the persona's look; Bardo never offers them as a way past a refusal.

When the set is described from the recording's background, one frame of the control video goes to Claude for that description. In Mesh and Depth mode that frame shows no face. The first review says so as well.

## Take models and providers

A new domain port, the **take generator**, sits beside the clip generator. Each of its models declares:

- the modes it takes;
- its draft tier, if any, and its final tier;
- whether a final finishes the draft;
- the take lengths it allows and its file limits.

It stages the inputs, submits, polls, downloads and quotes. The job saves the provider's ids before waiting, as clips do.

- **First: Higgsfield**, on the key and presigned uploads Bardo already uses. The uploads take `video/mp4`, `audio/wav` and images, so no other storage is needed.
  - **Kling 3.0 Motion Control** (`kling-video/v3/motion-control/std` and `…/pro`) takes Recording. The look goes as `image_url`, the take as `video_url`, with `character_orientation: video` (up to 30 s) and the reference's sound off. Kling's 720p tier costs three quarters of its 1080p one, so this model has no draft step. It makes the final at Pro (1080p), reviewed before use. It is the default take model because it costs least.
  - **Seedance 2.5 Video Edit** (`bytedance/seedance-2.5/video-edit`) takes Mesh and Depth. The control video goes as `video_url`, with the voice inside it, and the look in `image_urls`, with `generate_audio: false`. Drafts are 480p and finals 1080p. The slice checks with real keys whether also sending the voice in `audio_urls` improves lip sync.
  - Higgsfield exposes no draft task or seed for these, so its finals are new takes. A moderation refusal comes back with the status `nsfw` and no reason.
- **Later: Seedance 2.5 straight from BytePlus ModelArk** (`dreamina-seedance-2-5-260628`), for its draft mode.
  - `draft: true` makes a 480p draft. A second request naming the draft task makes the final at 1080p within seven days, reusing the draft's prompt, inputs, seed and settings. ModelArk lists draft mode for the model rather than per task type, so the slice first confirms with a real key that it works with the video edit.
  - ModelArk reads reference videos only from a public link or its asset library. So Bardo stages the control video in the user's own BytePlus object storage bucket under a presigned link (seven days at most), and deletes it once the final is made or the draft expires.
  - A look goes inline as an image. For BytePlus, the user can instead give an `asset://` id from their ModelArk asset library, chosen by where the look came from:
    - a drawn look: one of ModelArk's preset digital characters, or a private virtual portrait the user made there, which ModelArk requires not to resemble any real person;
    - the user's own photo: the user's likeness added through ModelArk's real-person verification, and nothing else.
  - The asset library needs ModelArk's Advanced Creation Rights and a verified account.
- **Not used:**
  - temporary public file hosts, since they would publish the user's recording;
  - preprocessing in the cloud, since local analysis is free, works offline and keeps the face on the computer;
  - Higgsfield's Genjutsu workflows, whose processing is undocumented and which overlap with Kling Motion Control.

Prices change. Each model is priced from the provider's quote when it gives one, else from the rate table. Two 30-second takes for scale, on the providers' own published prices (checked 2026-10-22):

- **Kling 3.0 Motion Control at 1080p:** US$ 0.168 a second, about US$ 5 for the final.
- **Seedance 2.5 Video Edit on ModelArk:** billed in tokens, (input + output seconds) × width × height × frame rate / 1024, at US$ 6.40 per million tokens up to 720p and US$ 7.00 at 1080p when the input has video. At 24 fps that is about US$ 3.70 for a 480p draft and US$ 20 for the 1080p final.

## Local analysis

- **Crate.** A new crate, `vision`, depending only on `domain`, holds adapters behind three ports: face tracker, depth estimator and person segmenter. They run ONNX Runtime through the `ort` crate, on DirectML with a Windows graphics card and on the CPU otherwise.
- **Models.** All are Apache-2.0:
  - MediaPipe Face Landmarker (BlazeFace short-range detector, the 478-point face mesh, the blendshape model), converted to ONNX;
  - Video Depth Anything Small at 280×504 (504×280 for vertical takes), which flickers five times less than Depth Anything V2 Small per frame; V2 Small smoothed over time where the 4.5 GB it needs on the CPU are not free;
  - MediaPipe's multiclass selfie segmenter for the person mask, since a take has one performer near the camera. A SAM-class video segmenter comes in only if reviews show masks failing.
- **Pinning.** Each model's source, SHA-256 and license are pinned in a manifest like `crates/media/ffmpeg.toml`. An xtask fetches them, they are mirrored as release assets, and the installer bundles them. The spike (#187: `docs/spikes/vision.md`, ADR-0024) confirmed the choice and measured speed: on a 4-core CPU, about 20 s of analysis per 30 s take for Mesh mode and 6.5 min for Depth mode.
- **Small faces.** A face as small as in a standing shot (13 % of the frame's height in the spike) is too small for the face detector in the whole frame; Bardo searches again around the person mask's face.
- **`face_landmarks.json`**, per take, holds:
  - the format version;
  - the source (file, width, height, frame rate, frame count);
  - the model (name, version, SHA-256);
  - per frame, its time in milliseconds and each face found, with 478 `[x, y, z]` landmarks (x and y from 0 to 1 across the frame) and the 52 blendshape scores by name.

  The cut reads it to keep the face in frame when a 16:9 take is cropped to 9:16, for models whose output keeps the source's framing.
- **Jobs.** Analysis is a job per take, with progress, cancel and resume. Its outputs live in the project folder.

## Persona look

A persona gains an optional look, kept in the profile's data folder: the image, its prompt, where it came from (drawn or imported) and whether it is realistic.

- **Drawn looks.** Claude turns the user's description into an image prompt for an original character, through a look template that declines descriptions of real, identifiable people. The image generator then draws it.
- **Imported looks.** An imported look must be the user's own likeness, and importing asks for that confirmation.
- **Packages.** The persona package carries a drawn look. It never carries an imported photo or a provider asset id: a photo is someone's likeness, and an asset id belongs to one account. Whoever imports the persona sets their own look.
- **No look, no performance.** A persona with no look cannot perform.

## Not in this module

- Putting the user, a persona or anyone into videos of other people, celebrities or films; recordings of anyone other than the user; template libraries; importing from links.
- Pitch-shifting or otherwise disguising voices or music; restoring a soundtrack over a result.
- Changing any input to get it past a provider's likeness or copyright filter.
- Carrying performed clips into a localized version of a project (#184). They are lip-synced to one language's narration, so a localized version shows scene pictures in their place.
- Later, possibly: more than one performer, recording from the webcam inside Bardo, a live preview of the persona.

## Acceptance

- From a recording of themselves reading the script, the user reaches a project whose narration is their capture in the persona's voice and whose cut shows the persona performing each take.
- No video-model call happens before the take's control video was approved, and no performed clip goes on the cut before the user reviewed it.
- In Mesh mode no frame shows the performer's face; in Depth mode with a set image, no pixel of the recording is in the control video.
- A recording whose speech does not follow the script, or a take with more than one face, cannot go on.
- Every analyzed take has a `face_landmarks.json` in the format above.
- A provider's refusal shows its reason (or that the content was refused) and nothing is resent in another mode.
- Every paid call is recorded in Costs and passes the budget check.
- The disclosure is on for a capture voiced by a realistic persona voice and for a cut with a performed clip of a realistic look.

## Slices

In order (each is a sub-issue of #186):

1. #187 Spike: face mesh, depth and person mask from Rust
2. #188 Persona look
3. #189 Captures: import, takes and voice
4. #190 Face mesh, depth and person mask on each take (after 1 and 3)
5. #191 Control video and its review (after 3 and 4)
6. #192 Performed clips through Higgsfield (after 2 and 5)
7. #193 Performed clips on the cut (after 6)
8. #194 Seedance 2.5 on BytePlus ModelArk with draft mode (after 6)
