# ADR-0043: An idea's concept image is drawn, reviewed and stands for a photo

- Status: Accepted
- Date: 2026-11-26
- Issue: #280
- Builds on: product images and kinds (#273), composed product shots (ADR-0040), the product in use and its check (ADR-0042), paid calls sent once (ADR-0030), text jobs named by their work (ADR-0036)

## Context

A product of kind *idea* does not exist yet, so the user has no photo of it. Without one, its ads have no product shot and no *product in use* scene: only drawn scenes and an end card of text. An image model can draw an idea from a description, but left to itself it borrows the look of existing products, brands and logos, and a concept ad that shows someone else's trade dress misleads as much as one that claims the idea is for sale.

## Decision

1. **A concept image is offered only for an idea with no image of its own.** The Products screen offers *Draw a concept* while the product is of kind *idea* and holds no image. Once it has an image of its own, or is no longer an idea, that image is used and the concept image is not, though it stays stored.
2. **Two calls, one job.** Claude writes an image prompt from the product's brief with a new template kind, *Concept prompt* (it reads `{{product}}` and `{{product_audience}}`); the image generator then draws a 2:3 picture with no references. Both calls are estimated together, checked against the budget before the job is queued, and recorded in Costs under no channel and no video. Claude's prompt is kept as a checkpoint, so a retry after it pays only for the drawing.
3. **A fixed instruction follows the template.** After the template's instructions, and again after its prompt (both are the user's to edit), Bardo always adds: an original product that does not exist, no existing product, brand, logo, trade dress, mascot or character, and no text in the picture. The job adds it again if what it reads lacks it, so no template edit takes it out. The image generator gets a short fixed sentence after Claude's prompt too: no existing brand and no text.
4. **Nothing is paid for a drawing that would not be used.** The job checks, before each paid call, that the product still exists and is still offered a concept image; one that gained an image or changed kind meanwhile stops there, and a retry of an old job does too. The product cannot be deleted while its concept is drawn.
5. **The drawing waits for review.** A new drawing is stored as *pending*; *Keep and use* makes it the product's concept image, replacing the last one in one step, and *Discard* drops it. Only a kept image is ever used.
6. **The concept image is used like a photo, and marked as drawn.** It takes a photo's place among the product's images in an ad (product shots, the scene planner's list, the reference of a *product in use* scene and its check), described to the planner as a concept image, a drawing of an idea, and labelled *Concept image* wherever it shows. The product check of a *product in use* scene drawn from it compares the scene with the concept image and says so, never calling it the real product; an imported picture can never be marked as a concept image.
7. **A project is a concept when its product starts as an idea.** The ad brief records it when the project starts; the project list and the header show a *Concept* badge, which stays whatever the product becomes later.

## Consequences

- A concept costs one Claude call and one image per drawing; keeping or discarding is free. A drawing larger than a product image's file limit is saved as a JPEG that fits.
- Migration 0075 adds the table `product_concept` (one *kept* and one *pending* row per product, deleted with it) and the column `ad_brief.concept`, filled for existing ad projects whose product is an idea.
- A duplicate of the product copies its kept concept image. Backups carry it with the database; settings files do not, like the other pictures.
- How a concept is disclosed in upload review and export is left to #282.
