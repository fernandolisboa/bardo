# ADR-0018: Video format, Short or Long, per channel and per project

- Status: Accepted
- Date: 2026-10-22
- Amends: the MVP spec, which assumed one 16:9 video of 8 to 10 minutes per project

## Context

The default flow was split between two shapes. The script template asked for 8 to 10 minutes of narration, scene prompts and every image and clip provider were called at a fixed 16:9, and the rough cut started in landscape, while YouTube's built-in render preset was a 9:16 Short capped at 3 minutes, like TikTok, Reels and X. A default video therefore came out as about 9 minutes of 16:9 whose YouTube render was blocked by the duration gate until the account's preset was overridden by hand, and a Short made on purpose cropped 16:9 pictures down to 9:16, losing most of each frame.

YouTube takes Shorts of up to 3 minutes (vertical or square) and long videos of up to 12 hours, but an account that has not verified itself can upload only up to 15 minutes. TikTok, Reels and X have one vertical shape each.

## Decision

1. **`VideoFormat` is a domain type, `Short` or `Long`.** A channel has a default format; a video project has an optional override and otherwise follows its channel, so changing the channel's format moves every project that never picked its own.
2. **New channels are Short; channels and projects from before this change are Long.** Their scripts and pictures were made for 8 to 10 minutes of 16:9, so they keep making that. A settings file without the format reads as Long for the same reason.
3. **The format drives what is made next, never what was made.** It sets:
   - the script's target length through a new `{{target_length}}` variable (Short about 110 to 150 words, 45 to 60 seconds spoken; Long about 1,200 to 1,500 words, 8 to 10 minutes);
   - the frame shape in the scene-plan instructions through `{{frame_shape}}`, and the aspect ratio each image provider is called with (Nano Banana's `imageConfig.aspectRatio`);
   - the aspect ratio of each clip, read from its first frame's header, so a clip always matches its picture whatever the format is now;
   - the shape of a new rough cut;
   - YouTube's built-in render preset: Short is 9:16, 1080×1920, up to 3 minutes; Long is 16:9, 1920×1080, 12 Mbps, up to 12 hours.
   Scripts, images, clips and cuts already made keep their shape; the user generates them again to switch.
4. **Only YouTube's preset changes with the format.** TikTok, Reels and X stay vertical and Kick stays 16:9. A long project still targets every account of its channel: a 9:16 network renders through the clips' crop windows, a 16:9 one shows the pictures whole (point 7), and a cut longer than a network takes is blocked for that network alone, as before. An account's own preset values still win over the built-in for either format.
5. **A warning, not a block, for long YouTube videos.** A cut over 15 minutes for YouTube gets the `LongForUnverified` warning pointing at YouTube's verification page; Bardo cannot see whether the account is verified.
6. **Untouched default templates are upgraded, part by part, once.** The script and image-prompt defaults gain the new variables. When the instructions or the prompt of a profile's current version are still a retired default, the next read of that template adds a version with that part moved to today's default and the other part kept. A part moves only if no version has held today's default for it yet, so an old text the user puts back by hand stays. Text the user wrote stays as it is, and the variables are optional in it.
7. **A 16:9 frame shows every picture whole.** A tall picture (a Short's scene, or vertical footage) in a 16:9 cut or file gets bars at the sides; crop windows apply to 9:16 frames only, as before.
8. **Scene cadence and caption placement do not change with the format.** Scenes still change every 4 to 8 seconds of narration, and captions are already sized and lifted from each rendered frame's own size.

## Consequences

- A new channel makes YouTube Shorts end to end with no preset override, and every network takes its render.
- Long videos are a channel or project choice, and their YouTube render is 16:9 with no Short limit.
- Migration 0045 adds `channel.video_format` (default `long` for existing rows) and a nullable `video_project.video_format`.
- A channel that was making Shorts before this change, with an edited script template and YouTube's built-in preset, becomes Long and renders 16:9 for YouTube until it is switched to Short; the render shows as out of date.
- Accounts shows each account's preset for the channel's format; the Render stage uses the project's.
- Two template variables are added; edited templates without them keep working and get no length or shape hint.
