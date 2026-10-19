# ADR-0015: Bardo composes the video's music with ElevenLabs Music

- Status: Accepted
- Date: 2026-10-19
- Amends: the MVP spec, which left the music generation API out (Bardo wrote a music prompt and the user imported the track)

## Context

Every video needs music under the narration. Until now Bardo only wrote a music prompt: the user copied it into another tool, made the track there and imported the file. That is the one production step Bardo did not do itself, and the copy, download and import round trip is where most of the time went.

The music must be usable on monetized channels in the high-CPM markets (US first, then BR), so the provider's license matters as much as the sound. It must stay under the narration, so it is always instrumental, and it must be about as long as the narration so it needs no looping or trimming to start.

## Decision

1. **Provider: ElevenLabs Music** (`POST /v1/music`, model `music_v1`), with the ElevenLabs key the user already saves for narration. Its API is open to paid ElevenLabs plans only, and paid plans carry the commercial license for the music. The key needs the **Music** permission besides the narration ones. It sits behind a domain port, `MusicGenerator`, like every other provider, so another one can follow.
2. **From the music prompt.** The music prompt step stays: Claude writes it from the template, the user edits it, and **Generate music** sends the prompt as saved. Copying it into another tool and importing the track still works.
3. **Always instrumental, as long as the narration.** Every request sets `force_instrumental`. The length is the narration's, within the API's 3 seconds to 10 minutes; with no narration yet the card says to make it first.
4. **Review before it enters the cut.** A new track waits on the card. **Use this track** puts it on the music track (A2) from 0:00, ducked as the cut has it, in place of the generated track used before; music the user placed stays. **Discard** deletes it. A new track takes the place of the one in review, never of the one in use. Every cut built later (rough cut, or a cut that starts over) starts with the track in use.
5. **A track is a media asset.** It is saved as a project media file with source `generated`, so the editor, proxies, preview and render treat it like an imported file. A track in review stays out of the media bin.
6. **Paid once, recorded fully.** Composing runs as a job. The audio is saved before the track is recorded, so a job that stops in between reuses it instead of paying again. The call is priced per second of music (purpose "Music", `rates.toml`), budgets ask first, and each track records provider, model, prompt, length and job. Tracks are signed with C2PA (`sign_with_c2pa`).

## Consequences

- Music needs a paid ElevenLabs plan and a key with the Music permission; without them the job fails with ElevenLabs' reason and nothing is added. Users without a paid plan keep the prompt-and-import path.
- ElevenLabs refuses prompts that name artists or copyrighted works and suggests another wording; Bardo shows it with the failure. The music prompt template already avoids artist names.
- A narration longer than 10 minutes gets a 10-minute track; the rest of the cut has no generated music until the user places more.
- `music_v2` (composition plans) and other providers can come later behind the same port.
