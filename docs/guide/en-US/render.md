---
id: render
title: Review and render
group: editing
place: projects/render
tour: render
---

# Review and render

Rendering turns the cut into the files you publish: one per network account of the channel, in that network's preset. It takes a while and replaces each network's last file, so the [Render](bardo:go/projects/render) stage first shows what will come out and what stands in the way, and renders only after you confirm. [Show me the stage](bardo:tour/render).

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

Each network account of the channel is a target, listed with its handle, its preset, its state and its last file. Each network starts from its own preset:

| Network | Frame | Size | Bitrate | Longest |
| --- | --- | --- | --- | --- |
| YouTube | 9:16 | 1080×1920 | 12 Mbps | 3 min |
| TikTok | 9:16 | 1080×1920 | 10 Mbps | 10 min |
| Instagram Reels | 9:16 | 1080×1920 | 10 Mbps | 15 min |
| X | 9:16 | 720×1280 | 6 Mbps | 2 min 20 s |
| Kick | 16:9 | 1920×1080 | 8 Mbps | 12 h |

All of them use H.264 and aim at −14 LUFS. You can change an account's preset (frame, size, codec, bitrate, length limit and loudness) on [Accounts](bardo:go/accounts). A channel with no accounts has nothing to render for: add one there first.

Tick the networks to render; those not rendered yet, or out of date, come ticked. Click a network to see it in the inspector: its preset, the encoder this computer will use (on the graphics card when it can), its checks and its last file.

<a id="gates"></a>
## What blocks and what warns

Each check is marked **Blocks** or **Warning**.

- **Blocks** keeps that network out of the render until you fix it: a cut longer than the network takes, or no encoder on this computer for the preset's codec. The other networks still render.
- **Warning** lets it render as it is: captions off, clips with no media (they render black), a silent mix, a mix far from the network's loudness (the render changes it that much), or peaks the render has to limit. When the cut's frame differs from the network's, each clip goes through its crop window.

A network's state sums it up: **Ready**, **n to check** (warnings), **Blocked**, or **Checking** while the checks run.

<a id="render"></a>
## Rendering

**Render (n)** asks once more: how many files, that each replaces the network's last one, and how many warnings stay. **Render now** queues one job for all of them. The job runs in the background with its progress on the stage and in Jobs, so you can keep working; **Cancel** stops it, and **Resume** renders the files left, keeping those already done. A render waits while an export is copying the files.

<a id="last"></a>
## The last file

Each network keeps its last rendered file. **Rendered** means it matches the cut and preset as they are; **Out of date** means the cut or the preset changed after it was made, so the next render ticks it again; **Not rendered** means there is none yet. The inspector shows its size, loudness (and whether it hit the target) and encoder, and **Show in folder** opens it. The Publish stage exports and uploads these files.
