# MVP spec

- Status: Draft, pending owner review
- Date: 2026-09-30
- Decisions: [ADR-0001](../adr/0001-architecture-and-stack.md) to [ADR-0006](../adr/0006-scheduling.md)

## Goal

One user can go from "which niche?" to a published Short on YouTube, TikTok and Instagram Reels (and a ready export for X and Kick) inside Bardo, with every creative and irreversible step under manual control.

## Principles that shape every feature

- AI suggests, the user decides. Every generated artifact is editable before it is used.
- Irreversible actions (final render, upload, publish) show a review screen and need an explicit confirmation.
- Originality: script, narration and editing are produced per video. No reupload or compilation features.
- Nothing blocks the UI; long work is a job with progress, cancel and resume.
- UI strings in pt-BR and en-US from day one.

## Pillar 1: Strategy

**In the MVP**
- Channels: create/edit with niche, themes, aesthetic notes, language, target country, default persona and network accounts.
- Niche research: the user enters seed niches or keywords; a job queries the YouTube Data API and computes, per niche, competition (recent upload volume, median views, channel size spread) and trend (view velocity of recent uploads). Results are cached with their fetch date.
- Theme suggestions: Claude proposes themes for a niche and channel; JEV ranks them with typed reasons (competition, trend, fit with channel, past performance when available). The user picks, edits or discards.
- Metrics tracking: a sync job pulls metrics snapshots for each publication (see ADR-0004) and shows them per video and per channel.

**Not in the MVP**
- Data sources other than YouTube for trend and competition.
- Automatic channel creation on networks.

**Acceptance**
- Given seed keywords, the user sees a ranked niche list with the numbers behind each score and when they were fetched.
- Re-running research within the cache window costs no API quota.
- A published YouTube video shows views and, for a monetized channel, estimated revenue/CPM/RPM after the next sync.

## Pillar 2: Production

**In the MVP**
- Personas (ADR-0005): library with default personas; create/edit; pick the voice from the user's ElevenLabs voices (including clones made there); export/import persona packages.
- Templates: versioned, editable templates for script, title, description, image prompt, video prompt, narration direction and music prompt. Each generation records the template version used.
- Generation, each step reviewable and editable before the next:
  1. Script (Claude) from niche + theme + persona + channel aesthetic.
  2. Narration (ElevenLabs) from the approved script with the persona's voice. The same call returns the word timings of the script text, so generated narration needs no speech-to-text.
  3. Scene plan and prompts (Claude), one per scene.
  4. Images (Nano Banana) and video clips through two video adapters: Higgsfield first (it aggregates Kling, Seedance, Wan, MiniMax and its own models), then Google through the Gemini API (Veo 3.1 and Gemini Omni Flash, sharing the Nano Banana key). Provider and model are a setting per channel with a per-scene override, and the scene screen shows the estimated cost before generating.
  5. Music: prompt generated; audio imported by the user.
- Imported narration (e.g. the user's own recording) gets word timings from the alignment adapter: forced alignment when the script text is known, speech-to-text with timestamps otherwise. Not on the critical path; can land late in the MVP.
- Regenerate any single asset without redoing the rest.

**Not in the MVP**
- In-app voice cloning (done in the provider's own flow).
- Video providers other than Higgsfield and Google (e.g. Kling direct); the adapter interface allows adding them later.
- Music generation API.

**Acceptance**
- From an approved theme, the user reaches a project with script, narration with word timings and one asset per scene, editing any of them in between.
- Every generated asset records provider, model, prompt and template version.
- A provider failure on one asset leaves the rest intact and the job resumable.

## Pillar 3: Editing

**In the MVP**
- Timeline with one video track and three audio tracks (narration, music, SFX).
- Manual cuts: split, trim, reorder, delete, with snapping to narration words.
- Audio: per-track gain, mute/solo, fade in/out, ducking of music under narration with an adjustable amount.
- Captions from the word timings: editable text and timing, a few burned-in styles per channel.
- Framing: 9:16 and 16:9 with per-clip crop/reframe position.
- Preview playback from low-resolution proxies.
- AI suggestions: JEV scores candidate cut points from the script and word timings; the user accepts or rejects each.
- Render presets per network; final render as a job after a review screen (duration, resolution, loudness, captions on/off, target networks).

**Not in the MVP**
- Transitions beyond cuts and crossfades, keyframed effects, color grading.
- Multiple video tracks / picture-in-picture.

**Acceptance**
- Timeline edits never block the UI; preview starts within one second on a 60 s project on the reference machine (target to confirm after the ffmpeg spike).
- A render matches the preset's resolution, codec and duration limit, and integrated loudness is within the preset target.
- Render can be cancelled and resumed.

## Pillar 4: Publishing

**In the MVP**
- Per-network metadata (title, description, tags, language, visibility) with defaults from channel and network account, generated by Claude and editable.
- Review screen before any upload: file, metadata, target account, visibility, schedule, synthetic-content disclosure.
- Upload adapters for YouTube, TikTok and Instagram Reels, with resumable uploads in the job queue.
- Scheduling per ADR-0006: YouTube native `publishAt`; TikTok and Instagram via the in-app scheduler.
- Export for all five networks: file in the network preset plus a metadata text file.
- Clear status when a network restricts posts to private (unaudited app).
- Instagram 50 posts / 24 h limit enforced before queuing.

**Not in the MVP**
- Upload to X and Kick (export only).
- Background agent for scheduling (right after the MVP).

**Acceptance**
- No upload starts without passing the review screen.
- A failed or interrupted upload resumes without re-sending completed chunks where the API supports it.
- A YouTube scheduled publication goes live with the app closed.

## Cross-cutting

- Storage: SQLite with versioned migrations; project media in a user-chosen folder.
- Secrets: API keys and OAuth tokens in Windows Credential Manager only; redacted from logs and errors.
- Job queue: persistent, with progress, cancel, retry with backoff and resume after restart.
- ffmpeg bundled with the app; version pinned.
- i18n: pt-BR and en-US resource files; no hard-coded UI strings.
- Dependency lint: only `ui` depends on GPUI.

## Reference machine

Performance targets are measured on the owner's PC: AMD Ryzen 9 9950X3D, NVIDIA RTX 3080 Ti, 32 GB DDR5-5600 (one module, single channel), NVMe system and project disk, plus SATA SSD and HDD. Hardware encode uses NVENC (H.264/HEVC; this GPU has no AV1 encode) with a software fallback for machines without it.

## Open questions (defaults in use until decided)

| Question | Default |
| --- | --- |
| Default personas shipped with the app | Two per language (en-US, pt-BR) using provider stock voices. |

## First tickets, in order

1. Workspace skeleton with the seven crates, CI (fmt, clippy, tests) and the GPUI dependency lint.
2. Domain model for user profile, channel, persona, network account, video project; SQLite schema and migrations.
3. Job queue with progress, cancel, resume and persistence.
4. Secrets store over Windows Credential Manager.
5. ffmpeg spike: proxy generation, preview frames, render of a two-clip, two-track timeline. Biggest technical risk; start early.
6. GPUI spike (after Context7 docs check): timeline view driven by `app` state.

Publishing work (network adapters, OAuth, platform audit requests) starts only once creation and editing are at least usable end to end (owner decision, 2026-09-30). Until then, export covers getting a video out.
