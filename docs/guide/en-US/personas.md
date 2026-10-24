---
id: personas
title: Personas
group: production
place: personas
tour: personas
---

# Personas

A persona is a narrator: a voice from your ElevenLabs account, a tone, a script style and the settings ElevenLabs reads with. Personas belong to you, not to a channel: one can narrate for several channels, and each channel has a default ([Projects and stages](projects.md#narrator)). They live on [Personas](bardo:go/personas). [Show me the screen](bardo:tour/personas).

<a id="library"></a>
## Your narrators

The list holds every persona, with its voice. Bardo comes with four, two in English and two in Portuguese; **New persona** adds your own, and picking one opens it for editing. Saving a persona that some channels use as their default changes their narrator too, so Bardo names those channels and asks first. To change only one channel's narrator, **Duplicate** the persona and edit the copy; the copy keeps the look.

<a id="voice"></a>
## The voice

**Choose from my ElevenLabs voices** lists the voices in your ElevenLabs account, clones included; each has a free ElevenLabs preview. The persona keeps only a reference to the voice, never audio or keys. Under the chosen voice, Bardo says whether it is still in your account; narration with a missing voice would fail. You need an ElevenLabs key ([Your API keys](api-keys.md#providers)).

<a id="style"></a>
## Tone and script style

**Tone** says how the narrator sounds ("sober, measured, no hype"), and **Script style** how scripts are written for them: structure, sentence length, hooks. Claude reads both whenever it writes a script for a video this persona narrates.

<a id="presets"></a>
## Generation presets

The presets set how ElevenLabs reads with this voice:

- **Stability**: higher is steadier and more even; lower is more expressive.
- **Similarity**: how closely the narration sticks to the original voice.
- **Style exaggeration**: amplifies the voice's own style; 0 is off.
- **Speed**: 100% is the voice's normal pace, from 70% to 120%.

<a id="sample"></a>
## Hearing it first

**Hear it with these settings** reads a short sentence (yours to change, up to 250 characters) with the chosen voice and the presets as they are now, saved or not. Each new combination is a short paid ElevenLabs call, recorded in Costs; a combination you already heard plays again for free from this computer.

<a id="share"></a>
## Saving and sharing

**Create persona** or **Save changes** keeps it. **Export to file…** saves a package with the persona, a reference to its voice and its look when it is a drawing, never audio, keys or a photo of you, to use on another computer; **Import from file…** at the top brings one in. Whoever imports it needs the same voice in their own ElevenLabs account: until Bardo finds it there, the persona is marked **Voice not checked** or **Voice unavailable** and cannot narrate.

<a id="realistic"></a>
## Realistic voices

Tick **Realistic synthetic voice or clone of a real person** when the voice sounds like a real person: a clone, or a professional voice made from someone's recordings. Picking a cloned or professional voice ticks it for you. Networks ask you to label such videos as altered or synthetic content, and Bardo reminds you when you export.

<a id="look"></a>
## The look

The look is the character this persona appears as in performed clips. A persona with no look cannot perform. There are two kinds:

- **Drawn**: describe the character under **Describe the character** and choose **Draw**. Claude writes an image prompt from your description with the [look prompt template](templates.md), for an original adult character only: it declines a real, identifiable person, a child or a copyrighted character and says why, even if the template was edited. Nano Banana then draws it in a tall 2:3 frame. Both calls are estimated before you start and recorded in [Costs](costs.md). The drawing waits for review under **New drawing to review**: **Keep this look** makes it the look, **Discard** drops it, and **Draw again** replaces it with a new one. You need Claude and Gemini keys ([Your API keys](api-keys.md#providers)).
- **Photo of you**: **Import a photo of me…** takes a PNG or JPEG of you alone, from the waist up, facing the camera on a plain background, 300 to 6,000 px a side and at most 10 MB. Tick **This photo shows me, and only me** first. It becomes the look at once, and it never leaves in a package.

**Realistic look** is always ticked for a photo of you, and ticked for a photoreal drawing: videos where a realistic look performs need the networks' altered or synthetic content label. **Remove look** deletes it after asking; a drawn look costs a new drawing to get back. Changes in this section save at once, without **Save changes**. A persona imported from a package with a drawn look brings it as a drawing to review.

<a id="look-asset"></a>
## BytePlus asset library

Seedance 2.5 on BytePlus ModelArk refuses real faces in what it is sent, and can take a look from your ModelArk asset library by its id instead of the image ([Getting each API key](get-api-keys.md#byteplus) says how to add one). Under **BytePlus asset library**, pick the **Kind**, paste the **Asset id** (with or without `asset://`) and select **Save asset**:

- A **photo of you** takes only your **Verified likeness**, the one ModelArk verified as you. Until it has one, its takes cannot be sent to BytePlus.
- A **drawn look** can take a **Preset character** from ModelArk's library or a **Virtual portrait** you made there, or go as its image with no asset at all.

An id holds for this look only: a new drawing or photo clears it. **Remove asset** sends the image again. The id stays on this computer: it never leaves in a persona package or the settings file, and duplicating the persona does not copy it.
