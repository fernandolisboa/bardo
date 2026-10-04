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

The list holds every persona, with its voice. Bardo comes with four, two in English and two in Portuguese; **New persona** adds your own, and picking one opens it for editing. Saving a persona that some channels use as their default changes their narrator too, so Bardo names those channels and asks first. To change only one channel's narrator, **Duplicate** the persona and edit the copy.

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

**Create persona** or **Save changes** keeps it. **Export to file…** saves a package with the persona and a reference to its voice, never audio or keys, to use on another computer; **Import from file…** at the top brings one in. Whoever imports it needs the same voice in their own ElevenLabs account: until Bardo finds it there, the persona is marked **Voice not checked** or **Voice unavailable** and cannot narrate.

<a id="realistic"></a>
## Realistic voices

Tick **Realistic synthetic voice or clone of a real person** when the voice sounds like a real person: a clone, or a professional voice made from someone's recordings. Picking a cloned or professional voice ticks it for you. Networks ask you to label such videos as altered or synthetic content, and Bardo reminds you when you export.
