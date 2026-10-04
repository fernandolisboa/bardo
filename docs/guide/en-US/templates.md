---
id: templates
title: Templates
group: production
place: templates
tour: templates
---

# Templates

Templates are the prompts Bardo sends to the AI. Change one to change how Bardo writes scripts, plans scenes, words music prompts or drafts posts, for every channel. They live on [Templates](bardo:go/templates). [Show me the screen](bardo:tour/templates).

<a id="kinds"></a>
## Kinds of template

There is one template per kind of generation:

- **Script**: the video's script ([Script](script.md)).
- **Image prompts**: the scene plan and each scene's image prompt ([Scenes and images](scenes.md#plan)).
- **Music prompt**: the prompt for your music tool ([Script](script.md#music)).
- **Metadata**: each network's title, description and tags at the Publish stage.

<a id="versions"></a>
## Versions

Every save adds a version, numbered in order; the newest is **Current** and is what generations use. Older versions stay in the list: pick one to load its text into the editor, and save it to make it current again. Each generation records the version it used, so a script's **Details** tells which one wrote it.

<a id="fields"></a>
## Instructions and prompt

A template has two parts, each up to 8,000 characters:

- **Instructions**: the standing rules, such as the role, the tone and the format of the answer.
- **Prompt**: the task itself, with the project's facts placed in it through variables. It cannot be empty.

<a id="variables"></a>
## Variables

Write a variable as `{{name}}`; each generation replaces it with the project's value. The list under the editor shows the variables this kind of template can use and what each holds, such as `{{channel_name}}`, `{{persona}}` or `{{narration_sentences}}`. A misspelled or unknown variable, or one missing its closing `}}`, keeps the template from being saved.

<a id="save"></a>
## Saving and starting over

**Save as new version** keeps your text as the next version; when nothing changed, no version is added. **Discard changes** goes back to the current version. **Load Bardo's default** puts the text Bardo shipped with into the editor, to save as a new version if you want it back.
