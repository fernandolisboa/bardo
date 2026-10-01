# ADR-0001: Architecture and stack

- Status: Accepted
- Date: 2026-09-30

## Context

Bardo is a native Windows desktop app, single-user and offline-first. It orchestrates several AI providers, heavy media processing (ffmpeg) and uploads to multiple networks. Providers will change over time; the core must not.

## Decision

- **Rust everywhere**, as a Cargo workspace:

  | Crate | Responsibility | May depend on |
  | --- | --- | --- |
  | `domain` | Entities, value objects, domain services, provider interfaces (traits). No I/O. | nothing internal |
  | `media` | ffmpeg-based probing, proxies, preview, render. | `domain` |
  | `ai` | Adapters for JEV, Claude, image, video, voice, STT providers. | `domain` |
  | `publish` | Adapters per network (upload, schedule, metrics, export). | `domain` |
  | `storage` | SQLite persistence, migrations, secrets access (Windows Credential Manager). | `domain` |
  | `app` | Application state, use cases, job queue, orchestration. The UI's only entry point. | all of the above |
  | `ui` | GPUI views and components. | `app`, `domain` |

- **Only `ui` imports GPUI.** Enforced by a dependency lint in CI (e.g. `cargo-deny` bans or a workspace check), not by convention.
- **GPUI usage**: consult current docs via Context7 before writing GPUI code; pin the exact version in `Cargo.toml`; keep usage in small components.
- **Every external integration is an adapter** behind a trait in `domain`. Swapping a provider never touches `domain` or `app`.
- **AI roles**:
  - JEV: typed decisions only (rank themes, score cut candidates, pick presets, quality gates). Text in, typed result out. Never generates content.
  - Claude API: scripts, titles, descriptions and prompts for media models.
  - Image: Nano Banana (Gemini). Video: Higgsfield API (aggregates Kling, Seedance, Wan, MiniMax and others) and Google via the Gemini API (Veo, Gemini Omni Flash). Voice: ElevenLabs. STT: Whisper with timestamps.
- **Secrets** live in Windows Credential Manager under the user's account. Never in files, the database or logs.
- **Long jobs** (generation, render, upload, metrics sync) run in a persistent queue with progress, cancel and resume. Nothing blocks the UI thread.
- **Performance** work targets ffmpeg, I/O and job concurrency, and only with measurements.

## Testing

- `domain`: unit tests, TDD.
- Adapters: integration tests against recorded fixtures (no live calls in CI).
- `media`: short versioned clips in the repo.
- UI: tested through `app` state, not pixels.

## Consequences

- Clear seams make providers cheap to swap and tests cheap to write.
- The dependency lint must exist before the first `ui` code lands.
- GPUI is young and moves fast; pinning and isolation contain the churn.
