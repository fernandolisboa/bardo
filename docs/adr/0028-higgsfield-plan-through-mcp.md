# ADR-0028: Clips paid from a Higgsfield plan through its MCP server

- Status: Accepted
- Date: 2026-11-20
- Issue: #255
- Builds on: ADR-0008 (sign-in and tokens in Credential Manager), the clip generator of the MVP spec

## Context

Higgsfield sells two products that do not share money. Its API (`open.higgsfield.ai`) is prepaid in dollars and reached with a key; that is the **Higgsfield** provider Bardo already has. Its plans on higgsfield.ai give monthly credits that are spent on the website, or through Higgsfield's own remote MCP server and CLI, which act as the signed-in account. A user with a plan cannot spend those credits through the API.

Higgsfield's terms (checked on 2026-11-19) cover programmatic access in their "Developer Access" section and allow third-party MCP clients (assistants, applications, agents) acting for the user, whose activity counts as the user's own. The same section warns that the MCP server's tools and schemas may change at any time.

The server, checked on 2026-11-19 and 2026-11-20:

- **Transport.** `https://mcp.higgsfield.ai/mcp`, MCP Streamable HTTP (protocol `2025-06-18`): `initialize`, then `notifications/initialized`, then `tools/call`, with the `Mcp-Session-Id` it hands out. Answers come as JSON or as server-sent events. A 401 carries OAuth protected resource metadata.
- **Sign-in.** OAuth 2.1 as MCP asks: authorization server metadata at `/.well-known/oauth-authorization-server`, dynamic client registration at `/oauth2/register` (a public client with a loopback redirect and `token_endpoint_auth_method: none` is accepted), authorization code with PKCE S256, the `resource` parameter, and scopes `openid email offline_access` (refresh tokens).
- **Tools.** `balance` (plan and credits), `estimate_video_cost` (credits, free), `media_upload` (a presigned PUT) and `media_confirm`, `generate_video_batch` (no idempotency key; it must be told `use_unlim: false` or it may answer with a choice instead of starting), `jobs_wait` (`timeout_seconds` 0 to 15), `show_generations` (jobs with their parameters, newest first, paged by `next_cursor`). Credits can be fractional (17.5).

Bardo's adapters are synchronous over `ureq`; the official Rust MCP SDK needs `tokio`.

## Decision

1. **A provider without a key.** `Provider::HiggsfieldPlan` (code `higgsfield-plan`) stands for the plan. It has no row under the keys: its card sits under **Plans you sign in to** in Settings › API keys, with **Sign in**, **Check credits** and **Sign out**, and the plan's name and credits once read. Where a feature asks whether a key is saved, a signed-in plan counts as one and a signed-out or refused one does not, so "missing key" checks, model menus and the default model work unchanged.
2. **A small MCP client over the existing transport.** `crates/ai/src/mcp.rs` speaks Streamable HTTP over the adapters' `Transport` seam: one session per access token (by its SHA-256), opened under a lock, reopened once when the server drops it (404), forgotten on a 401. It never retries a tool call on its own. No new async runtime.
3. **Sign-in as networks do, plus registration.** `PlanSignIn` (domain) registers Bardo as a public client for the loopback listener's address on every sign-in, builds the consent address with PKCE and `state`, trades the code and refreshes with the client id. The client id and the token set are kept together in Windows Credential Manager (`Bardo/{profile}/plan/{provider}`, format `bardo-plan-1`), within Credential Manager's 2,560-byte limit; nothing reaches the database. The sign-in is kept only after the tokens read the plan's balance.
4. **Fresh tokens per clip step.** A clip job asks for the plan's access token on every step instead of keeping it for the run. It is renewed 15 minutes before it expires, one renewal at a time (signing in and out wait for it); a renewal that fails while the token still works uses the token, and one the provider refuses (`invalid_grant` or `invalid_client`) marks the plan **Sign-in expired** for the session and fails clips as a missing key. A renewed sign-in the store cannot save is used from memory for the session. A call the server refuses with a 401 renews the token at once and is made once more; a second refusal stops the job.
5. **Never paying twice.** `generate_video_batch` takes no idempotency key, so the `ClipGenerator` port says so (`dedupes_submissions`) and the clip job writes down that it sends a submission before sending it. A submission refused, rate-limited or never sent (the session could not open) started nothing and clears the note; any other failure after sending (no answer, an answer cut off or without a job id) may have started a clip and keeps it. With the note set, the job looks for the clip instead of sending again (`find_submitted`): the generation whose parameters name the first frame uploaded for that submission alone, in the newest five pages of `show_generations`; a model and prompt match is not enough, as it may be an older clip of the scene. Found, the job polls it. Not found two minutes after sending, the scene fails saying the clip may have started and to check the Higgsfield library before animating it again; a failed lookup keeps the note and tries later. Bardo never sends a noted submission again on its own.
6. **Credits checked first, dollars recorded.** Before starting, the free estimate and the balance tell whether the plan can pay; a clip the plan cannot pay for is refused as a limit with the credits needed and left. The rate table prices each plan model per second from its credits at US$ 0.049 a credit (the Plus plan's US$ 49 for 1,000), so Costs, budgets and the whole-video estimate keep working; the user can change those rates.
7. **A fixed catalog.** Kling 3.0 Standard (the default) and Pro, Kling 2.6, Seedance 2.0 1080p, Wan 2.7 1080p and Hailuo 2.3, with sound off where the model makes sound, as the narration is the soundtrack.

## Consequences

- A user with a Higgsfield plan makes clips from its credits without an API balance; the API provider stays as it was.
- The MCP server is not a stable API: a changed tool or schema shows as an unexpected answer on the clip, and the adapter's fixtures say which answers were recorded and which were written from the tools' descriptions.
- Signing out forgets the sign-in in Bardo only; ending Bardo's access at Higgsfield is done in the user's Higgsfield account.
- The dollars recorded are an estimate of the credits' worth, not a charge; the plan's own balance is the truth, shown on the card.
- What has to be confirmed with a real plan (sign-in, a clip's answers, the credits spent against the estimate) is on the regression pass.
