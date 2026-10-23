---
id: capture
title: Performance capture
group: production
place: projects/capture
tour: capture
---

# Performance capture

A capture is a recording of you reading the project's script on camera. Its speech becomes the narration, in the persona's voice or your own, and the recording is split into takes that later become performed clips. It lives at the [Capture](bardo:go/projects/capture) stage, which shows while the narration is the capture's. [Show me the stage](bardo:tour/capture).

<a id="record"></a>
## Recording it

Record yourself reading the current script as written, alone in the picture, with your face lit and the microphone close. Bardo imports MP4 or MOV files of 4 seconds to 15 minutes, at 24 to 60 frames per second and 720p or more; a phone held upright or a webcam is enough. Pause briefly between sentences: takes are cut at sentence ends and pauses.

Only record yourself. Bardo imports a capture only after you confirm the recording shows only you, and it never imports from a link.

<a id="import"></a>
## Importing it

On the [Narration](bardo:go/projects/narration) stage, **Import capture** opens a file picker. Bardo reads the file first and shows its size, frame rate and length, or why it cannot be a capture. Then pick the voice, tick **This recording shows only me.** and check the estimate: the script check, the voice and the timing of the words are each billed per hour of sound by ElevenLabs. **Import this capture** runs as a job; you need an ElevenLabs key ([Your API keys](api-keys.md#providers)).

The capture replaces the current narration once it is done. Generating or importing another narration later removes the capture.

<a id="voice"></a>
## The voice

**The persona's voice** converts your speech into the narrator's voice with its voice settings (ElevenLabs Voice Changer), keeping your timing so the words stay on your lips. The narrator's voice must be in your ElevenLabs account ([Personas](personas.md#voice)). A realistic voice is labelled as synthetic content when you post ([Exporting](exporting.md)).

**My own voice** keeps your voice, cleaned of music and room sound (ElevenLabs Voice Isolator).

Only the recording's sound is sent to ElevenLabs; the video stays in the project folder.

<a id="check"></a>
## Checking it follows the script

Before the voice is paid for, ElevenLabs transcribes the sound and Bardo checks it says at least 80% of the script in order, and that at least 80% of what it says is the script. A recording that does not stops, saying it does not follow the script closely enough, having paid only for the transcription. Record it again reading the current script and import the new file.

<a id="takes"></a>
## Takes

The capture is split into takes of 4 to 30 seconds. Takes start and end at sentence ends, or at pauses of 300 ms or more inside a long sentence, and sentences are packed together up to 30 seconds. Each take lists its words, where it starts and ends in the recording, and its length.

A take that cannot become a performed clip says why: **Over 30 seconds** with no pause to cut at (record that line again with a pause), or **Under 4 seconds** with no neighbour to join. The stage asks for attention until none is left.

<a id="mode"></a>
## A take's mode

Each take has a mode: what its take model receives. **Recording** sends the take as recorded. **Mesh** covers your face with its drawn mesh. **Depth** shows you as a depth figure over the set, and also needs the take's depth and person mask. New takes start in the first mode their take model takes ([Performed clip](#performed)): Recording for the default model. Pick another under the take's words; a mode the take's model does not take is greyed out.

<a id="analysis"></a>
## Analysis

Bardo analyzes each take on this computer, free, and nothing leaves it. As soon as the takes exist, it finds your face in every frame and keeps its mesh: 478 points and 52 expression scores per frame, in the take's `face_landmarks.json` in the project folder. Switching a take to **Depth** also works out its depth and person mask, over the whole take at once so the depth does not pump from frame to frame; switching back stops them.

Each part is a job per take, shown under the take with its progress, where it runs and about how long is left. It runs on the graphics card when it can; on a computer without a usable one, the processor runs it, which takes longer and still works. Cancelling one keeps what it finished: **Resume** picks up at the last finished stretch of frames. Moving a cut analyzes both takes again.

<a id="checks"></a>
## The checks

When a take's analysis is done, it says what it found, in frames:

- **More than one face**: only you may be in the recording. The take cannot go on; record it again alone.
- **No face**: frames where your face is out of the picture or hidden.
- **Face too small**: under 15% of the frame's height. Move closer to the camera.
- **Head turned**: more than 45° away from the camera.
- **Mask jumps** (Depth mode): the person mask changing by more than 20% from one frame to the next.

<a id="control"></a>
## The control video

Each take has a control video: exactly what its take model will receive, built on this computer, free, as soon as the take's analysis is done. It is the take in its mode, with its stretch of the narration as its sound, as an MP4 that fits every take model: at most 1920 × 1080 (or 1080 × 1920 upright), never larger than the recording, and well under 100 MB. Recording keeps the recording's frame rate; Mesh and Depth are drawn on every analyzed frame.

- **Recording** is the take's stretch of the recording as it is.
- **Mesh** covers your face, frame by frame, with an oval filled in a dark slate and the mesh drawn over it: lips, eyes and eyebrows each in their own color. A frame where no face was found borrows the faces of the nearest frames within half a second, grown wider the farther they are, in case your head moved. A longer gap would show your face, so the review counts those frames and the take cannot be approved until you record it again or switch it to Depth.
- **Depth** draws you as a figure colored by depth, near warm and far cool, inside your person mask, with the mesh lines over your face, over the [set](#set).

Changing the take's mode or set, or moving a cut, builds it again. **Build again** starts one that failed or was stopped.

<a id="review"></a>
## Reviewing and approving

Under the take, **Play** plays the control video with its sound, and a line says what leaves the computer with it: your face, body and room (Recording); your body and room, not your face (Mesh); or your body shape and face geometry as a depth figure over your room or over the set image (Depth). The take's voice always goes with it. Unless the take is in Depth over an image, one frame of the take also goes to Claude to describe your room when its [prompt](#take-prompt) is written.

**Approve** says this take may go to its take model; nothing is paid for a take before you approve it. An approval holds for what you watched: changing the take's mode, set or cut, or its analysis running again, asks you to watch and approve the new build. A take with more than one face, a Mesh take with frames left uncovered, or one no take model accepts at its length, cannot be approved.

<a id="set"></a>
## The set

In Depth mode, **Set** picks where you appear: **Your room**, as recorded; a **Scene image** of the project; or **Import image**, a PNG or JPEG of yours. The image is copied into the project folder, so the take keeps it whatever happens to the scene or the file. Over an image, the recording's picture is never decoded, and no pixel of it leaves the computer.

<a id="performed"></a>
## Performed clip

Once a take's control video is approved, its take model makes the persona perform it: the persona's [look](personas.md#look) moves, speaks and turns as you do in the take. Takes go to the channel's take model ([Channels](channels.md#defaults)); **Take model** under the take picks another for that take alone. Both run on Higgsfield with your Higgsfield key ([Your API keys](api-keys.md#providers)):

- **Kling 3.0 Motion Control**, the default, takes Recording takes of 3 to 30 seconds and makes the final straight away, at 1080p.
- **Seedance 2.5 Video Edit** takes Mesh and Depth takes of 4 to 30 seconds: a draft at 480p first, then the final at 1080p.

Picking a model that does not take the take's mode switches the take to the model's first mode, which builds its control video again for you to approve. Nothing can be made while the take is too short or too long for its model, the persona has no look, or the control video is not approved; the take says which.

<a id="take-prompt"></a>
## The take's prompt

**Write prompt** has Claude write the take's prompt from the **Take prompt** template ([Templates](templates.md#kinds)): the look's description (a photo has none), the set, the take's words (for the delivery, never quoted) and what the model does with the control video. Unless the take is in Depth over an image, one frame from the middle of the control video goes with it, so Claude describes your room without anyone in it. Edit the prompt as you like, up to 2,500 characters; **Write again** replaces it, edits included. A prompt holds for the control video and model it was written for: approving a new build or picking another model asks for a new one.

<a id="draft-final"></a>
## Draft and final

**Make draft** and **Make final** send the control video, the look and the prompt to Higgsfield; the estimate under them is per second of the take. The look always goes as a JPEG Bardo makes from it, which leaves out what a photo records about where and with what it was taken. Higgsfield's sound is off: drafts and finals play with the take's voice, from the control video.

- A draft (Seedance) waits for review: **Play draft**, then **Approve draft**, or **Regenerate draft** for another one, which replaces it. Only an approved draft lets you **Make final**; **Back to drafts** takes the approval back.
- A final waits for review the same way: **Play final**, then **Use**. **Make it again** makes another, which replaces it. A final is billed whether you use it or not.

A line says how much longer or shorter than the take a result came out. **Stop waiting** stops Bardo waiting, not Higgsfield, which may still finish and bill it: **Check again** picks up the same request without paying twice, as Bardo does after a restart. When Higgsfield refuses the content, the take says so and nothing is changed or sent again on its own; change the prompt or the look, then make it again.

<a id="use"></a>
## Using a performed clip

**Use** copies the final into the project's media as a performed clip named after its take ("Performed take 1"), in the editor's **Media** bin, where you put it on the cut ([Timeline and tracks](editor.md#timeline)). Each **Use** adds one, and the take counts them. Performed clips stay when the capture is removed, and versions in other languages do not carry them. When the persona's look is realistic, a cut with one of its performed clips turns the [synthetic-content disclosure](uploading.md#disclosure) on.

<a id="cuts"></a>
## Moving a cut

**Cut earlier** and **Cut later** move the cut between a take and the next one to the nearest sentence end or pause before or after it that keeps both takes 4 to 30 seconds long. Both takes change, and work made from their earlier stretch will show as out of date.

<a id="stale"></a>
## Out of date

When the script changes after the capture, its narration is out of date and so is the capture: record it again with the current script and import it.

<a id="remove"></a>
## Removing it

**Remove capture** takes the recording and its takes out of the project, with their drafts and finals. The narration made from it, and the performed clips you used, stay. Bardo asks once first; getting the takes back means importing the recording again.
