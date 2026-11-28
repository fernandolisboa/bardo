# Scene chat spec

- Status: Proposed. Defaults chosen on 2026-11-28; each slice may revisit its own details before it starts.
- Date: 2026-11-28
- Parent issue: #367 (a sub-issue of the PRD, #4)
- Builds on: scenes and images (#15), clips and motion prompts, the storyboard plan with reference shots (#258, ADR-0029), visual styles (#259, ADR-0032), paid calls sent once (ADR-0030), text jobs at work on the same thing (ADR-0036), Gemini Omni clips, costs and budgets
- Decision record: ADR-0049 (`docs/adr/0049-a-scene-is-adjusted-through-a-reviewed-proposal-and-edits-not-redraws.md`)

## Goal

A scene's image or clip often comes out almost right: the cup is the wrong color, the camera moves too fast, the woman should look at the window. Today the only fix is to rewrite the prompt in English and pay for a whole new drawing or clip, which changes more than the user asked for. The **scene chat** lets the user pick a scene's image or clip, write in their own words what should change, see what Claude understood and what it costs, and apply it. The result goes to review, as every new image and clip does.

## Principles

- **AI-assisted, never automatic.** Nothing paid happens on *Send*. Claude first answers with what it understood and one or more options, each with its price; the user applies one, or writes again.
- **Change only what was asked.** An image is edited, not drawn again: the current image goes to the model with the instruction, so framing, people and setting stay. A clip can be edited the same way.
- **Everything new is reviewed.** An applied option lands as a new image or clip waiting for review (*Use new* / *Keep current*), exactly as *Draw again* and *Animate again* do today. Rejecting it leaves the scene as it was.
- **Nothing paid twice.** Each paid call follows ADR-0030 (written down before it is sent, never sent again by itself); a Claude answer that arrived is kept until shown (ADR-0036).
- **The user's words, Bardo's prompts.** The user writes in the interface language; Claude replies in it. What goes to the image and video models is English, as every prompt Bardo sends.

## Vocabulary

Each slice adds its terms to `docs/CONTEXT.md` when it lands.

| Term | Meaning |
| --- | --- |
| **Scene chat** | The conversation about one scene: the user's requests, Claude's replies and what was applied. Kept with the scene, and removed with it when the scenes are planned again. One chat per scene, about its image and its clip. |
| **Chat target** | What a request is about: the scene's *image* or its *clip*. The user picks it when opening the chat; the chat header shows it and lets the user switch. |
| **Proposal** | Claude's answer to a request that it understood: a short sentence of what will change, and one or more **options**, one of them recommended. A request Claude did not understand gets a **question** instead, with no options. |
| **Option** | One way to apply a proposal, with its price: *Edit this image*, *Edit this clip*, *New clip* or *Edit the image, then a new clip*. Applying an option starts its job. |
| **Image edit** | A new image drawn from the scene's current image and an English instruction ("make the cup on the table white; keep everything else the same"), with the references the scene is drawn with (its people's portraits and its place), the current image last; other shots of the video are left out, since the current image already carries the look. |
| **Clip edit** | A new clip made by Gemini Omni from the scene's current clip and an English instruction; the motion stays. |
| **Chat placement** | Where the chat opens, chosen in Settings › Appearance: *beside the scene* (in the inspector, the default), *at the bottom* (a dock under the scenes) or *floating* (a box next to the picked scene). |

## How a request works

1. The user picks a scene and opens the chat on its image or its clip (*Adjust with AI* on the picture or the clip in the inspector, or the scene card's menu). The chat shows the scene's earlier messages.
2. The user writes a request and sends it. Sending costs a Claude call of a few cents, shown next to *Send* and recorded in Costs; the budget question of #268 applies as for any text call.
3. A **scene chat job** asks Claude, with the *Scene chat* template, for a proposal or a question. Claude receives:
   - the request and the scene chat's recent messages (the last 12);
   - the scene: its number and time, narration, image prompt, place and cast with their descriptions (the motion prompt too for a clip target), the video's visual style and its frame shape;
   - what it is about: the current image for an image target; for a clip target the image and three frames of the clip (first, middle, last), taken by the media sidecar;
   - which options this target allows now (below) and the price of each.
4. Claude answers with structured output: a reply in the interface language, then either a question, or the options it proposes with, per option, the English instruction or motion prompt it would send and which option it recommends. The answer is kept in the job's checkpoint until saved in the chat.
5. The chat shows the reply and one button per option, *Apply · US$0.10*, the recommended one first. Applying asks for budget consent as *Draw again* and *Animate again* do, then starts that option's job. An option proposed for an image the scene no longer shows does not apply, and none applies while a new image waits for review; the price shown counts what applying draws first (missing portraits), as *Draw again*'s does.
6. The new image or clip waits for review in the inspector and in the chat. The chat notes what happened: applied, used, kept current, failed with why.

Only one scene chat job runs per scene at a time (`Work::SceneChat`), and an option is refused while the scene is being drawn or animated, as *Draw again* is today.

## The options

| Target | Option | What it does | Price shown |
| --- | --- | --- | --- |
| Image | **Edit this image** | Image edit with Claude's instruction. When the new image is used, the scene's image prompt becomes Claude's rewritten prompt, so a later *Draw again* keeps the change. | One scene image (Nano Banana) |
| Clip | **Edit this clip** | Clip edit with Gemini Omni. Offered when the clip is at most 15 seconds, 16:9 or 9:16, and a Gemini key is saved. The scene's image stays as it was. | The clip's seconds at Gemini Omni's rate for its resolution |
| Clip | **New clip** | Claude rewrites the motion prompt; a new clip is made from the same image with the scene's video model. The motion prompt becomes the new one when the clip is used. | One clip with the scene's model |
| Clip | **Edit the image, then a new clip** | An image edit, reviewed; once it is used, a new clip from it. Asked for when the change should also hold in the still picture (the scene's image is a reference shot for the scenes of the same place, #258). | One image plus one clip |

Claude recommends *Edit this clip* for changes to how things look, *New clip* for changes to movement or camera, and *Edit the image, then a new clip* when the scene's image is a reference shot for other scenes. The user may apply any option offered.

The chat is not offered on composed scenes (product shots and end cards, ADR-0040, ADR-0044): their pictures never go to a model. A scene with no image yet offers no image target; a scene with no clip offers no clip target. A stale plan (#262) shows the chat read-only, as every paid button is held there.

## Placement

The chat placement is a profile setting (Settings › Appearance), and the chat header has the same choice:

- **Beside the scene** (default): a *Chat* tab in the inspector, under the scene's picture or clip, so what is being changed stays in sight.
- **At the bottom**: a dock under the scenes grid, as wide as the screen, with a thumbnail of the target on its left. It can be resized and closed.
- **Floating**: a box over the screen next to the picked scene's card, which follows the selection and can be dragged or closed.

Each placement works in both layouts (Workspace, Studio), in all ten themes and down to the narrow shell (640 × 600), where *at the bottom* and *floating* fall back to *beside the scene*.

## Templates and costs

- A new template kind, **Scene chat**, holds Claude's instructions and prompt, with variables for the request, the recent messages, the scene, the target and the options offered. It is versioned and per project like the other kinds; the chat names the version its next reply is written with.
- Cost purposes: *Scene chat* (Claude); an applied edit is recorded under the purpose of what it makes (scene images, scene clips) on its own job, so Costs lists it with the job that made it.
- First guesses for the estimate: a chat reply 3,000 input / 600 output tokens; refined by the real-key checks.

## Not in this feature

- Editing the transition between two clips (#363) and keeping recurring objects consistent (#361) are their own issues.
- No history of a scene's earlier images or clips to go back to: rejecting a review keeps the current one, as today.
- No chat about the narration, the script or the music.
- No other video-edit providers (Kling O1/O3, Runway Aleph); Gemini Omni is already Bardo's default clip model.

## Acceptance

- A request never pays more than its Claude reply until the user applies an option; the price shown is the price recorded.
- An applied image edit keeps the parts of the image that were not asked to change (real-key check); a rejected one leaves the scene, its prompt and its motion prompt untouched.
- A chat reply that arrived while the job was cancelled is not lost and not paid again.
- Scenes planned again remove their chats; an old proposal can never apply to a new scene.
- Guide pages and tours updated in the same PR as each screen change (coverage test in `crates/app/src/guide.rs`); both layouts, all ten themes; pt-BR and en-US; new terms in `docs/CONTEXT.md`.

## Slices

1. **#368 Scene chat on a scene's image**: the chat model, storage and template, the scene chat job, *Edit this image*, the chat beside the scene in the inspector, guide and tour.
2. **#369 Scene chat on a scene's clip: new clip, or edit the image then a new clip** (after #368).
3. **#370 Edit a clip with Gemini Omni from the scene chat** (after #369).
4. **#371 Chat placement: beside the scene, at the bottom or floating** (after #368; may run after #370).
