# Domain vocabulary

Shared language for specs, code and reviews. When a term here and a name in code disagree, one of them is wrong.

| Term | Meaning |
| --- | --- |
| **User profile** | The local owner of all data. Single user today; modeled so more profiles can exist later without a login system. |
| **Channel** | A brand: niche, themes, aesthetic, language, target country and a default persona. Owns one or more network accounts. |
| **Network** | A social platform: YouTube, TikTok, Instagram Reels, X, Kick. |
| **Network account** | A channel's profile on one network: credentials (in Windows Credential Manager), metadata defaults and render preset overrides. |
| **Persona** | A reusable narrator identity owned by the user profile: voice reference, tone, script style and generation presets. Assigned to a channel as default and overridable per video. |
| **Voice reference** | A pointer to a voice held by a voice provider (e.g. an ElevenLabs voice id). The voice itself never lives in Bardo. |
| **Niche** | A content market (e.g. "space history"). Scored for competition and trend. |
| **Theme** | A specific video idea inside a niche. |
| **Word timings** | When each word of the narration is spoken. Comes with generated narration; computed by alignment for imported audio. Drives captions and cut snapping. |
| **Template** | A versioned, editable prompt/plan used to generate a script, title, description or media prompt. |
| **Video project** | One video in production: script, generated assets, timeline, per-network metadata and publications. |
| **Asset** | Any media file in a project: generated image, video clip, narration, music, SFX or imported file. |
| **Timeline** | Ordered clips on video and audio tracks, with cuts, gain, fades and captions. |
| **Render preset** | Output format per network (aspect ratio, resolution, codec, bitrate, duration limits). |
| **Render** | Producing the final media file from a timeline. Irreversible in cost/time; requires review. |
| **Publication** | A render sent (or scheduled) to one network account, with its metadata and status. Requires review. |
| **Manual publication** | A publication the user posted by hand from an export, linked to Bardo by its post URL so its metrics can be tracked. |
| **Export** | A ready-to-post package (file in the network preset + metadata to copy) for manual posting. |
| **Budget** | A monthly spending limit the user sets per AI provider. Reaching it makes new jobs for that provider ask for confirmation. |
| **Job** | A long-running task (generation, render, upload, metrics sync) in the queue, with progress, cancel and resume. |
| **Metrics snapshot** | Post-publication statistics for one publication at a point in time (views, retention, engagement and, on YouTube, estimated revenue/CPM/RPM). |
| **Decision engine** | Answers typed questions (choice, score, yes/no) about a piece of content, with probabilities and confidence. Ranks, scores and gates; never generates content. Implemented by JEV (hosted) and, later, Laya (local). |
