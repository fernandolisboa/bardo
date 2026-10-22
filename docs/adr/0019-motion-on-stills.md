# ADR-0019: Motion on stills (Ken Burns), drawn with `zoompan`

- Status: Accepted
- Date: 2026-10-22
- Amends: the MVP spec, which left keyframed effects out of the editor (a still showed as a frozen frame)

## Context

Most scenes of a video stay stills: animating each one with a video model costs money per second (Kling 3.0 at 720p is about US$0.084 a second, about US$45 for a 9-minute video). A still was rendered as a frozen frame (`-loop 1`), so a long video of stills played as a slideshow, which holds viewers poorly and reads as low-effort content.

The bundled ffmpeg is the LGPL build (ADR-0007), which has no `perspective` filter. `zoompan` is in it, but it places and sizes its window in whole pixels of the picture it is given, and in pairs of pixels for 4:2:0 input: fed the output-sized picture, a slow pan visibly judders.

## Decision

1. **Six motions and none.** Zoom in, zoom out, and pan left, right, up and down, each at a strength: subtle (8% closer), medium (15%) or strong (25%). A zoom stays centered; a pan holds its strength's zoom and crosses the picture from edge to edge. The zoom changes by the same ratio each frame and a pan moves at an even pace, with no easing.
2. **A clip's own field, after framing.** A motion is set per video clip, apart from its framing, and moves over the picture as framed: the crop window in 9:16, the whole picture in 16:9; where the picture shows with bars (Fit in 9:16, a tall picture in 16:9), the bars move with it. It plays on stills only: a scene that gets a clip keeps its motion unplayed, footage has none, and black has nothing to move.
3. **The whole time on screen.** The view moves from the first frame the clip shows to the last, the halves of the transitions into and out of it included, so a clip never stops moving while it shows. A split gives each piece the whole motion; two halves join only with the same motion.
4. **A varied default per channel.** Each channel has a pattern for its new rough cuts: none, varied (zoom in, pan right, zoom out, pan left, in turn, so consecutive scenes differ) or one motion for every scene, at one strength. New channels start at varied and medium. Channels from before start at none: a rough cut nobody edited is rebuilt from its channel, so motion there would change cuts and outdate renders made before; the user turns motion on in the channel or in the editor. The inspector puts one motion, or a varied pattern, on every scene at once, scenes showing a clip included (unplayed until they show their still again).
5. **`zoompan` on a picture twice as large, opened when it plays.** A moving still is read once (its first frame only, not looped) by a `movie` source in the graph, framed at twice the output size in 4:4:4, and `zoompan` takes the view from it for each frame. Its whole-pixel steps are then half an output pixel times the zoom. Two media tests follow a band edge through a pan and a zoom and check every frame's step: both fail at 1× and pass at 2×. 3× was measured too: about twice the time of 2× for a step hardly anyone sees. The `movie` source, unlike an input, is opened only when its clip plays: ffmpeg sends every input's first frame through the graph as it starts, so with inputs a preview of 60 moving stills took 3.3 s to its first frame (1.0 s frozen); with `movie` it takes 0.3 s.
6. **Preview as rendered.** The preview plays motions from the proxies with the same graph. A preview starting inside a moving still, and the frame shown while paused there, move on from where the view was.
7. **Edits like any other.** Setting a clip's motion, or every still's, is a cut edit, saved as it is made and undone as one step. No AI picks motions.

## Consequences

- A moving still costs more to render than a frozen one: on a 4-core Linux container, about 8 ms more per 1080p frame, and 40 moving stills of a 1080×1920 render peaked at 1.7 GB against 1.6 GB frozen. Renders of plans without motion are unchanged, and their fingerprints too, so existing renders stay current. The reference-machine figure is recorded with the creation-phase validations (#51).
- Cuts and channels made before this keep stills still until the user picks a motion; new channels' cuts move. A saved cut that starts over (new scenes or narration) gets its channel's motions, as any new cut.
- Motions are not keyframes: there is no start or end point to place, only a kind and a strength. Keyframed effects stay out of scope.
