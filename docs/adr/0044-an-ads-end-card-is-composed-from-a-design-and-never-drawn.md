# ADR-0044: An ad's end card is composed from a design and never drawn

- Status: Accepted
- Date: 2026-11-26
- Issue: #281
- Builds on: composed product shots (ADR-0040), a product shot's look (ADR-0041), thumbnail looks (ADR-0021), product images and logo (#273)

## Context

An ad ends on its call to action. Until now its last scene, the end card, was a product shot of the product's image, or a drawn picture when the product had none, and the cut ended with the narration. A drawn card cannot carry readable text (models misspell it and the rules forbid text in pictures), the call to action had nowhere to stay on screen once spoken, and captions ran over the last picture.

## Decision

1. **The end card is always composed, never drawn.** The last scene of kind *end card* is composed on the computer like a thumbnail: the backdrop of its shot look, the product image it shows (whole, only scaled) when the product has one, the product's logo when it has one, and its lines drawn by libass in Bardo's fonts, in one of the thumbnail looks. A product with no image gets a card of text. Nothing of it goes to a model, and composing is free and needs no key.
2. **A card design per project, starting from the brief.** The design holds a headline (up to 40 characters, not empty), a second line (up to 60, may be empty), whether the offer shows, the look, where the text sits (above or below the image) and where the logo sits (in the corner or with the text). Until the user saves one, the design starts from the brief: its call to action, its link or handle made readable, and the offer shown when there is one. It is stored in its own table, keyed by project.
3. **The offer line is only the brief's offer.** The design stores whether to show it, never its words, so a change to the brief's offer shows on the card. The typed lines are the user's own; the claims check does not read them.
4. **Fixed places, clear of the networks.** Every part sits inside a room that keeps clear of each network's buttons and the caption band, in 9:16 and 16:9; the text takes a band of that room, the product the rest, and the logo a box of its own. The image's position (left, center, right) does not apply to a card.
5. **Out of date by fingerprint.** The scene records the fingerprint of what its card should show (the design, the resolved offer and the logo's bytes); the composed picture records the one it was made for. Saving a design, a new brief offer or a new logo moves the scene's fingerprint and starts a composing job; until the picture matches, the card reads *to compose*, the Scenes stage is not done and the video does not render. The app also compares the fingerprint with the inputs as they are now, so a card the job could not move yet still counts as missing.
6. **Timing in the cut.** With an end card, the rough cut ends at the later of 1.5 s after the last word and 2.5 s after the card's start (its scene starts on the call to action). No caption shows over the card, the card holds still (no motion), and a generated track is made as long as the cut so the music runs under the card.
7. **Off is a kind change.** There is no separate switch: making the last scene another kind turns the card off, and the cut then ends with the narration. The saved design stays for when it is made an end card again.

## Consequences

- Migration 0076 adds the table `end_card` (one row per project, deleted with it) and the columns `scene.card` (the wanted fingerprint) and `scene.shot_card` (the one composed for); a composed picture's image may now be empty, for a card of text.
- The whole-video estimate counts the end card as free even when the product has no image.
- An ad has no versions in other languages, so a card design is never copied to another project.
- The card's text is drawn by the bundled ffmpeg with libass, as captions and thumbnails are; a card is composed per frame shape, again when the video's shape changes.
