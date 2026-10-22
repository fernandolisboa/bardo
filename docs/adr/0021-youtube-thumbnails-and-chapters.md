# ADR-0021: YouTube thumbnails and chapters, composed and checked in Bardo

- Status: Accepted
- Date: 2026-10-22
- Amends: the MVP spec's YouTube upload, which sent the video and its metadata only

## Context

A long YouTube video lives or dies by its thumbnail, and chapters help viewers and search. Bardo uploaded the video with its title, description and tags, so the user had to finish both in YouTube Studio. YouTube takes a custom thumbnail through `thumbnails.set` (JPEG or PNG, 16:9, up to 2 MB on mobile, 50 quota units), only from a channel verified for custom thumbnails; it reads chapters from timestamps in the description, and shows them only when there are at least three, the first at 0:00, each at least 10 seconds, in ascending order. A Short takes its thumbnail from a frame (a custom one is set only in YouTube Studio on a computer) and has no chapters.

## Decision

1. **Long videos on YouTube only.** The thumbnail and chapters cards appear in the YouTube inspector of a project in the Long format (ADR-0018). A Short's card says why there is nothing to set. Other networks pick their cover their own way and get neither.
2. **Bardo draws the text, not the image model.** A thumbnail is a background plus a short text (up to 40 characters) at one of six places, in one of three looks (Bold, Yellow, Banner), composed by ffmpeg with libass and the bundled fonts into a 1280 × 720 JPEG. Image models misspell text and cannot be edited after the fact; drawing it ourselves keeps the text exact, editable and in the channel's fonts. The background is scaled to cover the frame and centred.
3. **Backgrounds are a strip, never replaced.** The background is any scene image of the project (free) or one the image provider drew from an editable prompt (the channel's image model, 16:9, with the cost estimate and budget question of scene images). Every drawn background is kept; drawing again adds one and never changes the background in use.
4. **Claude writes, the user decides.** A new "Thumbnail" template writes the text (a hook that adds to the title) and the background prompt in one call; a new "Chapters" template takes the narration's sentences with their times and answers with the sentence each chapter starts at and a title. Both are ordinary versioned templates, with their cost shown before the call.
5. **The saved picture is named after its design.** Saving composes the picture off the UI thread to `thumbnail-<fingerprint>.jpg`, the fingerprint covering the text, placement, look and background. A design changed since shows as not saved, and an upload or export always carries the picture the user saw. The upload review's and the export's stamps include the file, so saving a new picture after either asks for a new review or marks the export out of date.
6. **A thumbnail never fails an upload.** The upload job calls `thumbnails.set` once, right after `videos.insert` returns the video and before it waits for processing, and records the outcome on the publication: set, not allowed (403 `forbidden`, the channel is not verified) or failed with YouTube's reason. The video stays up in every case, and the user can set the picture in YouTube Studio from the export. A resumed job does not send it again.
7. **Chapters are anchored to the narration.** A chapter keeps its start in narration-file time and its title. Its time in the video comes from the cut as it is now, through the same narration-to-cut mapping captions use, so trims move chapters with the cut. A narration made after the chapters were written marks them out of date.
8. **Rules are checked, never blocking.** The first chapter always reads 0:00. Chapters that break YouTube's rules (fewer than three, one under 10 seconds, a start the cut removed) are left out of the description with a warning that says why; they never block an export or upload, since YouTube would ignore them anyway. When included, they go between the description and the account's footer and count toward the 5,000-character limit; chapters that would push the description over it, or put a `<` or `>` in it, are left out the same way, so they never turn a valid post into a blocked one.
9. **Per channel look, nothing existing changes.** Each channel has a default thumbnail look (Bold for new and existing channels: thumbnails are new, so no render or upload changes).

## Consequences

- Migration 0048 adds the channel's look, the project's thumbnail, drawn backgrounds, chapters, the two template kinds and the publication's thumbnail outcome.
- Uploads of long videos with a saved thumbnail spend 50 more quota units.
- Custom thumbnails need a verified channel; Bardo cannot verify it and says where to do so when YouTube refuses.
- Thumbnail A/B tests, thumbnails for Shorts, chapter titles in other languages and thumbnails for other networks are out of scope.
