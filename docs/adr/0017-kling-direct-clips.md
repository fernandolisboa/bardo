# ADR-0017: Scene clips straight from Kling's own API

- Status: Accepted
- Date: 2026-10-20
- Amends: the MVP spec, which left video providers other than Higgsfield and Google out

## Context

Bardo already reaches Kling's models through Higgsfield, which resells them per second. Kling sells the same models over its own API, paid from prepaid resource packages (units, US$ 0.14 each at list price). Its published prices, checked on 2026-10-20, match Higgsfield's at 1080p and are lower at 720p: Kling 3.0 costs US$ 0.084 a second at 720p and US$ 0.112 at 1080p, against US$ 0.112 for Kling 3.0 on Higgsfield. A direct adapter also removes one middleman for users who already have a Kling account, and newer Kling models reach Kling's API first.

Kling runs two API designs side by side. The older one (`/v1/videos/image2video`, the model in a `model_name` field) authenticates with a JWT signed from an access key and a secret key. The newer one (`/image-to-video/{model}`, the model in the path) takes a single API key as a bearer token and refuses the older pair, and its account endpoints still take only the pair.

## Decision

1. **A third clip generator, `KlingClips`,** behind the existing `ClipGenerator` port, with its own key (`Provider::Kling`, code `kling`) in Settings › API keys and a Step by step from the guide. It uses Kling's newer API design and its single API key (`Authorization: Bearer`). Higgsfield stays the app-wide default provider; Kling is a choice per channel and per scene.
2. **Six models:** Kling 3.0 (3 to 15 seconds), Kling 2.6 and Kling 2.5 Turbo (5 or 10 seconds), each at 720p or 1080p, named `{model}/{resolution}` like Google's. Kling 3.0 at 720p is Kling's default. Left out: Kling 3.0 Turbo (it always makes sound and costs more than Kling 3.0), Kling 3.0 Omni and O1 (the same price, adding reference inputs Bardo does not use), and 4K (US$ 0.42 a second for a 1080p render).
3. **One shot, no sound.** The scene's image goes inline (raw base64) as the first frame; sound is off (the narration is the soundtrack), multi-shot is off, the watermark is off, and the prompt is cut at the 2,500 characters Kling advises.
4. **Resent submissions find the first task.** Each submission carries a custom task id (`external_task_id`): `bardo-` and the first 128 bits of a SHA-256 of the job's submission id and the request body. Kling keeps custom ids unique per account, so a submission sent again after a lost answer is refused, and the adapter then looks the task up by that id instead of paying for another clip. The request is in the hash, so another install (a source build, another computer) on the same Kling account never gets back a clip it did not ask for. The job saves Kling's task id before polling, so a restart polls the same task.
5. **Priced from the rate table.** Kling quotes nothing before a clip, so the estimate and the recorded cost come from `rates.toml` per second, by model and resolution, as with Google; budgets work as for every paid provider.
6. **The key check** looks up a task by a custom id no task has (`GET /tasks?external_task_ids=bardo-key-check`), which costs nothing: a valid key gets an empty list or a not-found code, an invalid one 401.

## Consequences

- Users pick between the same Kling models through Higgsfield or straight from Kling. Going direct needs a Kling resource package, which expires; without units Kling refuses the call and the job says so.
- Kling deletes results after 30 days; Bardo downloads each clip as soon as it is ready, without sending the key to the CDN.
- The answer Kling gives to a repeated custom task id is not documented. The adapter looks the task up after any refusal about the request, so it does not depend on the exact code; a real-key check confirms it.
- Other Kling features (first and last frame, elements, motion control, native audio) can come later through the same adapter.
