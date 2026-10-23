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

<a id="cuts"></a>
## Moving a cut

**Cut earlier** and **Cut later** move the cut between a take and the next one to the nearest sentence end or pause before or after it that keeps both takes 4 to 30 seconds long. Both takes change, and work made from their earlier stretch will show as out of date.

<a id="stale"></a>
## Out of date

When the script changes after the capture, its narration is out of date and so is the capture: record it again with the current script and import it.

<a id="remove"></a>
## Removing it

**Remove capture** takes the recording and its takes out of the project. The narration made from it stays. Bardo asks once first; getting the takes back means importing the recording again.
