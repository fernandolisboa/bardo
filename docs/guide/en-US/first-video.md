---
id: first-video
title: Your first video
group: getting-started
---

# Your first video

This page follows one video from an empty Bardo to a published post. Each step is a screen; the links open it. Save your [API keys](api-keys.md) first, since most steps call a provider.

<a id="channel"></a>
## 1. Create a channel

A channel is your brand: its name, niche, the themes it covers, its look, the language it speaks and the country it targets. On [Channels](bardo:go/channels), choose **New channel** and fill it in. Pick a default narrator for it too, and add its account on each network you post to; those accounts hold each network's post defaults (tags, footer, visibility) and render settings.

<a id="persona"></a>
## 2. Choose a narrator

A persona is a narrator: a voice from your ElevenLabs account, a tone, a writing style and voice settings. Bardo comes with four, two in English and two in Portuguese. On [Personas](bardo:go/personas) you can listen to a voice before using it, tune it, or create your own from any voice in your ElevenLabs account, clones included. Personas belong to you, not to a channel, so one can narrate for several channels.

<a id="research"></a>
## 3. Research a niche

On [Research](bardo:go/research), pick the channel and enter a few seed niches or keywords. A job looks up recent YouTube uploads for each one and scores it: **competition** (how hard it is to stand out), **trend** (how fast new videos gather views) and **opportunity**, which weighs both. Results are kept for seven days, so looking again costs no quota.

<a id="theme"></a>
## 4. Pick a video idea

On [Themes](bardo:go/themes), ask for ideas for a niche. Claude suggests titles and angles, and the decision engine ranks them by fit with the channel, trend and competition, plus how your past videos did once you have some. Edit an idea, discard it, or approve it: **approving starts a video project**.

<a id="project"></a>
## 5. Produce it, stage by stage

A [project](bardo:go/projects) moves through stages, shown across its top. Each one can be reviewed and changed before the next:

- **[Script](bardo:go/projects/script)**: Claude writes it from the idea, the persona and the channel. Edit it freely; a new version waits beside the current one until you accept it.
- **[Narration](bardo:go/projects/narration)**: the persona reads the script, with the timing of every word. You can also import your own recording.
- **[Scenes](bardo:go/projects/scenes)**: Claude splits the narration into scenes, each with an image prompt you can edit, and each scene gets its image.
- **[Clips](bardo:go/projects/clips)**: optional short videos made from a scene's image, with the cost shown before you generate.

When a script or a narration changes, what was made from it shows as out of date, so you always know what to generate again.

<a id="editor"></a>
## 6. Edit

The **Edit** stage opens the editor with a rough cut already laid out from the scenes and the narration. Split, trim, move and delete clips; cuts snap to the narration's words. Mix the narration with music and sound effects, edit the captions, and choose the frame shape (16:9 or 9:16). Cut suggestions mark good places to cut; accepting one is just an edit, and you can undo it. [The editor](editor.md) explains each part.

Bardo does not make music. In the project it writes a music prompt for your own music tool; import the track you make in the editor's Media tab.

<a id="render"></a>
## 7. Review and render

**Review & render** shows the cut, the mix loudness, the captions and, for each of the channel's network accounts, its format and any problems found (too long for the network, captions off, a mix far too quiet). Choose the networks and confirm: the render runs as a job, and you can keep working meanwhile. More in [Review and render](render.md).

<a id="publish"></a>
## 8. Publish

At the [Publish](bardo:go/projects/publish) stage Claude writes each network's title, description and tags, which you edit within each network's limits. Then either:

- **Export** a folder per network with the file and a text file to copy from, and post by hand; paste the post's link back so Bardo can follow its numbers. See [Exporting](exporting.md).
- **Upload** to YouTube, Instagram Reels or TikTok (as a draft you finish in the TikTok app), once the channel's account is connected on [Accounts](bardo:go/accounts) with your own app saved in [Settings › Networks](app-credentials.md). Every upload shows a review first, and YouTube and Instagram uploads can be scheduled. See [Uploading and scheduling](uploading.md).

<a id="after"></a>
## 9. See how it did

[Performance](bardo:go/performance) shows each video's numbers and how its first week compares with the channel's usual. The theme ranking reads them too, so your next ideas are weighed against what worked.
