# ADR-0022: Retention over the cut, lined up by the cut's timing

- Status: Accepted
- Date: 2026-10-22
- Amends: ADR-0012 (what Bardo shows from YouTube), the metrics screens of the MVP spec

## Context

The metrics sync reads a YouTube post's audience retention curve through the connected account: the share of viewers still watching at each hundredth of the video (`elapsedVideoTimeRatio`, `audienceWatchRatio`). Bardo showed it as bars on the Performance screen, so the user saw *when* viewers left but had to find, by hand, *what* was on screen and being said at that moment. The editor knows both, but only for the cut as it is now, while the curve describes the file that was published. TikTok reports no retention, Instagram only the average watch time, X and Kick nothing.

## Decision

1. **YouTube only, the owner's own curve.** The project's YouTube post (a project has at most one per network) with a curve read by the last sync. Without one, the editor's toggle is disabled with a hint and the Performance list says why.
2. **Drops are computed, never stored.** A step between two points is steep when it loses at least twice the curve's average loss per step and at least half a percentage point (`STEEP_TIMES`, `MIN_STEP_LOSS`). Adjacent steep steps make one drop. The five largest by points lost are kept and listed in time order. Rises (rewatched parts) are not marked. The thresholds are relative to the video's own curve, so a Short and a long video are each judged against themselves.
3. **A render records the cut's timing.** The timing is a fingerprint of where each picture clip and narration piece sits, and which source it shows, with contiguous pieces of one source merged so a split keeps it. It changes with trims, moves and new narration; the mix, captions, framing and motion on stills keep it. Bardo keeps, per rendered file, its length, shape, full cut fingerprint and timing for as long as the project's render, a post or an export refers to it, so a later render does not lose what an earlier post was cut from.
4. **Lined up only when the timing matches.** When the post's file has a timing, the curve lines up with the editor while the current timeline has the same timing. A file rendered before this change has none and lines up only while the cut's full fingerprint for its shape is unchanged. When lined up, each drop names its span in the video, the scene playing in its middle and the sentence spoken there. When not, the drops are given by their times in the published video, with a note that the cut changed. When the file's record is gone, the drops are given as shares of the video.
5. **Where it shows.** The editor has a **Retention** toggle next to the cut suggestions (one turns the other off; off by default, not saved). When on and lined up, a row under the ruler draws the curve with the drops shaded and numbered, and a list of the drops takes the inspector's place. Clicking a drop puts the playhead at its start. On Performance and the Publish stage, the retention bars tint each drop, and **Where viewers leave** lists them, each with **Show in editor**.
6. **Policy.** Nothing new is read from or kept about YouTube. The drops come from the curve the sync already keeps under ADR-0012, so its retention, disconnection and *Delete YouTube data* rules cover them: deleting the curve removes the drops. No new consent version.

## Consequences

- Migration 0049 adds the rendered-cut table, backfilled from the current renders with no timing.
- A post rendered before 0049 whose project was cut again cannot be lined up; its drops show by time only.
- Comparing curves across posts, other networks' watch time and suggestions from drops are out of scope.
