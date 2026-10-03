# PRD: Bardo MVP

- Status: Ready for implementation
- Date: 2026-10-01
- Source of truth for scope and acceptance: [`docs/spec/mvp.md`](../spec/mvp.md) (Accepted). Decisions: ADR-0001 to ADR-0008. Vocabulary: [`docs/CONTEXT.md`](../CONTEXT.md).

## Problem Statement

Running faceless channels for high-CPM markets means juggling a dozen tools: YouTube search to guess which niche is worth it, a chat window for scripts, ElevenLabs for narration, separate sites for images and video clips, a video editor for cuts, captions and audio mix, and each network's own upload page. Nothing connects them. Prompts, voices and settings are re-typed for every video, costs are invisible until the bill arrives, and there is no feedback loop from what performed back to what to make next. Fully automatic "content farm" tools solve the juggling but produce low-quality, unoriginal videos that platforms demonetize, and they take away the creative control the creator wants.

## Solution

Bardo is a native Windows desktop app that holds the whole workflow in one place, with AI assisting at every step and the user deciding at every step. The user defines channels and personas once, researches niches with real competition and trend numbers, gets ranked theme suggestions, generates script, narration, scene images and clips step by step (reviewing and editing each), cuts and mixes them on a timeline with captions snapped to the narration, renders per-network files after a review screen, and exports or publishes them. Generation costs are tracked against a monthly budget, and metrics of published videos flow back into strategy. Everything runs locally; secrets stay in Windows Credential Manager.

Build order: creation (Strategy and Production) and Editing first, with export as the way out; network upload and owner metrics follow once creation and editing are usable end to end.

## User Stories

### Setup and settings

1. As a creator, I want to enter my provider API keys (Claude, ElevenLabs, Gemini, Higgsfield, TypeSafe, YouTube Data API) once, so that every feature can use them without asking again.
2. As a creator, I want my keys stored in Windows Credential Manager and never shown in logs or error messages, so that a shared log or screenshot never leaks them.
3. As a creator, I want to test each key from the settings screen, so that I know a provider works before a long job fails on it.
4. As a creator, I want to pick the folder where project media lives, so that large files go to the disk I choose.
5. As a creator, I want to switch the UI between Portuguese and English, so that I can use the app in my language.
6. As a creator, I want the app to start with my local profile and no login, so that I can work offline and immediately.

### Channels

7. As a creator, I want to create a channel with niche, themes, aesthetic notes, language and target country, so that every generation is aimed at the right audience.
8. As a creator, I want to set a default persona per channel, so that its videos sound consistent.
9. As a creator, I want to set the default video provider and model per channel, so that each channel keeps its visual style and budget.
10. As a creator, I want to add network accounts to a channel (one per network) with metadata defaults and render preset overrides, so that exports come out right for each network.
11. As a creator, I want to see all my channels and their recent videos at a glance, so that I know where each one stands.

### Personas

12. As a creator, I want four default personas (a documentary narrator and a storyteller in en-US and pt-BR), so that I can produce a first video without setup.
13. As a creator, I want to create and edit personas with voice, tone, script style and generation presets, so that my narrators have distinct identities.
14. As a creator, I want to pick a persona's voice from my ElevenLabs voices, including clones made there, so that I can use any voice I am entitled to.
15. As a creator, I want to see which channels use a persona before I save changes to it, so that I do not change other channels by accident.
16. As a creator, I want to duplicate a persona, so that I can make a variant without touching the original.
17. As a creator, I want to export and import a persona as a package file, so that I can share it or move it to another machine.
18. As a creator, I want an imported persona whose voice is unavailable in my account to be flagged, so that I find out before a generation fails.
19. As a creator, I want to override the persona for a single video, so that I can experiment without changing the channel.

### Strategy

20. As a creator, I want to enter seed niches or keywords and run a research job, so that I get data on which niches are worth entering.
21. As a creator, I want each niche scored on competition and trend for my channel's target country and language, so that I can compare them on equal terms.
22. As a creator, I want to see the numbers behind each score (upload volume, median views, channel size spread, view velocity) and when they were fetched, so that I can judge the score myself.
23. As a creator, I want research results cached, so that re-running research does not burn YouTube API quota.
24. As a creator, I want to refresh a cached result on demand, so that I can get fresh data when I need it.
25. As a creator, I want Claude to propose themes for a niche and channel, so that I start from ideas rather than a blank page.
26. As a creator, I want the decision engine to rank proposed themes with typed reasons (competition, trend, channel fit, past performance), so that I see why one idea beats another.
27. As a creator, I want to pick, edit or discard each suggested theme, so that the final idea is mine.
28. As a creator, I want to start a video project from an approved theme, so that strategy flows into production without re-typing.

### Production

29. As a creator, I want to generate a script from niche, theme, persona and channel aesthetic, so that the script fits the channel.
30. As a creator, I want to edit the script before narration, so that I control what is said.
31. As a creator, I want to generate the narration with the persona's voice from the approved script, so that the voice matches the channel.
32. As a creator, I want word timings to come with the generated narration, so that captions and cut snapping work immediately.
33. As a creator, I want to import my own recorded narration and get word timings for it, so that I can narrate some videos myself.
34. As a creator, I want Claude to plan scenes and write one image or video prompt per scene, so that visuals follow the narration.
35. As a creator, I want to edit each scene's prompt before generating, so that I control the visuals.
36. As a creator, I want to generate scene images with Nano Banana, so that each scene has a visual.
37. As a creator, I want to animate a scene into a video clip with Higgsfield or Google (Veo, Gemini Omni Flash), so that I can choose the model that looks best.
38. As a creator, I want to override the video provider and model per scene, so that I spend more only where it matters.
39. As a creator, I want to see the estimated cost of a generation before starting it, so that I never spend by surprise.
40. As a creator, I want a music prompt generated and to import the music file myself, so that I can use music I have rights to.
41. As a creator, I want to import my own SFX and other media into a project, so that I can mix generated and own material.
42. As a creator, I want to regenerate any single asset without redoing the others, so that one bad image does not cost the whole video.
43. As a creator, I want every asset to record provider, model, prompt, template version and cost, so that I can reproduce or audit any result.
44. As a creator, I want a provider failure on one asset to leave the others intact and the job resumable, so that outages do not lose work.
45. As a creator, I want versioned, editable templates for script, title, description, image prompt, video prompt, narration direction and music prompt, so that I can improve my prompts over time.

### Jobs

46. As a creator, I want every long task to run as a job with progress, so that the app never freezes.
47. As a creator, I want to cancel a job, so that I can stop a mistake quickly.
48. As a creator, I want jobs to resume after I close and reopen the app, so that a restart does not lose progress.
49. As a creator, I want failed jobs to retry with backoff and then show a clear error, so that transient failures fix themselves and real ones are visible.
50. As a creator, I want a jobs panel showing everything running, queued, failed and done, so that I know what the app is doing.

### Cost and budget

51. As a creator, I want every generation's cost recorded, as reported by the provider or estimated from its rate, so that I know what each video cost.
52. As a creator, I want to see spend per video, per channel and per month, so that I can compare cost to revenue.
53. As a creator, I want to set a monthly budget per provider, so that I cap my spending.
54. As a creator, I want a warning at 80% of a budget, so that I can slow down in time.
55. As a creator, I want new jobs for a provider to ask for confirmation once its budget is reached, so that a regeneration loop or an expensive model cannot overspend silently.

### Editing

56. As a creator, I want a timeline with one video track and three audio tracks (narration, music, SFX), so that I can assemble the video.
57. As a creator, I want the timeline pre-filled from the scene plan and narration, so that I start from a rough cut instead of an empty timeline.
58. As a creator, I want to split, trim, reorder and delete clips, so that I control the cut.
59. As a creator, I want cuts to snap to narration words, so that edits land cleanly on speech.
60. As a creator, I want the decision engine to score candidate cut points and to accept or reject each one, so that AI speeds up cutting without taking it over.
61. As a creator, I want per-track gain, mute and solo, so that I can balance the mix.
62. As a creator, I want fade in and out per clip, so that audio does not pop.
63. As a creator, I want music to duck under narration with an adjustable amount, so that the voice stays clear.
64. As a creator, I want captions generated from the word timings, so that I do not caption by hand.
65. As a creator, I want to edit caption text and timing, so that I can fix any mistake.
66. As a creator, I want a few burned-in caption styles per channel, so that each channel has its look.
67. As a creator, I want 9:16 and 16:9 framing with per-clip crop position, so that the same assets fit Shorts and long format.
68. As a creator, I want preview playback from low-resolution proxies starting within one second, so that editing feels immediate.
69. As a creator, I want timeline edits never to block the UI, so that I can keep working while proxies build.
70. As a creator, I want undo and redo for timeline edits, so that I can experiment safely.

### Render and export

71. As a creator, I want render presets per network (aspect, resolution, codec, bitrate, duration limit, loudness target), so that files meet each network's rules.
72. As a creator, I want a review screen before the final render showing duration, resolution, loudness, captions on or off and target networks, so that I catch problems before spending render time.
73. As a creator, I want the decision engine's quality gates to flag problems on the review screen (duration over limit, loudness off target), so that I do not export a broken file.
74. As a creator, I want the final render to use hardware encoding when available, so that renders are fast.
75. As a creator, I want to cancel and resume a render, so that a long render does not hold me hostage.
76. As a creator, I want to export a ready-to-post package for each of the five networks (file in the network preset plus a metadata text file), so that I can post anywhere by hand.
77. As a creator, I want Claude to generate per-network title, description and tags, editable, so that metadata is quick but mine.
78. As a creator, I want the export to include the synthetic-content disclosure reminder when a realistic synthetic voice of a real person is used, so that I disclose it when posting by hand.

### Manual publications and metrics

79. As a creator, I want to mark an export as posted and paste the post URL, so that Bardo knows the video is live.
80. As a creator, I want public YouTube statistics (views, likes, comments) synced for my YouTube publications, so that I see performance without leaving the app.
81. As a creator, I want metrics shown per video and per channel over time, so that I see what works.
82. As a creator, I want past performance to feed theme ranking, so that suggestions improve with my own data.

### Publishing (after creation and editing are usable)

83. As a creator, I want to connect YouTube, TikTok and Instagram accounts with OAuth through apps I register myself, entering each app's credentials once in Settings, so that Bardo can upload for me without shipping anyone's secrets (ADR-0008).
84. As a creator, I want a review screen before any upload with file, metadata, account, visibility, schedule and disclosure, so that nothing goes out by accident.
85. As a creator, I want uploads to resume without re-sending finished chunks, so that a dropped connection does not restart a large upload.
86. As a creator, I want YouTube scheduled publications to use native scheduling, so that they go live with my PC off.
87. As a creator, I want Instagram scheduled posts to go out while the app is open and missed ones to run after I confirm on next launch, and TikTok videos sent to my TikTok inbox as drafts that I finish and schedule in the TikTok app, so that I stay in control (ADR-0008).
88. As a creator, I want a clear status when a network restricts my posts to private because my app is unaudited, so that I am not confused by invisible videos.
89. As a creator, I want each network's posting limits (Instagram's publishing limit as the API reports it, TikTok's pending drafts, YouTube's daily uploads) enforced before queuing, so that I do not hit API errors.
90. As a creator, I want owner metrics (YouTube revenue, CPM, RPM, retention; TikTok and Instagram metrics) once accounts are connected, so that I can measure money, not just views.

## Implementation Decisions

- **Workspace and boundaries (ADR-0001).** Rust workspace with seven crates: `domain`, `media`, `ai`, `publish`, `storage`, `app`, `ui`. Only `ui` imports GPUI, enforced by a CI dependency lint from the first commit. GPUI version pinned and its docs consulted before use.
- **Domain model (ADR-0005).** User profile owns personas and channels; channel has a default persona and one network account per network; video project has an optional persona override, assets, a timeline, per-network metadata and publications. Publications have a kind (uploaded or manual) and a status. Every entity carries the owning profile so more profiles can exist later without migration.
- **Provider interfaces in `domain`, adapters in `ai`/`publish`.** Deep, narrow interfaces, each hiding its provider's protocol:
  - Text generation (Claude): generate from a template version plus variables; returns text, usage and cost.
  - Voice: synthesize script text with a voice reference; returns audio plus word timings. Also lists the account's voices.
  - Alignment: given audio and optional known text, return word timings (forced alignment when text is known, speech-to-text otherwise). Off the critical path.
  - Image generation (Nano Banana).
  - Video generation: submit image and/or prompt with provider and model, get a provider job handle; poll or wait; download result. Adapters: Higgsfield first, Google (Veo 3.1, Gemini Omni Flash) second. Both are async on the provider side, so the interface is submit/poll, not a blocking call.
  - Decision engine: three typed question kinds (choice, score, yes/no) evaluated against a state; answers carry probabilities and confidence. JEV adapter (TypeSafe HTTP API) in the MVP; Laya (local ONNX) evaluated right after.
  - Market data: search recent uploads for a niche in a country and language; returns raw statistics. Scoring (competition, trend) is pure domain logic over those statistics.
  - Network metrics: fetch statistics for a publication. Public YouTube stats need only the API key.
  - Network publishing: connect (OAuth with PKCE over loopback, ADR-0008), upload with resume, schedule where the network supports it, status.
- **Cost.** Every adapter call that costs money returns a cost record (reported or estimated from a per-model rate table). Budget checks happen in `app` before a job is enqueued: under 80% runs, 80–100% warns, at or over 100% requires confirmation.
- **Job queue in `app`, persisted in `storage`.** Jobs have type, payload, state (queued, running, paused, failed, cancelled, done), progress, attempt count and checkpoint. Retry with exponential backoff; resume from checkpoint after restart. Long provider jobs persist the provider's job handle so a restart polls instead of resubmitting (no double charge).
- **Storage.** SQLite with versioned migrations. Project media in the user-chosen folder; the database stores paths and metadata, never secrets. Same database will later be shared with the background agent (ADR-0006), so writes go through transactions with locking in mind.
- **Secrets.** Windows Credential Manager only, behind a secrets interface; a redaction layer scrubs known secrets from logs and errors. Network app credentials are kept per user profile; OAuth tokens per network account (ADR-0008).
- **Media.** ffmpeg bundled and pinned. `media` exposes probe, proxy generation, preview frames/playback source and render from a timeline description. NVENC (H.264/HEVC) when present, software fallback. Loudness measured and normalized to the preset target.
- **Timeline model in `domain`.** Pure data and operations (split, trim, move, delete, gain, fades, ducking, captions, crop), with snapping to word timings and undo/redo as a command history. `media` turns a timeline into ffmpeg work; `ui` only renders `app` state.
- **Captions** derive from word timings plus user edits; styles are channel settings.
- **Market research cache.** Results stored with fetch date per niche, country and language; re-running within the cache window reads from SQLite.
- **i18n.** pt-BR and en-US resource files from the first UI screen; no hard-coded strings.
- **Reference machine.** Ryzen 9 9950X3D, RTX 3080 Ti, 32 GB DDR5-5600, NVMe. Performance targets measured there.

## Testing Decisions

- **Good tests check external behavior through the highest seam available**, not internals: given inputs at a module's interface, assert outputs and persisted state. Refactors must not break tests.
- **Seams, highest first:**
  1. `app` use cases and state: the primary seam. UI behavior is tested here (e.g. "approving a script enqueues narration", "budget at 100% requires confirmation"), with provider interfaces replaced by in-memory fakes. No pixel tests.
  2. `domain`: unit tests, test-first, for scoring, timeline operations, snapping, caption derivation, budget rules, persona rules.
  3. Adapters (`ai`, `publish`, market data): integration tests against recorded HTTP fixtures; no live calls in CI. A manual opt-in suite can hit live APIs with local keys.
  4. `storage`: tests against a temporary SQLite database, including migrations from every previous version.
  5. `media`: tests with short clips versioned in the repo; assert duration, resolution, codec and integrated loudness of outputs.
- **Prior art:** none yet (no code). The first slices establish the patterns above, and later slices copy them.
- **CI:** fmt, clippy, tests, dependency lint, on Windows.

## Out of Scope

- Data sources other than YouTube for trend and competition; any pre-upload CPM table.
- Automatic channel creation on networks.
- In-app voice cloning; music generation API.
- Video providers other than Higgsfield and Google.
- Transitions beyond cuts and crossfades, keyframed effects, color grading, multiple video tracks, picture-in-picture.
- Upload to X and Kick (export only).
- Background scheduling agent (ships right after the MVP).
- Laya adapter (spike right after the MVP).
- Multiple user profiles in the UI, login, any server component.

## Further Notes

- Platform actions for the owner (Google Cloud project and YouTube audit, TikTok Content Posting audit, Meta app for Instagram) are deferred until creation and editing are usable (ADR-0002, ADR-0003).
- The biggest technical risks are ffmpeg proxy/preview/render and the GPUI timeline; both start early as spikes that feed the editing slices.
- Higgsfield's API launched 2026-09-16; its contract may move, which the adapter boundary contains.
