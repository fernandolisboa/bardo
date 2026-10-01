# MVP spec

- Status: Accepted (owner review closed 2026-10-01)
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
- Theme suggestions: Claude proposes themes for a niche and channel; the decision engine ranks them with typed reasons (competition, trend, fit with channel, past performance when available). The user picks, edits or discards.
- Manual publications: after posting an export by hand, the user marks it as posted and pastes the post URL. This creates a publication without an upload.
- Metrics tracking (ADR-0004), in two steps:
  1. Public YouTube statistics (views, likes, comments) for every YouTube publication, manual or uploaded, through the Data API key. No OAuth; available during the creation phase.
  2. Owner metrics that need the network account's sign-in (YouTube Analytics revenue/CPM/RPM, retention; TikTok and Instagram metrics), shipped with the publishing work at the end of the MVP or right after it.

**Not in the MVP**
- Data sources other than YouTube for trend and competition.
- Automatic channel creation on networks.

**Acceptance**
- Given seed keywords, the user sees a ranked niche list with the numbers behind each score and when they were fetched.
- Re-running research within the cache window costs no API quota.
- A YouTube video posted by hand and linked by URL shows views, likes and comments after the next sync.
- Once owner metrics land, a monetized channel's videos also show estimated revenue/CPM/RPM.

## Pillar 2: Production

**In the MVP**
- Personas (ADR-0005): library with four default personas, two per language (en-US, pt-BR): a sober documentary narrator and a dramatic storyteller, one male and one female voice, using ElevenLabs default voices available to every account; create/edit; pick the voice from the user's ElevenLabs voices (including clones made there); export/import persona packages.
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
- AI suggestions: the decision engine scores candidate cut points from the script and word timings; the user accepts or rejects each.
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
- Generation cost: every generation records its cost (as reported by the provider, else estimated from the published rate). Spend is shown per video, per channel and per month. The user sets a monthly budget per provider: a warning at 80%, and at 100% new jobs for that provider need explicit confirmation instead of starting.
- ffmpeg bundled with the app; version pinned.
- i18n: pt-BR and en-US resource files; no hard-coded UI strings.
- Dependency lint: only `ui` depends on GPUI.
- Decision engine: one domain interface with three typed questions (choice, score, yes/no), each answer with probabilities and confidence. The MVP adapter is JEV (TypeSafe HTTP API). Right after the MVP, a spike compares Laya (open weights, run locally through ONNX, no Python) against JEV on Bardo's own tasks and in pt-BR; if Laya wins it becomes the default and decisions run offline. Laya's short context means long scripts are scored in chunks.

## Reference machine

Performance targets are measured on the owner's PC: AMD Ryzen 9 9950X3D, NVIDIA RTX 3080 Ti, 32 GB DDR5-5600 (one module, single channel), NVMe system and project disk, plus SATA SSD and HDD. Hardware encode uses NVENC (H.264/HEVC; this GPU has no AV1 encode) with a software fallback for machines without it. Planned upgrades (a second identical RAM module, an RTX 50-series GPU) will be recorded here when they happen; targets are set on the current configuration.

## Implementation plan

The PRD ([`docs/prd/mvp.md`](../prd/mvp.md)) turns this spec into user stories, and GitHub Issues break it into vertical slices. Creation and editing come first. Publishing work (network adapters, OAuth, platform audit requests) starts only once creation and editing are usable end to end (owner decision, 2026-09-30). Until then, export covers getting a video out.
