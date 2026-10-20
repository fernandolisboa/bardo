# ADR-0016: Transitions between video clips, centered on the cut

- Status: Accepted
- Date: 2026-10-20
- Amends: the MVP spec, which left transitions out of the editor (the picture only cut from one clip to the next)

## Context

The video track only cut: the next clip's first frame followed the last frame of the clip before. Faceless channels lean on soft joins between scenes (a dissolve, a dip to black, a push) to carry the viewer from one image to the next, and a hard cut on every scene reads as a slideshow.

The narration drives the cut. Clips are placed against the narration's words, captions follow the words, and the music ducks under them. A transition must not move any of that.

## Decision

1. **Ten kinds, drawn by ffmpeg's `xfade`.** Crossfade, fade through black, fade through white, wipe left and right, slide left, right, up and down, and zoom in. Each is one `xfade` transition, so the preview and the render draw the same thing with no new dependency (ADR-0007).
2. **Centered on the cut.** The clip after starts half the transition (rounded down to a frame) before the cut, and the clip before plays on for the rest. The cut stays where it was, so the video is as long as it was and stays in sync with the narration. Where a clip's file has no picture past its edge (the start or end of the file, or a still), its edge frame holds for that half.
3. **Set on the clip it leads into.** A transition belongs to the clip after the cut. It moves with that clip when clips are reordered, goes away with it when it is deleted, and the first clip never has one. A split makes a seamless cut, with no transition, and two halves with a transition between them do not join.
4. **Lengths in whole frames.** 0.1 s to 2 s in steps of 0.1 s; 0.5 s unless the user sets another. A transition is kept as set; a clip shorter than its transitions plays them cut short, never overlapping, and one cut to under two frames plays as a cut.
5. **Per channel default.** Each channel has a default transition (Crossfade, 0.5 s): the kind and length the T key and a kind picked on a cut take. The rough cut stays all cuts; the user adds transitions.
6. **Edits like any other.** Setting, changing or removing a transition, and putting one on every cut at once, are cut edits, saved as they are made and undone as one step.
7. **No AI suggestions.** The decision engine does not suggest transitions.
8. **Preview as rendered.** The preview plays transitions from the proxies; a preview that starts inside one, and the frame shown while paused there, draw it from its start and skip to the playhead, so the paused frame is the rendered one.

## Consequences

- A rendered video differs from a hard-cut one only around the cuts that have a transition; plans without transitions render exactly as before, so their existing renders stay current.
- Opening from black and closing to black are not transitions between clips and stay out; they can come later as their own setting.
- Each transition adds an `xfade` to the render graph. Runs of clips joined by transitions are drawn as one chain and joined to the rest with `concat`, so a cut with many transitions costs a little more to render than an all-cuts one.
