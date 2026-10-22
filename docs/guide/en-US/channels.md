---
id: channels
title: Channels
group: publishing
place: channels
tour: channels
---

# Channels

A channel is one niche for one market: a subject, a language, a country and a narrator. Everything else hangs off a channel: its research, its video ideas, its projects and its [network accounts](network-accounts.md). Channels live on [Channels](bardo:go/channels). [Show me the screen](bardo:tour/channels).

<a id="list"></a>
## Your channels

The list shows each channel with its niche and country. Pick one to edit it beside the list; **New channel** empties the form for a new one. **Create channel** (or **Save changes**) checks the form and says, under each field, what it still needs.

<a id="niche"></a>
## Niche and themes

- **Niche**: what the channel is about, in a few words (for example *space history*), up to 100 characters. [Research](niche-research.md) scores it, and [theme ideas](themes-ranking.md) are written for it.
- **Themes**: the subjects the channel keeps coming back to, one per line. Ideas and scripts lean on them, so a channel about space history can stay with missions, astronauts and discoveries.

<a id="look"></a>
## Aesthetic notes

How the channel's videos should look and feel: the visual style, the palette, the mood, what to avoid. Ideas and scripts take these notes along, and so do the scene prompts written from the script, so every video keeps the channel's look.

<a id="market"></a>
## Language and country

- **Content language**: the language the channel's scripts, narration and post texts are written in.
- **Target country**: the audience the channel aims at. [Research](niche-research.md#market) reads trends and competition for that country.

Each network account of the channel can still post in another language; see [metadata defaults](network-accounts.md#metadata).

<a id="persona"></a>
## Default persona

The [persona](personas.md) that narrates the channel's videos: its voice, tone and script style. A new project starts with it, and a project can pick another narrator. One persona can narrate several channels; **No default persona** leaves the choice to each project.

<a id="format"></a>
## Video format

- **Short (9:16, under a minute)**: a vertical video of 45 to 60 seconds, which every network takes. New channels start here.
- **Long (16:9, 8 to 10 min)**: a wide video of 8 to 10 minutes, for YouTube. The other networks keep their presets (vertical, except Kick's 16:9), and a cut longer than one of them takes is [blocked](render.md#gates) for that network alone, so a long video is really a YouTube video.

The format decides how long Claude writes the script, the shape the scene images and clips are made in, the shape of a new cut in the editor and which [render preset](render.md#targets) YouTube uses. Each project follows its channel's format unless it picks the other ([Projects](projects.md#format)). Changing it applies to what is made next; scripts, images and cuts already made keep their shape.

<a id="defaults"></a>
## Video model, captions, transitions and motion

- **Video model**: the model that animates the channel's scene images into short clips, through Higgsfield, Kling or Google. Keeping the provider's default follows it when it changes. Any scene can pick another model at the [Clips](clips.md#model) stage.
- **Caption style**: how captions look in the channel's new projects. Each project can pick another style in the [editor](editor.md#captions).
- **Default transition** and its **Length**: what the editor puts on a cut when you press **T** or pick a transition for a clip, a crossfade of 0.5 s unless you change it. Every cut can use another one ([Transitions](editor.md#transitions)).
- **Motion on stills** and its **Strength**: how a new cut moves the view over each still, the Ken Burns effect. **Varied** (the default for a new channel, at **Medium**) gives each scene a different zoom or pan from the one before; **None** keeps stills still, and is where channels made before motions existed start, so their cuts and renders stay as they were; or pick one motion for every scene. Cuts already made keep their motions, and every still can use another one ([Motion on stills](editor.md#motion)).

<a id="accounts"></a>
## Next: network accounts

A channel posts through its accounts, one per network. Add them on [Accounts](bardo:go/accounts): each has the handle it posts as, the metadata every post starts from and the render preset its files use. See [Network accounts](network-accounts.md).
