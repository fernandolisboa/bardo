---
id: projects
title: Projects and stages
group: production
place: projects
tour: projects
---

# Projects and stages

A video project is one video on its way from an approved idea to a published post. [Projects](bardo:go/projects) shows one project at a time: who narrates it, its stages from the script to publishing, and what it has cost. Approving an idea on [Themes](themes-ranking.md#review) starts a project. [Show me the screen](bardo:tour/projects).

<a id="switch"></a>
## Picking a project

The project's name heads the screen, with a menu beside it that lists the channel's other projects; pick one to switch. The channel picker next to **Projects** lists another channel's projects.

<a id="narrator"></a>
## The narrator

The narrator is the persona that reads the video's narration, and its tone and script style shape the script Claude writes. A project uses its channel's default persona until you pick another one in **Narrator**, for this video only. Change the channel's default on [Channels](bardo:go/channels); create and tune narrators on [Personas](personas.md).

<a id="stages"></a>
## The stages

The stages run across the top, in order: [Script](script.md), [Narration](narration.md), [Scenes](scenes.md), [Clips](clips.md), Edit, Render and Publish. Pick one to open it; Edit opens the editor. The line under each stage says where it stands:

- Once done, what it holds: the script's words, the narration's length, the images or clips made.
- **Generating…** while one of its jobs runs. Follow it in [Jobs](bardo:go/jobs).
- **… to review** when a new version waits beside the current one.
- **Out of date** when an earlier stage changed after it was made.
- **After the …** when it is locked (below).

<a id="unlock"></a>
## How a stage opens

Each stage works on what the one before gave it, so a stage is locked until then and its line says what it waits for:

- **Narration** opens once there is a script, since the narrator reads it.
- **Scenes** opens once there is a narration, since scenes are timed on its words.
- **Clips** opens once a scene has an image, since a clip animates it.
- **Edit** opens once the scenes are planned: the editor lays out a rough cut from them and the narration.
- **Render** and **Publish** open after the edit and the render.

Nothing is thrown away when an earlier stage changes. What was made from it shows as **Out of date**, so you know what to make again: a narration after the script changed, scenes after the narration was made again.

<a id="cost"></a>
## What it has cost

The line under the project's name shows its niche, when it was started and, once something was paid for, what the project has cost so far: every call to a provider for this video, from the script to the clips. Each generation shows its estimate before you start it, and a call that would go past a budget asks first. [Costs](bardo:go/costs) breaks the spending down by provider, channel and video; see [Your API keys](api-keys.md#budgets) for budgets.
