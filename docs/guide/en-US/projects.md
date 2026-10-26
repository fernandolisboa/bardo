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

<a id="format"></a>
## Short or long

**Format** beside the project's name says whether this video is a Short (vertical, under a minute) or a long video (16:9, 8 to 10 minutes). **Channel's** follows the channel's [video format](channels.md#format); pick **Short** or **Long** to make this one video the other kind.

The format applies to what is made next: the next script's length, the shape of the next scene images and clips, a new cut in the editor and YouTube's [render preset](render.md#targets). What was already made keeps its shape, so after switching, write the script again and draw the scenes again for a video that matches.

<a id="mode"></a>
## How it is produced, and what it will cost

**Mode** beside the format says how this video is made: which scenes become AI clips and with which model. The other scenes stay still pictures, moved by the channel's [motion on stills](channels.md#defaults).

- **Economy**: stills with motion, no AI clips. The cheapest.
- **Animated hook**: stills with motion, and the scenes that start in the first 10 seconds animated by Kling 2.6 at 720p, to hold the viewer. New channels start here.
- **All animated**: every scene animated by Kling 2.6 at 720p.
- **All animated, premium**: every scene animated by Kling 3.0 at 1080p.
- **All animated, channel's model**: every scene animated by the channel's [video model](channels.md#defaults), as channels made before modes do.

**Channel's** follows the channel's [production mode](channels.md#production). Each choice in the menu shows what the whole video would cost in that mode, and the line beside it shows the total for the mode picked and what was spent so far. The estimate counts what is left to make (script, narration, scene plan, images and portraits, and the clips the mode makes) from the [rate table](bardo:go/costs), so it drops as the video gets made. Before the narration exists it assumes a minute for a Short and eight for a long video. With the rate table as Bardo ships it, a whole Short costs about US$ 1.40 in Economy, US$ 2.20 with the hook animated, US$ 5.60 all animated and US$ 8.10 premium; a long video about US$ 9, US$ 10, US$ 43 and US$ 63. Kling 2.6 makes clips of 5 or 10 seconds, so a 6-second scene pays for 10.

A mode whose clips need a key you have not saved says so in the menu: Kling's modes need a Kling key under [Settings › API keys](bardo:go/settings/keys). Changing the mode applies to what is animated next: on the Clips stage, **Animate scenes without a clip** (or **Animate the hook**) animates only the scenes the mode animates, clips made already stay, and any scene can still be animated by hand with any model.

**Cap** is what this video may cost before Bardo asks. A paid step of the video (a script, a narration, images, clips, takes) that would take its spend past the cap asks first, with what the step costs and what the rest of the video would cost in its mode; **Generate anyway** goes on. A step whose price is not known asks too. A job still running counts toward the cap once it has finished, so starting several at once can pass it. Leave it empty for no cap.

<a id="stages"></a>
## The stages

The stages run across the top, in order: [Script](script.md), [Narration](narration.md), [Scenes](scenes.md), [Clips](clips.md), [Edit](editor.md), [Render](render.md) and Publish. Pick one to open it; Edit opens the editor. The line under each stage says where it stands:

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

The line under the project's name shows its niche, when it was started and, once something was paid for, what the project has cost so far: every call to a provider for this video, from the script to the clips. Each generation shows its estimate before you start it, and a call that would go past a budget or the video's [cap](#mode) asks first. [Costs](bardo:go/costs) breaks the spending down by provider, channel and video; see [Your API keys](api-keys.md#budgets) for budgets.
