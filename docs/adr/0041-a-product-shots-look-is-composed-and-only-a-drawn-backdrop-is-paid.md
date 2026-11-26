# ADR-0041: A product shot's look is composed, and only a drawn backdrop is paid

- Status: Accepted
- Date: 2026-11-25
- Issue: #285
- Builds on: composed product shots (ADR-0040), visual styles (#259), the video's spend cap (#268), paid calls sent once (ADR-0030)

## Context

ADR-0040 composed every product shot one way: the product's image as a card with rounded corners over a plain tone of its color. That suits a photo on a plain background and nothing else. A photo whose background is already cut out looks pasted when boxed in a card, a screenshot reads better inside something that looks like the device it came from, and a plain tone gets monotonous over a whole ad. The user needs to change how a shot looks without the product ever being redrawn, and without paying for what can be composed.

## Decision

1. **A composed scene has a look.** `ShotLook` is a layout, a position and a backdrop, stored on the scene as one line (migration 0073; `NULL` is the first look, and a line that will not read is the first look too). The composed picture records the look it was made in next to its image and shape; `Scene::needs_shot` compares all three, so any change of look makes the scene wait to be composed again. A picture made before looks has none recorded and is composed again, for free.
2. **Layouts follow the image, and only ever scale it.** `ProductLayout::offered`: a photo marked cut out (it declares transparency) may be *cut out* or a *card*; a screen may be in a *device* or a *card*; any other photo is a card. The first offered is the default, and a chosen layout the image does not offer falls back to it, so changing the scene's image never leaves an impossible look. A cut-out photo with no see-through pixel is drawn as a card.
3. **The geometry stays a domain rule.** `ShotLayout::of(aspect, picture, layout, placement)` fits the product (the image plus what is drawn around it: a phone's bezel, or a browser's sides and bar) into the same room as ADR-0040, inside the strongest zoom and above the caption band, and aligns it left, center or right; the cut-out layout also keeps room for its shadow below. The same tests hold every layout and position to that room under every motion.
4. **The device outline is plain.** A phone for a screen taller than wide, a browser window otherwise: dark rounded shapes with a thin lighter rim, a bar with three dots and an empty address field for the browser. No maker's shape, logo, notch or button: nothing that names a brand.
5. **Backdrops are painted, except a drawn one.** *Auto* is ADR-0040's tone. *Plain* is one color or a gradient between two (top to bottom, left to right, diagonal) with a faint ordered grain against banding. The composer returns tones of the image (its auto tone, then a deep and a pale tone of its main colors) with each picture, stored with it, so the inspector offers swatches without decoding the image on the UI thread.
6. **A drawn backdrop is a paid job of its own, and sees nothing of the product.** `JobKind::ShotBackdrop` asks the image generator for an empty surface or set, in the video's style and shape, with no product, text, logo or person, and no reference image. The prompt names only the product's kind, from the English catalog, so nothing the user wrote about the product reaches the image model. It costs one image (`CostPurpose::ShotBackdrop`, priced as a scene image), is estimated in the inspector, checked against the provider's budget and the video's cap before it starts, held under `CostBook::paying` across the call, recorded or recorded as failed, and counted by a retry. One runs at a time per project (`Work::Backdrop`), and a job run again that already drew draws nothing.
7. **A drawn backdrop waits for review, then is free.** It is stored in `shot_backdrop` unkept. Only a kept backdrop can be in a look; keeping one can put it behind the scene it was kept from. A kept backdrop is offered to every composed scene of the project, and composing over it reads its file, covered to the frame's shape. One backdrop waits for review at a time: the next is drawn once it is kept or discarded. A backdrop a product shot shows cannot be discarded (a scene drawn again keeps its look but shows no backdrop); a discarded one loses its row and file, and the backdrop is cut to the frame's shape before it is scaled, so an odd shape costs no more than the frame. A look whose drawn backdrop is gone fails that scene's composing with a reason, and the other scenes are still made. A version of the video gets copies of the kept backdrops under ids of its own, and its scenes' looks name the copies.

## Consequences

- Every look change is free and runs in a job; the only new paid call is one image the user asks for and sees priced first.
- The product's pixels are unchanged apart from scaling in every layout, which the media tests check against the scaled source.
- Pictures composed by #278 are composed once more after the update, for free, the first time the user composes product shots or draws images.
- Not in this slice: a picture per render shape (#283), the end card's own layout (#281), the product reference for *product in use* (#279), and cutting out a photo locally (#300, gated on its check).
