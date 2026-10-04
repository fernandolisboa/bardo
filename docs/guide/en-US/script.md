---
id: script
title: Script
group: production
place: projects/script
tour: script
---

# Script

The script is what the narrator will say. Claude writes it, you edit it, and every later stage builds on it. It lives at the [Script](bardo:go/projects/script) stage of a project, with the video's music prompt below it. [Show me the stage](bardo:tour/script).

<a id="write"></a>
## Generate, then edit

**Generate script** asks Claude for a script built from the **script template** ([Templates](templates.md)), the channel (its name, niche, themes, look and language), the approved theme and the narrator's tone and script style. It runs as a job, so you can keep working meanwhile; you need a Claude key ([Your API keys](api-keys.md#providers)).

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

Below the script, **Generate music prompt** asks Claude for a prompt for your own music tool, from the **music prompt template**, the channel, the theme and the video's length. Bardo does not make music: **Copy** the prompt, make the track in a tool you have the rights to use, and import it in the editor's Media tab. Edit the prompt and **Save**; **Generate again** replaces it, your edits included.
