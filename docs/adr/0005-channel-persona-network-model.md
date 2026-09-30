# ADR-0005: Channel, persona and network account model

- Status: Accepted
- Date: 2026-09-30

## Context

A persona (narrator voice, tone, script style) may be reused across channels, for example the same narrator on an English and a Portuguese channel. Persona choice is an individual user decision, and users may create personas from their own voice or from people close to them.

## Decision

```
UserProfile 1──* Persona
UserProfile 1──* Channel
Channel     *──1 Persona          (default persona)
Channel     1──* NetworkAccount   (one per network)
Channel     1──* VideoProject
VideoProject *──0..1 Persona      (per-video override)
```

- **Persona is a user-owned library**, independent of channels. The app ships a few default personas; the user creates and edits their own.
- A channel has a default persona; each video can override it.
- Each network account can override the channel's metadata defaults and render presets.
- **No login system.** Ownership is by local user profile so more profiles can be added later without a data migration.
- **Voices stay with the provider.** A persona holds a voice reference (e.g. ElevenLabs voice id), never voice samples or credentials.
- **Cloned voices follow provider rules.** Cloning a voice requires the voice owner's verification in the provider's flow. When a publication uses a realistic synthetic voice of a real person, the YouTube upload sets `containsSyntheticMedia` and other networks' equivalent disclosure where available.
- **Sharing** is export/import of a persona package file (tone, style, presets, voice reference). The recipient needs the voice shared with them on the provider side; Bardo runs no server.

## Consequences

- Reuse across channels and languages without duplication, at the cost of one more entity.
- Editing a persona affects every channel using it as default; the UI must show where a persona is used before saving.
- An imported persona whose voice reference is unavailable in the user's provider account must be flagged, not fail at generation time.
