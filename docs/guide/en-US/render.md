---
id: render
title: Review and render
group: editing
place: videos/render
tour: render
---

# Review and render

Rendering turns the cut into the files you publish or share: one per network you tick, in that network's preset. A file never needs an account; only [uploading](uploading.md) does. It takes a while and replaces each network's last file, so the [Render](bardo:go/videos/render) stage first shows what will come out and what stands in the way, and renders only after you confirm. [Show me the stage](bardo:tour/render).

<a id="review"></a>
## The review

Opening the stage checks the cut and this computer: it measures the mix and tries the video encoders, which takes a few seconds. The figures at the top describe the cut as it will come out:

| Figure | Says |
| --- | --- |
| Length | How long the cut runs |
| Cut frame | 16:9 or 9:16, as set [in the editor](editor.md#framing) |
| Mix loudness | The mix's integrated loudness, in LUFS |
| Captions | Whether the captions show |

A figure in amber deserves a look: a silent mix, or captions turned off. **Check again** runs the checks once more, after you change something outside the cut.

<a id="targets"></a>
## Networks and presets

Every network is a target, listed with its handle, its preset, its state and its last file. A network the channel has an [account](bardo:go/accounts) for shows that account's handle. Every other network, and every network of a [loose video](projects.md#loose-videos), shows **File**: Bardo renders the file for you to download, send to someone or post yourself, and it is never uploaded from Bardo. Each network starts from its own preset, and YouTube's follows the project's [format](projects.md#format):

| Network | Frame | Size | Bitrate | Longest |
| --- | --- | --- | --- | --- |
| YouTube, Short | 9:16 | 1080×1920 | 12 Mbps | 3 min |
| YouTube, long video | 16:9 | 1920×1080 | 12 Mbps | 12 h |
| TikTok | 9:16 | 1080×1920 | 10 Mbps | 10 min |
| Instagram Reels | 9:16 | 1080×1920 | 10 Mbps | 15 min |
| X | 9:16 | 720×1280 | 6 Mbps | 2 min 20 s |
| Kick | 16:9 | 1920×1080 | 8 Mbps | 12 h |

All of them use H.264 and aim at −14 LUFS. An account's preset (frame, size, codec, bitrate, length limit and loudness) can change on [Accounts](bardo:go/accounts), for every video of the channel; [this video's settings](#settings) change it for one video.

Tick the networks to render. The accounts not rendered yet, or out of date, come ticked, and so does a file out of date; the other files do not, so the render stays on the networks the channel is for. A video with no account at all, and nothing rendered yet, starts with one file ticked: the first network in the cut's own frame. Click a network to see it in the inspector: its preset, the encoder this computer will use (on the graphics card when it can), its checks, this video's settings and its last file.

<a id="settings"></a>
## This video's settings

The inspector's **This video's settings** change the picked network's frame, size, codec, bitrate, length limit and loudness for this video only: say a 16:9 file at 720p to send to a friend, or a longer limit for a file you will post by hand. Each one starts at **Default**, the account's value (or the network's, for a file); blank boxes keep it. **Save settings** keeps them, and the network's last file goes out of date until you render again. **Restore defaults** goes back to the account's or the network's preset. The account's own preset never changes here.

<a id="gates"></a>
## What blocks and what warns

Each check is marked **Blocks** or **Warning**.

- **Blocks** keeps that network out of the render until you fix it: a cut longer than the network takes, or no encoder on this computer for the preset's codec. Both can change in [this video's settings](#settings). The other networks still render.
- In an ad, a scene of the [product in use](scenes.md#product-check) whose product you have not checked keeps the whole video from rendering: the review lists those scenes. Check each one on Scenes, or use the real image there.
- In an ad, an [end card](scenes.md#end-card) not yet composed with its current design, brief offer and logo keeps the whole video from rendering. Compose it on Scenes: it is free.
- **Warning** lets it render as it is: captions off, clips with no media (they render black), a silent mix, a mix far from the network's loudness (the render changes it that much), peaks the render has to limit, or a YouTube video over 15 minutes, which YouTube takes only from a [verified account](https://www.youtube.com/verify). When the cut's frame differs from the network's, a 9:16 file takes each clip through its crop window, and a 16:9 file shows each picture whole, a tall one with bars at the sides.

A network's state sums it up: **Ready**, **n to check** (warnings), **Blocked**, or **Checking** while the checks run.

<a id="render"></a>
## Rendering

**Render (n)** asks once more: how many files, that each replaces the network's last one, and how many warnings stay. **Render now** queues one job for all of them. The job runs in the background with its progress on the stage and in Jobs, so you can keep working; **Cancel** stops it, and **Resume** renders the files left, keeping those already done. A render waits while an export is copying the files.

<a id="last"></a>
## The last file

Each network keeps its last rendered file. **Rendered** means it matches the cut and preset as they are; **Out of date** means the cut or the preset changed after it was made, so the next render ticks it again; **Not rendered** means there is none yet. The inspector shows its size, loudness (and whether it hit the target) and encoder, and **Show in folder** opens it. The Publish stage exports these files, and uploads those with an account behind them.
