# ADR-0031: Claude's answers are streamed, and a scene plan's room follows its narration

- Status: Accepted
- Date: 2026-11-25
- Issue: #267
- Builds on: careful requests and the storyboard plan (ADR-0029, #258)

## Context

Every request to Claude was sent with one token limit and one clock on the whole answer: 16,000 tokens and three minutes, or 32,000 tokens and ten minutes for a careful request (ADR-0029). The scene plan is the one answer whose size follows the video's length: a few lines once (the story, the cast, the places) and then an image prompt and a motion prompt for every scene.

A plan written under the default template was counted (the figures are on #267). A scene takes 120 to 165 tokens. Before any thinking, the plan of a one-minute video is 1,100 to 2,600 tokens, of a ten-minute video 9,000 to 25,000, of a fifteen-minute video 14,000 to 37,000. The model writes roughly 75 to 100 tokens a second. So the plan of the long format (8 to 10 minutes) passed the token limit or the clock in the upper half of its range, and a longer one always did. An answer cut at the token limit, or one the clock gave up on, is billed in full and nothing of it is kept, and the job could then be started again by hand at the same price.

Two limits are the provider's and do not move: a model writes at most 128,000 tokens in one answer, thinking included, and a request that is not streamed is expected to finish within ten minutes.

## Decision

1. **Every Claude request is streamed.** The adapter sends `stream: true` and reads the server-sent events as they arrive: the text, the model that wrote it (the fallback's, when a declined request moved to it mid-answer), the reason it stopped and the usage. An answer that arrives whole (a proxy that buffers) is still read.
2. **The limit is on silence, not on the answer.** Claude sends keep-alives while it thinks, so a connection that says nothing for three minutes is dead, and one that keeps talking is left alone however long the answer takes. A bound of an hour on the whole answer remains only as a last resort.
3. **A stream that stops says where.** Before anything was generated, an `error` event is the same as its HTTP status (an overload is waited out and sent again). After Claude began to answer, the request is not sent again by the adapter: what was generated is billed, so a second attempt is the user's choice.
4. **A request can say how large its answer gets.** `TextRequest` gains `answer_tokens`. With it the adapter asks for that room and as much again for thought, never less than the request's usual limit and never more than the model's 128,000. Without it nothing changes. Room that is not used is not billed.
5. **The plan's size comes from the narration.** `ScenePlanSize` gives the scenes a narration can have at most (one every four seconds, never more than its sentences) and usually (one every six seconds), and from them the tokens of the answer at most (2,500 + 250 a scene) and usually (600 + 160 a scene). The plan request sends the first as `answer_tokens`. The estimate before planning takes the second, and half as much again for thought, as the least the call will write; past plans that took more still count.
6. **A narration that cannot fit is refused before it costs.** When the plan's room, with half as much again for thought, passes the model's limit (about 330 scenes, 20 to 30 minutes of narration), **Plan scenes** says so instead of starting a job.
7. **Planning in parts is left out.** Asking for the cast and the places first and then the scenes in runs of sentences would lift the limit of point 6, but each part is its own paid call, the parts must agree with each other, and a failure midway needs saved checkpoints so the earlier parts are not paid twice. Bardo's own formats end at ten minutes, well inside one answer.
8. **The per-request wait of ADR-0029 goes.** A careful request no longer needs ten minutes of its own: no request has a clock on its whole answer.

## Consequences

- The plan of a long video arrives: about a hundred scenes take several minutes, during which the job runs as any other.
- A request's cost does not change. Streaming is the same call at the same price, and the larger limit is only room.
- The estimate before planning a long video is higher than before, because it was too low: it now follows the narration.
- A scripted narration longer than about 20 to 30 minutes cannot be planned in one project. If that becomes something people do, planning in parts (point 7) is the change to make.
- The stream's event shapes are the provider's documented ones, and the adapter's fixtures are written from them. The real-key check on the regression pass confirms a long plan against the live API.
- How a lost or cut answer shows in Costs is its own change (#266).
