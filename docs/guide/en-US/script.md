---
id: script
title: Script
group: production
place: projects/script
tour: script
---

# Script

The script is what the narrator will say. Claude writes it, you edit it, and every later stage builds on it. It lives at the [Script](bardo:go/projects/script) stage of a project, with the video's music below it: its prompt and the tracks made from it. [Show me the stage](bardo:tour/script).

<a id="write"></a>
## Generate, then edit

**Generate script** asks Claude for a script built from the **script template** ([Templates](templates.md)), the channel (its name, niche, themes, look and language), the approved theme, the narrator's tone and script style, and the video's [format](projects.md#format): 45 to 60 seconds of narration for a Short, 8 to 10 minutes for a long video. It runs as a job, so you can keep working meanwhile; you need a Claude key ([Your API keys](api-keys.md#providers)).

Once the script is here, it is yours: type straight into it. It holds up to 60,000 characters, and the word count beside the title gives a sense of the video's length.

<a id="review"></a>
## A new version to review

**Regenerate** asks for a whole new script without touching yours. The new one waits above it as **New script to review**:

- **Accept new script** replaces the current one, your edits included.
- **Keep current** throws the new one away.

Until you choose, the Script stage reads **New version to review**.

<a id="save"></a>
## Saving your edits

**Save** keeps what you typed, and the script is marked **Edited**. **Discard changes** goes back to the script as last saved. Anything made from an earlier script, such as the narration, then shows as **Out of date** ([Narration](narration.md#stale)).

<a id="cost"></a>
## What it costs

Before you generate, the estimate under the buttons says what the call will cost at the provider's rates. If the call would go past one of your budgets, Bardo asks before it starts. What the project has spent so far is under its name ([Projects and stages](projects.md#cost)).

<a id="details"></a>
## Where it came from

**Details** lists the provider, the model, the template and its version, the tokens used and when the script was generated. **Show prompt** shows the instructions and the prompt exactly as they were sent. A new script waiting for review has its own details.

<a id="music"></a>
## The music prompt

Below the script, **Generate music prompt** asks Claude for a prompt for the video's music, from the **music prompt template**, the channel, the theme and the video's length. Edit the prompt and **Save**; **Generate again** replaces it, your edits included. Make the music from it right below ([Music from the prompt](#generated-music)), or **Copy** it into a tool you have the rights to use and import the track in the editor's Media tab.

<a id="generated-music"></a>
## Music from the prompt

Under the prompt, **Generate music** has ElevenLabs Music compose an instrumental track from the prompt as saved, as long as the narration (from 3 seconds up to 10 minutes). Make the narration first. It runs as a job, and it needs an ElevenLabs key with the **Music** permission on a paid ElevenLabs plan ([Getting each API key](get-api-keys.md#elevenlabs)). The estimate beside the button is priced per minute of music, and a track that would go past ElevenLabs' budget asks you first ([Costs](costs.md)).

The new track waits on the card, marked **New track**, with its length and model. **Play** it, then:

- **Use this track** puts it on the music track (A2) from 0:00, ducked under the narration as your cut has it, in place of the generated track used before. Music you placed yourself stays. Every cut made later (the rough cut, or a cut that starts over after new scenes or a new narration) starts with the track in use.
- **Discard** deletes it.

**Generate another** makes a new track that takes the place of the one waiting for review; the track in use stays until you use another. A track you used is a media file of the project, so it is also in the editor's Media tab ([The editor](editor.md)), where you can place it again by hand. A track waiting for review is not.

Each track keeps the prompt, the length asked for, the model and the job that made it. ElevenLabs signs it with C2PA content credentials, which say it was made by AI. A job that failed shows ElevenLabs' reason: no Music access on the plan, a key without the Music permission, credits used up, or a prompt ElevenLabs refused (for example one that names an artist; ElevenLabs suggests another wording).
