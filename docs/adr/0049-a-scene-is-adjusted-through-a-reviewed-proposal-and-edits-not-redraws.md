# ADR-0049: A scene is adjusted through a proposal the user applies, and an adjustment edits rather than redraws

- Status: Accepted
- Date: 2026-11-28
- Issue: #367 (slices #368 to #371)
- Spec: `docs/spec/scene-chat.md`
- Builds on: ADR-0029 (storyboard plan and reference shots), ADR-0030 (paid calls are sent once), ADR-0036 (text jobs at work on the same thing), ADR-0040 and ADR-0044 (composed pictures never go to a model)

## Context

A scene's image or clip is often almost right. The only fix so far was to edit the English prompt and draw or animate again, which pays for a whole new picture and changes more than the user wanted: a new drawing reframes the shot, recasts the background and may change the people. The user wants to say, in their own words, what to change on the scene they are looking at.

Three choices shape that feature: whether a request is paid at once, how a change is made without disturbing the rest, and where the conversation lives.

## Decision

1. **A request is answered before anything is paid but the answer.** *Send* starts a scene chat job that asks Claude for a **proposal** (what will change, and one or more options, each with the English instruction or prompt it would send) or a **question** when the request is unclear. Each option shows its price; only *Apply* starts the paid image or clip job, after the budget consent every paid button asks for. Claude's answer is kept in the job's checkpoint until saved, so a cancel or a close never pays for it twice (ADR-0036).
2. **An image is edited, not drawn again.** The image job's edit mode sends the instruction with the references the scene is drawn with (its people's portraits, its place), and the scene's current image as the last reference, so the numbering of the others does not change; other shots of the video are left out, since the current image already carries the look. The change holds and the rest stays. The option also carries a rewritten scene prompt, which replaces the scene's prompt only when the user uses the new image; a later *Draw again* then keeps the change.
3. **A clip has three ways, and the user picks.** *Edit this clip* sends the clip itself to Gemini Omni 1.2 Flash, which edits a video and keeps its motion, at the same per-second rate as generating; it is offered only within Omni's limits (15 seconds, 16:9 or 9:16). *New clip* rewrites the motion prompt and animates the same image with the scene's model. *Edit the image, then a new clip* changes the still first, for when it is a reference shot of other scenes (ADR-0029), and never animates an image the user did not use.
4. **Every result is reviewed** as a new image or clip waiting for review, as *Draw again* and *Animate again* do. Rejecting it leaves the scene, its prompt and its motion prompt as they were.
5. **An option is tied to what it was proposed for.** It names the scene's image or clip generation; once that changed, or the scene's model or the price changed, it is offered again with the new price instead of running.
6. **The chat belongs to the scene.** It is stored with the scene plan, kept when the plan is fitted to a new narration, removed when the scenes are planned again, and carried by project packages through its foreign key (ADR-0048). The prompt is a versioned template kind, *Scene chat*, like every other Claude call.
7. **Composed pictures are left out.** Product shots and end cards are composed by Bardo and never go to a model (ADR-0040, ADR-0044); the chat is not offered on them.

## Consequences

- A small change costs one Claude reply plus one image or clip, and changes only what was asked; the user sees the interpretation before paying for the picture.
- The image job and the clip job gain an edit mode instead of a parallel job kind, so review, stale handling, budget consent and the paid-once rules stay in one place.
- Editing a clip depends on Gemini Omni's editing, which Google does not offer in the EEA, Switzerland or the UK; there the option fails with Google's reason and the other two options remain.
- Repeated edits of an edited image may slowly lose detail; *Draw again* from the rewritten prompt remains the way back to a fresh drawing.
