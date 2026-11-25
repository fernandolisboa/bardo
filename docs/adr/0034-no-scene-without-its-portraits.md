# ADR-0034: A scene is not drawn without the portrait of everyone in it

- Status: Accepted
- Date: 2026-11-25
- Issue: #269
- Builds on: the cast and its reference portraits (#246), the storyboard plan and reference shots (ADR-0029), visual styles (ADR-0032), a paid call is sent once (ADR-0030)

## Context

A recurring person keeps one face across a video because every scene that shows them is drawn with their reference portrait. Two things undid that silently, and both were paid for:

- **A portrait that failed was skipped.** When the provider declined a portrait, or its file would not read, the scenes that show the person were drawn anyway from the description alone. Each was a paid image with a different face, and nothing on the screen said so.
- **A portrait drawn again left the old images as they were.** Editing a person's look or drawing the portrait again changed the face for the next images only. The images already drawn kept the old face with no mark, and the user animated them (clips cost more than images) before noticing.

## Decision

1. **No portrait, no scene.** An image job does not send a scene whose cast member has no portrait in the video's visual style: the portrait failed, will not read, or does not fit in the request. The scene is failed as `portrait_missing`, costs nothing, and the job goes on with the other scenes; its failure lists the scenes that wait, under the people whose portrait is missing.
2. **Going without a portrait is the user's choice, per person.** A cast member can be set **described only**: no portrait is drawn, sent or counted for them, and their scenes are drawn from the description. It is offered for every member and taken back from the same place. A failed portrait never leads there by itself.
3. **Each image records the portraits it was drawn with**: per cast member shown, the portrait's generation, or none. The record is written from the same read of the plan the request was built from, moves with the image when a new one is accepted, and is copied with a version. An image from before the record has none, which is not the same as an image that shows nobody.
4. **An image that does not match is marked, not redrawn.** A scene's image has an **old portrait** when a member it shows has another portrait now, has none, or is not in the record. For an image from before the record, a portrait drawn after the image tells, and so does having none. Marked scenes read "Older face" on both the Scenes and Clips stages and are counted apart, with what drawing them again costs; they are not missing images, so the stage's main button leaves them alone. Drawing them again is one job the user starts, and each new image waits for review.
5. **An image with an old portrait is no reference shot.** Sent beside the new portrait, it would pull the next image back to the face being replaced, as an image in another style would pull it back to that style (ADR-0032).
6. **A portrait that will not read is let go.** Its record is dropped with the reason, so the person shows as having none, estimates count a new one, and the next run draws it. The run that found it does not: what it was started for did not count that portrait.
7. **No portrait is bought for a scene that is already stopped.** Once one portrait a scene needs has failed in a run, the other portraits that scene needs are not drawn for it in that run.
8. **A wait ends when its cause does.** A scene failed as `portrait_missing` goes back to its state before (drawn, or to draw) when everyone it shows has a portrait or is described only.

## Consequences

- A declined portrait now costs one request instead of one per scene that shows the person, and the user sees which scenes wait and why.
- Projects from before the record get marks only where the portraits there are now tell: a later portrait, or none. An old image drawn with a portrait that is still current is not marked, which is right.
- A portrait past the request's size limit also stops its scene. That needs the media engine to fail on portraits of several megabytes each, and choosing described only for one person is the way out.
- ADR-0029 item 10 changes for portraits: a reference shot past the size limit is still left out, a portrait is not.
- The record is one text column per image slot on `scene` and a flag on `scene_cast`. A value that does not read is taken as not recorded, so a damaged row cannot keep a plan from loading.
