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

Each take has a mode: what its take model receives. **Recording** sends the take as recorded. **Mesh** covers your face with its drawn mesh. **Depth** shows you as a depth figure over the set, and also needs the take's depth and person mask. New takes start as Recording; pick another under the take's words.

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

Under the take, **Play** plays the control video with its sound, and a line says what leaves the computer with it: your face, body and room (Recording); your body and room, not your face (Mesh); or your body shape and face geometry as a depth figure over your room or over the set image (Depth). The take's voice always goes with it. Unless the take is in Depth over an image, one frame of the take also goes to Claude to describe your room when the take is generated.

**Approve** says this take may go to its take model; nothing is paid for a take before you approve it. An approval holds for what you watched: changing the take's mode, set or cut, or its analysis running again, asks you to watch and approve the new build. A take with more than one face, a Mesh take with frames left uncovered, or one no take model accepts at its length, cannot be approved.

<a id="set"></a>
## The set

In Depth mode, **Set** picks where you appear: **Your room**, as recorded; a **Scene image** of the project; or **Import image**, a PNG or JPEG of yours. The image is copied into the project folder, so the take keeps it whatever happens to the scene or the file. Over an image, the recording's picture is never decoded, and no pixel of it leaves the computer.

<a id="cuts"></a>
## Moving a cut

**Cut earlier** and **Cut later** move the cut between a take and the next one to the nearest sentence end or pause before or after it that keeps both takes 4 to 30 seconds long. Both takes change, and work made from their earlier stretch will show as out of date.

<a id="stale"></a>
## Out of date

When the script changes after the capture, its narration is out of date and so is the capture: record it again with the current script and import it.

<a id="remove"></a>
## Removing it

**Remove capture** takes the recording and its takes out of the project. The narration made from it stays. Bardo asks once first; getting the takes back means importing the recording again.
