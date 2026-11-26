# ADR-0042: The product in use is drawn with its photo and checked by the user

- Status: Accepted
- Date: 2026-11-26
- Issue: #279
- Builds on: scene kinds and composed product shots (ADR-0040), the storyboard's references (#258), paid calls sent once (ADR-0030)

## Context

An ad's *product in use* scene (ADR-0040) shows the product being used, so it has to be drawn: no composed picture shows a hand holding the product. Drawn from its prompt alone, the image model invents a product that only resembles the real one, and an ad that shows another label or shape misleads. A model can be steered with a reference image, but none promises a faithful copy, and nothing Bardo can compute says reliably whether two pictures show the same product. Animating such a picture then risks the product changing shape from frame to frame.

## Decision

1. **The product's photo is a reference of the scene's request.** A *product in use* scene is drawn with the product's first **Photo**, copied once into the project folder, right after the cast portraits and before the other shots of the storyboard (place, previous shot, look reference), within the image request's limit. The prompt numbers it like the other references, gives how the product looks from its sheet, and adds a fixed sentence: keep the product's shape, colors, label and proportions exactly as in the reference. Without the photo (none, unreadable, no room) the scene is not drawn: it would be paid for and show another product.
2. **A screen never goes to a model.** Only a photo is sent. A product whose images are all screens has no *product in use* kind (ADR-0040), and the fixed ad rules tell the planner that a screen in a drawn picture is unreadable.
3. **The check is the user's.** A scene records whether its image, and its image waiting for review, were drawn with the product, and which image the user checked, by its generation. Bardo does not score likeness. The inspector shows the picture beside the real photo with three choices: *It matches* (the check), *Draw again* (paid, with its estimate) and *Use the real image instead* (the scene becomes a product shot, composed for free). A picture drawn without the product (the scene was drawn before it showed the product) counts as missing: drawing it is paid, so it waits for the user's *Generate N images*.
4. **Clips wait for the check.** A *product in use* scene is animated only once its picture is checked; a clip job that reaches one without the check fails that scene with the reason, and the others go on. Its clip prompt starts with a fixed sentence: the product stays rigid and unchanged, and only the camera or the person moves. Its clip has a check of its own, recorded by the clip's generation; until then the cut plays the checked still.
5. **The render waits for every check.** The render review lists the *product in use* scenes whose picture is not checked, offers no target, and starting a render is refused until each is checked or made a product shot. The check is read again when the render starts.
6. **A check names what it was made on.** A new image used (drawn again and accepted) or a new clip used is not checked; a check never carries over. Unchecked scenes hold back only their own clip and the render, never the scenes around them.
7. **Every other scene is as before.** Drawn scenes, product shots, end cards and every scene of a video that is no ad need no check and are drawn, animated and rendered exactly as before.

## Consequences

- The product in use costs at least one scene image, and a redraw each time the user turns a picture down; a check costs nothing.
- The stored scene gains one column, `product_checks` (migration 0074): two flags and two generation ids in one line. A line that will not read holds nothing, so the scene is drawn and checked again rather than shown unchecked.
- A product image re-marked as a screen after drawing is not sent again; scenes drawn with it keep their picture until drawn again (follow-up #320 covers how image kinds and transparency are stored).
