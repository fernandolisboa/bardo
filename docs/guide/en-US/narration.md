---
id: narration
title: Narration
group: production
place: videos/narration
tour: narration
---

# Narration

The narration is the script read aloud by the project's narrator, with the time of every word, so scenes, captions and cuts line up with it. It lives at the [Narration](bardo:go/videos/narration) stage of a project. [Show me the stage](bardo:tour/narration).

<a id="generate"></a>
## Generating it

**Generate narration** has ElevenLabs read the current script with the narrator's voice and generation presets ([Personas](personas.md#presets)). ElevenLabs bills by character; the ⓘ next to the button says how many characters this script has, and the estimate under it what that costs. For an ad, a line above the estimate says when the script states things no claim of the brief backs, or was not checked as it reads now ([The claims check](script.md#claims)); it is a reminder and holds nothing back. It runs as a job; you need an ElevenLabs key ([Your API keys](api-keys.md#providers)).

The button waits until there is a script and a narrator whose voice is in your ElevenLabs account. If the narrator's voice was never checked or is missing, the stage says so; fix it on [Personas](personas.md#voice) or pick another [narrator](projects.md#narrator).

<a id="play"></a>
## Playing it

**Play** plays the narration and highlights each word as it is spoken. Click a word to play from there. **Details** says which voice and model read it, what it cost, its length and when it was made.

<a id="stale"></a>
## Out of date

When the script is saved or replaced after the narration was made, the narration is marked **Out of date**: the words no longer match. **Generate again**, or import a new recording, so they do. Scenes planned on an older narration show as out of date in turn ([Scenes](scenes.md#fit)).

<a id="replace"></a>
## Making it again

A new narration, whether generated, imported or from a capture, replaces the current one. The scenes are timed on the narration, so when they already hold images or clips Bardo asks first and says how many there are. If the script did not change since the scenes were planned, **Fit to the narration** on the Scenes stage keeps all of them afterwards ([Scenes](scenes.md#fit)); if it changed, the scenes have to be planned again and those images and clips go ([Scenes](scenes.md#replan)). The question says which of the two it is.

While scenes, images or clips are being made for the project, a new narration waits for them: let them finish, or cancel them on the [Jobs panel](jobs.md).

<a id="import"></a>
## Importing a recording

Recorded the script yourself? **Import recording** takes an MP3 or uncompressed WAV file of up to 500 MB. Bardo shows its name and length and what timing it costs, and **Use this recording** has ElevenLabs time its words against the current script, billed per hour of audio. It replaces the current narration once its words are timed. Read the script as written, so the timing matches.

<a id="capture"></a>
## Importing a capture

Recorded yourself reading the script on camera? **Import capture** takes the video: its speech becomes the narration, in the persona's voice or your own, and the recording is split into takes at the [Capture](bardo:go/videos/capture) stage. See [Performance capture](capture.md).
