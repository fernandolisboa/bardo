# ADR-0046: An ad's export for ads renders each shape again and never sends it

- Status: Accepted
- Date: 2026-11-26
- Issue: #283
- Builds on: framing (PRD story 67), composed pictures (ADR-0040), the end card (ADR-0044), the ad review (ADR-0045), the export for networks (#28)

## Context

An ad runs as a paid ad as well as a post. Ad managers ask for shapes the editor does not cut: 4:5 and 1:1 above all, and the other of 9:16 and 16:9. They also ask for two texts beside the video: a primary text above it (cut after about 125 characters) and a headline beside the button (40). Cropping the rendered video into those shapes would cut the product shots and the end card, whose product and text sit inside the cut's frame, and would leave captions laid out for another frame.

## Decision

1. **Four frame shapes.** The domain gains `FrameShape` (9:16, 16:9, 4:5, 1:1), each 1080 px on its short side. The editor still cuts in `AspectRatio`; framing, product shots and end cards take any `FrameShape`. The caption band grows with a squarer frame (38% in 9:16 and 16:9, 42% in 4:5, 46% in 1:1), so a product shot keeps clear of two caption lines in each.
2. **Each shape renders the cut again; nothing is cropped from a render.** For each shape Bardo builds the cut's render plan in that shape: composed scenes (product shots and the end card) are composed again for it from their product image and design, and fit whole; every other picture and clip fills the frame through a window. In 16:9 every picture shows whole, as the editor's framing already does there.
3. **A window per clip and per shape.** A window is centered until the user moves it on the Publish stage; it is kept by shape and by clip, the clip known by its picture and where its source starts, so it stays with the clip when others move.
4. **Captions are laid out per shape** by the renderer, and none show over the end card.
5. **The texts for an ad manager come with the post texts.** The ad's metadata job writes a primary text and a headline in the language of the first network's post, under the ad rules, and the check of the post texts reads them with the networks' texts. The user edits them; text over a limit is kept and keeps the export from starting.
6. **One job, a folder per shape.** The export goes with the ad review's acknowledged items and waits for unchecked *product in use* scenes and for composed pictures that wait. A job renders the chosen shapes one after another, with progress and cancel, and resumes from the shapes it finished. Each shape's folder (`Ads/<shape>` in the project's export folder) gets the video (H.264, 12 Mbps, AAC 192 kbps, at the loudness of the project's render) and `ad.txt`: the file, its size and shape, the two texts, the brief's call to action and link, the commercial disclosure and synthetic-media reminders, and the acknowledged items.
7. **Out of date by fingerprint.** Each shape's export records what it was made from (its plan, its composed pictures, the texts and the loudness); another fingerprint reads *Out of date*.
8. **Bardo never sends it.** No ad manager is called; the user uploads the files.

## Consequences

- Migration 0078 adds `ad_texts` (one row per project), `ad_window` (one row per project, shape and clip) and `ad_export` (one row per project and shape), each deleted with the project.
- A shape renders from the original pictures and clips, so an export for ads takes about as long as a render per shape.
- Composed pictures for a shape are files of their own (`ad-<shape>-<picture>`), so the cut's pictures stay as they are.
- A window is not shown over the live preview; the review shows each clip's picture with its window outlined.
