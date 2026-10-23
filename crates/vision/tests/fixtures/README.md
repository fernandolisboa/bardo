# Vision test clips

Short clips the vision model tests (`../models.rs`) run the real models on.
They are cut from two NASA Goddard interviews (GOLD mission resources, 2018),
works of the US federal government and so in the public domain. The full
recordings are fetched by SHA-256 by `spikes/vision-probe/tools/takes.py`
(`source-head.mp4` and `source-gestures.mp4`).

All clips are 640×360 at 15 fps, without sound, H.264 at CRF 30.

| Clip | What it shows | Cut |
| --- | --- | --- |
| `head-3s.mp4` | A seated talking head, 3 s. | `source-head.mp4` from 7.2 s |
| `standing-1s.mp4` | A standing speaker with a small face, 1 s. | `source-gestures.mp4` from 7.2 s |
| `two-people-1s.mp4` | Two faces side by side (the head clip and its mirror image, a dark band between), 1 s. | `source-head.mp4` from 7.2 s |

```sh
ffmpeg -ss 7.2 -t 3 -i source-head.mp4 -vf "fps=15,scale=640:360,format=yuv420p" \
  -an -c:v libx264 -crf 30 -preset slow head-3s.mp4
ffmpeg -ss 7.2 -t 1 -i source-gestures.mp4 -vf "fps=15,scale=640:360,format=yuv420p" \
  -an -c:v libx264 -crf 30 -preset slow standing-1s.mp4
ffmpeg -ss 7.2 -t 1 -i source-head.mp4 -filter_complex \
  "[0:v]fps=15,scale=640:360,crop=240:360:330:0,split[a][b];[b]hflip[c];color=c=0x203040:s=160x360:r=15,trim=duration=1[g];[a][g][c]hstack=inputs=3,format=yuv420p" \
  -an -c:v libx264 -crf 30 -preset slow two-people-1s.mp4
```
