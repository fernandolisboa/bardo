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

Under the take, **Play** plays the control video with its sound, and a line says what leaves the computer with it: your face, body and room (Recording); your body and room, not your face (Mesh); or your body shape and face geometry as a depth figure over your room or over the set image (Depth). The take's voice always goes with it. Unless the take is in Depth over an image, one frame of the control video also goes to Claude to describe your room when its [prompt](#take-prompt) is written; in Recording, that frame shows you.

**Approve** says this take may go to its take model; nothing is paid for a take before you approve it. An approval holds for what you watched: changing the take's mode, set or cut, or its analysis running again, asks you to watch and approve the new build. A take with more than one face, a Mesh take with frames left uncovered, or one no take model accepts at its length, cannot be approved.

<a id="set"></a>
## The set

In Depth mode, **Set** picks where you appear: **Your room**, as recorded; a **Scene image** of the project; or **Import image**, a PNG or JPEG of yours. The image is copied into the project folder, so the take keeps it whatever happens to the scene or the file. Over an image, the recording's picture is never decoded, and no pixel of it leaves the computer.

<a id="performed"></a>
## Performed clip

Once a take's control video is approved, its take model makes the persona perform it: the persona's [look](personas.md#look) moves, speaks and turns as you do in the take. Takes go to the channel's take model ([Channels](channels.md#defaults)); **Take model** under the take picks another for that take alone. Each model is named with the provider it runs on, with your key there ([Your API keys](api-keys.md#providers)):

- **Kling 3.0 Motion Control · Higgsfield**, the default while the Higgsfield key is saved, takes Recording takes of 3 to 30 seconds and makes the final straight away, at 1080p.
- **Seedance 2.5 Video Edit · Higgsfield** takes Mesh and Depth takes of 4 to 30 seconds: a draft at 480p first, then the final at 1080p, made anew.
- **Seedance 2.5 Video Edit · BytePlus ModelArk** takes the same takes, straight from ModelArk: a draft at 480p, then a final that finishes that same draft at 1080p, so the final keeps what you approved in the draft. See [On BytePlus ModelArk](#byteplus).
- **Kling 3.0 Motion Control 720p · Kling** and **Kling 3.0 Motion Control 1080p · Kling** take Recording takes of 3 to 30 seconds straight from Kling, with the Kling key Bardo already keeps for clips, for less than through Higgsfield. An optional draft on Kling 2.6 at 720p costs about half of a 720p final. See [Straight from Kling](#kling).

Picking a model that does not take the take's mode switches the take to the model's first mode, which builds its control video again for you to approve. Nothing can be made while the take is too short or too long for its model, the persona has no look, or the control video is not approved; the take says which.

<a id="spend"></a>
## What the video's takes cost

**Performed takes of this video**, above the takes, adds up what the video's performed takes cost so far (prompts, the test, drafts and finals, as [Costs](costs.md) records them) and about what they still need: each take's prompt when it has none, a draft where its model requires one, and its final when it has none. It shows against the video's cap, US$ 10 unless you change it with **Change cap** (US$ 1 at least; **Use the default** goes back to US$ 10). When a step has no rate in the table, the line says the total is short.

Before any paid take (test, draft or final), Bardo checks that the projection plus that take stays within the cap; a take already counted in the projection is not added twice. Past the cap, the take waits and asks **Make it anyway?**: nothing is sent until you confirm. The cap is per video and does not replace the providers' monthly budgets ([Costs](costs.md)): when a take goes past both, one question lists the cap and each budget it goes past, and confirming it sends the take.

<a id="test"></a>
## The test take

Before the first draft or final of a capture, Bardo asks for a test: **Make test take** sends up to the first 5 seconds of a take that is ready (an approved control video and a prompt), the first one that passed its [preflight checks](#preflight) when there is one, at its model's cheapest tier (its draft tier where it has one). It shows what it costs before you start, is recorded in [Costs](costs.md) like any paid take, and plays with the take's voice. Then:

- **Approve test** lets the capture's drafts and finals be made.
- **Discard test** deletes it; **Make test take** again makes another, which replaces it.

A test holds for the persona's look it was made with: another look asks for a new one. A capture that already had a draft or final when tests came along needs none.

<a id="preflight"></a>
## Preflight checks

Under each take, **Preflight checks** read the take's analysis and the look, free, before anything is paid:

- **One person**: no frame with another face (one already blocks the control video).
- **In frame**: in no more than 10% of frames is the face missing, at an edge, or without room for the body below it.
- **Face large enough**: the face is at least 8% of the frame's height on average.
- **Steady motion**: in no more than 5% of frames does the face move faster than two face heights a second.
- **Same framing as the look**: a take framed from the waist up needs a look framed the same, and a full-body take a full-body look. Motion-control models follow the look's framing; video-edit models keep the take's, so they skip this check.

Bardo measures the look's face once per look image, in the background. A failed check says why and holds the take's test, draft and final until you record or cut it again, or select **Send anyway**, which holds for that take's approved control video. A check that cannot be read yet (a take analyzed before the check existed, or a look still being measured) says so and holds nothing; analyzing the take again runs it.

<a id="take-prompt"></a>
## The take's prompt

**Write prompt** has Claude write the take's prompt from the **Take prompt** template ([Templates](templates.md#kinds)): the look's description (a photo has none), the set, the take's words (for the delivery, never quoted) and what the model does with the control video. Unless the take is in Depth over an image, one frame from the middle of the control video goes with it, so Claude can describe your room (the prompt describes it without anyone in it). In Recording, that frame is of your recording and shows you; the take says so before you write the prompt. Edit the prompt as you like, up to 2,500 characters; **Write again** replaces it, edits included. A prompt holds for the control video and model it was written for: approving a new build or picking another model asks for a new one.

<a id="draft-final"></a>
## Draft and final

**Make draft** and **Make final** send the control video, the look and the prompt to the take model's provider; the estimate under them is per second of the take (in tokens on BytePlus). The look goes as a JPEG Bardo makes from it, which leaves out what a photo records about where and with what it was taken, unless it goes by its asset id on BytePlus. The provider's sound is off: drafts and finals play with the take's voice, from the control video.

- A draft waits for review: **Play draft**, then **Approve draft**, or **Regenerate draft** for another one, which replaces it. **Back to drafts** takes the approval back.
- Where the final is a new take (Seedance on Higgsfield, Kling straight from Kling), the draft is optional: **Make final without a draft** makes the final straight away, with its estimate beside it. On BytePlus ModelArk, whose final finishes the draft, only an approved draft lets you **Make final**.
- A final waits for review the same way: **Play final**, then **Use**. **Make it again** makes another, which replaces it. A final is billed whether you use it or not.

A line says how much longer or shorter than the take a result came out. **Stop waiting** stops Bardo waiting, not the provider, which may still finish and bill it: **Check again** picks up the same request, as Bardo does after a restart, and nothing new can be made for the take until then. When the provider refuses the content, or the request fails on its side, the take says so (with the provider's reason when it gives one) and nothing is changed or sent again on its own: making it again is a new paid request, and only you start it.

<a id="byteplus"></a>
## On BytePlus ModelArk

Seedance 2.5 straight from ModelArk needs the **BytePlus ModelArk** card's five fields ([Getting each API key](get-api-keys.md#byteplus)). What changes there:

- **The final finishes the draft.** **Make final** sends ModelArk only the approved draft's task, and ModelArk makes the final from the draft's own prompt, inputs and seed. Approving another draft, or regenerating it, makes that one the draft the final finishes.
- **A deadline.** ModelArk finishes a draft only within seven days, so the draft shows the date and time to make its final by. Past it, **Make final** is not offered and the take asks for a new draft; an approved draft past its deadline offers **Regenerate draft**.
- **Your bucket.** ModelArk reads the control video, and the look unless it goes by its asset id, only from a link. Bardo uploads them to your bucket under random names in `bardo/takes/`, with links that last seven days at most, and deletes them once they are no longer needed: when the final is made, when a new draft replaces the old one, when a draft fails or is refused, when the capture is removed, and at the latest when the draft's seven days are up. A delete that fails is tried again later; the bucket's lifecycle rule is the backstop.
- **A photo of you needs your verified likeness.** Seedance refuses real faces, so a take whose persona has a photo look is sent only once the look holds your verified likeness from ModelArk's asset library ([The look](personas.md#look-asset)).
- **Costs in tokens.** ModelArk bills the video's tokens: about US$ 3.70 for a 30-second draft and US$ 20 for its final. [Costs](costs.md) records the tokens ModelArk reports.
- **A lost answer is found again.** ModelArk has no way to tell a repeated request from a new one, so Bardo saves the time right before each send. If Bardo stops before ModelArk's answer arrives, the next attempt looks in ModelArk's task list for that request and waits for it instead of paying again. If it finds several, it sends nothing and asks you to look at the task list in ModelArk.

<a id="kling"></a>
## Straight from Kling

The Kling 3.0 Motion Control models straight from Kling use the **Kling** key ([Getting each API key](get-api-keys.md#kling)) and Kling's resource packages. What changes there:

- **Prices.** Kling bills the seconds of video it returns: about US$ 0.126 a second for a 720p final, US$ 0.168 at 1080p, and US$ 0.07 for a draft or test on Kling 2.6 at 720p. A take can come back a little shorter than it was sent, and [Costs](costs.md) records what Kling returned.
- **Your storage holds the control video.** Kling takes the look inside the request but reads the control video only from a link, and keeps no files. Bardo stages it in storage you already set up: your BytePlus bucket when the **BytePlus ModelArk** key is saved (with a signed link that lasts seven days at most, deleted once the take is made), otherwise Higgsfield's upload, which is free. With neither key, the take says what to add.
- **A lost answer is found again.** Each request carries an id of its own; if Bardo stops before Kling's answer arrives, the next attempt finds that request instead of paying again.

<a id="use"></a>
## Using a performed clip

**Use** copies the final into the project's media as a performed clip named after its take ("Performed clip 1"), in the editor's **Media** tab, where **Put on the cut** places it in sync with the narration of its take ([Performed clips](editor.md#performed)). Each **Use** adds one, and the take counts them. Performed clips stay when the capture is removed, and versions in other languages do not carry them. When the persona's look is realistic, a cut with one of its performed clips turns the [synthetic-content disclosure](uploading.md#disclosure) on.

<a id="cuts"></a>
## Moving a cut

**Cut earlier** and **Cut later** move the cut between a take and the next one to the nearest sentence end or pause before or after it that keeps both takes 4 to 30 seconds long. Both takes change, and work made from their earlier stretch will show as out of date.

<a id="stale"></a>
## Out of date

When the script changes after the capture, its narration is out of date and so is the capture: record it again with the current script and import it.

<a id="remove"></a>
## Removing it

**Remove capture** takes the recording and its takes out of the project, with their drafts and finals. The narration made from it, and the performed clips you used, stay. Bardo asks once first; getting the takes back means importing the recording again.
