# ADR-0050: An AI bridge covers its cut and never adds time

- Status: Accepted
- Date: 2026-11-28
- Issue: #363
- Builds on: #362 (clip models that end on a given frame), ADR-0016 (transitions centered on the cut), ADR-0030 (paid calls are sent once), ADR-0033 (stretched clips)

## Context

Two consecutive clips can show the same subject in very different places, and a crossfade or a wipe still reads as a jump. A video model that starts on one picture and ends on another (#362) can animate between the last frame before the cut and the first frame after it. The editor needs a way to ask for that clip, review it and put it on the timeline without breaking the sync with the narration, and without paying twice for one answer.

Two placements were weighed:

- **Insert the bridge at the cut**, pushing everything after it later. The video grows by the bridge's length and every caption, narration line and music cue after it drifts.
- **Lay the bridge over the cut**, replacing the last half of the bridge's length of the clip before and the first half of the clip after. The video keeps its length and its sync.

## Decision

1. **A bridge covers `[cut − L/2, cut + L/2]` and never adds time.** `L` is a whole number of seconds the model offers. Each side keeps at least one frame of its own clip; a length that would eat a side whole is not offered. Using the bridge is one `Replace` edit over that stretch, so it is undone and redone in one step, like any overwrite.
2. **Its two ends are the frames the viewer would see**: the last frame before the stretch, in the clip before, and the first frame after it, in the clip after. A clip's frame is read from its file at its own time (converted back through a stretched clip's speed); a still's is the picture as it is. Both are taken by ffmpeg at the same size (the first frame's), so the model never gets two shapes.
3. **Only a model that ends on a frame is offered.** The model is checked before any picture is staged with the provider, and the request carries both pictures.
4. **The bridge is sent once (ADR-0030)**, through the same submission machinery as scene clips, in a namespace of its own (`{job}-bridge-{n}`) so its idempotency keys never meet a scene clip's. Every new submission stages both frames again. Its cost is recorded as *bridge clip* when the provider accepts it and taken back when the provider fails it.
5. **A made bridge waits for review.** It plays on its cut in the preview (a trial timeline over the proxies) before it is used, and only *Use* changes the cut. It is tied to the two clips it was asked for (their file, start, place and length); if either changes, the bridge can only be discarded, and discarding one still being made cancels its job so it does not go on to pay. A bridge still being made over an unchanged cut is cancelled from Jobs, not discarded. Two bridges never share a stretch: a bridge that would reach into another's is refused, since using one would leave the other on a changed cut.
6. **A used bridge becomes a project media asset** (generated), so it is backed up, packaged and reused like any other footage. The bridge record goes away; the clip stays.

## Consequences

- The narration, captions and music after a bridge stay where they were, at the price of losing up to half the bridge's length of each neighboring clip.
- A cut too close to the start or end of its neighbors offers no length; the panel says the clips around it are too short.
- A bridge over a cut that changes later (a trim, a move, a new clip) is kept as *stale* until discarded, so its paid clip is never thrown away silently.
