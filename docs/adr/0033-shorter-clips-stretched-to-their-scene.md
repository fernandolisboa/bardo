# ADR-0033: A scene a little longer than a clip length pays for that length, and the clip plays slowed

- Status: Accepted
- Date: 2026-11-25
- Issue: #260
- Builds on: production modes and the whole-video estimate (#248), the clip job that never pays twice for a request (ADR-0030), the channel default that old channels read unchanged (ADR-0018)

## Context

A clip runs as long as its scene is narrated. Several video models only make a few fixed lengths: 5 or 10 seconds, or 4, 6 or 8. Bardo asked such a model for the first length that covers the scene, and the editor cut the rest. Scenes of a narrated video mostly last 5 to 8 seconds, so most of them paid for a 10 second clip and showed little more than half of it. In a Short with every scene animated, that was about half of the clip seconds paid for and never seen, and clips are by far the largest cost of a video.

Slowing a clip a little is a common editing move: slight slow motion on a camera drift or a subtle action is hard to notice, while a clip that plays at half speed reads as an effect. How far is acceptable depends on what a channel shows.

## Decision

1. **A fixed-length model is asked for the longest length short of the scene that the stretch limit covers.** `ClipDurations::fit(wanted, stretch)` returns the seconds to ask for and, when it picked a shorter length, the limit it used. With no such length, the first length that covers the scene, as before. A scene longer than every length takes the longest, as before, and stretches it too: for the same price it covers more of the scene before it holds its last frame. A scene that exactly matches a length takes it. A model with a range of lengths is asked for the scene's own length, clamped to the range, and is never stretched.
2. **The limit is a percent of the clip's own length**, `StretchLimit`, from 5 to 100 in steps of 5, 40 by default: a 5 second clip covers a scene of up to 7 seconds and plays no slower than 71% of its speed. A value between two steps (from a settings file) is read as the nearest step, so what is stored is always what the form shows. The default is deliberately on the cautious side of where slow motion starts to show.
3. **The channel chooses.** `channel.clip_fit` is `stretch` (the default, also for channels from before) or `cover`, and `channel.clip_stretch_percent` holds the limit. An unknown fit code reads as the default. Both travel in the settings file; a file from before them brings the defaults.
4. **A clip records the limit it was made under** (`scene_clip.stretch_percent`, 0 for none). The clip plays over its scene and half a second after it (the picture a transition out of it shows, which would otherwise be its last frame held), up to its own length plus that percent, and never shorter than it was made. A clip without a limit plays at its own speed and, when its scene is longer, holds its last frame as before. So no clip made before this changes, no cut already made changes, and changing the channel never changes a clip the user has reviewed.
5. **A clip job carries the limit it was queued with**, like its models. A retry or a resumed run asks for the same lengths as the run the user saw the estimate for, whatever the channel says by then. A payload from before the field covers every scene.
6. **Estimates use the same rule as the request.** The scene's next clip, the run of missing clips and the production modes' whole-video figures all call the same `fit`, so what is shown is what is asked for.
7. **The timeline stays in played time; only the render converts.** A video source on the timeline has the length it plays and, when stretched, the length of its file. Trimming, splitting, transitions, captions and sync all count played time, exactly as with any clip. The render graph alone maps a played range to the file's range (times the file length over the played length), resets timestamps and scales them by the inverse, and resamples to the output frame rate by blending neighbouring frames (`framerate`) rather than repeating them (`fps`), so a slowed clip has no stutter. The blended stream is put on the output's time base, which a transition needs of both its sides, and the file is read a tenth of a second past the range, so the last frame shown has a next one to blend with. A plan with no stretched clip is written byte for byte as before, which keeps render fingerprints.
8. **The user is told.** The next clip's line says it will play slowed, a stretched clip carries a tag, and the editor's inspector shows the length it was made at and the length it plays.

## Consequences

- A typical Short with every scene animated on a 5 or 10 second model pays for about half the seconds it did; the production modes' figures show it before anything is made.
- Slight slow motion is now the default look of clips from fixed-length models. Channels whose clips show fast action can lower the limit or cover every scene.
- Blended frames soften fast motion a little. Motion-compensated interpolation would be sharper and can invent artefacts; it is not used. Blending costs about a sixth more render time on the clips it touches.
- A clip's length is the length asked for. A file a provider returns a little shorter holds its last frame for the difference, as it did before; measuring each file is left for when real clips show it matters.
- Playing a scene's clip from the Clips stage opens the file itself, at the speed it was made; the slowed clip is seen in the editor's preview and in the render.
- A scene that grows after its clip was made is covered up to the clip's limit and holds the last frame beyond it; a scene that shrinks below the clip's own length plays the clip at its own speed and cuts the rest, never faster than it was made.
- The rule looks only at one scene. Asking for one clip over two neighbouring scenes, or chaining clips by their last frame, are separate ideas and not part of this.
- What a stretch of 40% looks like on real clips of each provider is a real-key check, since no paid run is made while building.
