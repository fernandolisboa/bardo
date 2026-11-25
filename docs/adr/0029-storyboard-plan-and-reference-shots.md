# ADR-0029: A storyboard plan with places, and scene images drawn from earlier ones

- Status: Accepted; decision 9's wait and the long plan's limits amended by [ADR-0031](0031-streamed-answers-and-a-plan-sized-from-its-narration.md)
- Date: 2026-11-25
- Issue: #258
- Builds on: the scene plan of the MVP spec (`docs/spec/mvp.md`), the cast and motion prompts of #246, the template defaults that move on by themselves (#179)

## Context

A video's scenes did not read as one sequence. The same person held from scene to scene (#246), but the place, the light, the time of day and the rendering style changed at every cut, and the pictures did not follow the narration as a story.

The causes sit along the whole chain, not in one prompt:

- **The script template** asked for a hook, tension and a payoff, and said nothing about order or about writing what can be shown. A script that jumps between times and ideas cannot be drawn as a sequence.
- **The scene plan template** told Claude that each image prompt stands alone, because the image model sees one prompt at a time. That is true of the image model, but it made the planner write every scene as if it were the only one: a new place and a new framing for every sentence.
- **Each image was drawn with only the cast portraits.** Nothing told the image model what the scene before looked like, so two scenes in "the kitchen" were two kitchens.
- **Every text call ran at the model's default effort**, including the two that shape the whole video.

Nano Banana (`gemini-3.1-flash-image`) takes up to 14 reference images in a request, 10 of objects or places and 4 of characters. How much inline data a request may carry is given as 100 MB on Google's page about file inputs and as 20 MB for the whole request on its page about images (both read on 2026-11-25). An image costs the same with or without references, apart from a few input tokens.

## Decision

1. **The script follows one through-line.** The default script template asks for a story in the order it happened, or an explanation built step by step, written so it can be shown, staying in one moment and place for a few sentences and saying so when it moves. The opening may tease the end, and a list video gives each item its own part.
2. **The plan is a storyboard.** The default image prompt template asks for the images to tell the narration's story with the sound off. Its answer starts with `story`, a paragraph Claude writes before anything else, so the scenes are planned from one account of the video. Claude answers in the order of the schema's properties, so the schema must reach it in the order written: the workspace turns on `serde_json`'s `preserve_order`, and the adapter's test reads the body sent. `story` is only kept in the plan's generation record: it is working text, not something the user edits.
3. **Places, as the cast.** The plan lists the recurring places once (`places`, at most eight), each with a name and one fixed description, and each scene says which place it happens in (`place`, -1 for a place of its own). They follow the cast's rules: an unusable or unused place is dropped and the scenes' indexes follow; the user can edit a description, and the plan keeps the planned one beside it. A place returned to at another time or in another state is another entry, so one place has one look. Past eight, the places with the most scenes stay, and a scene of another keeps the description in its own prompt. The schema says what `story`, `places` and `place` are for, so a template the user edited before they existed still gets an answer that uses them.
4. **Bardo adds the place to the image prompt**, as it adds the cast: the scene's prompt, then the people, then "The scene happens in *name*: *description*". Claude is told not to write those out, so an edit to a place reaches every scene of it.
5. **Reference shots.** A scene's image request carries, after the cast portraits, other scenes' current images, each named in the prompt with what it is for:
   - the first image of the same place: keep the place;
   - the scene right before it, when it is in the same place: continue from it (alone when it is also the first of the place);
   - when the place has no image yet, or the scene has no place: the video's first image, for how it is rendered only (not its place, light, colors or people).

   At most two shots, so with three portraits a request has five references, well within the model's limits. The prompt tells the model to draw a new shot framed as the scene's prompt says, since a reference alone invites a copy, and that people in a reference are in the scene only if its prompt names them. In the request each image follows its number ("Reference 2:"), the name the prompt calls it by.
6. **Order gives the references.** The image job already draws its scenes in order and saves each image as it arrives, and a reference is always a scene's current image at the moment of drawing (an image waiting for review is not one yet). So nothing new is stored: the first image of a video sets its look, each later one finds the earlier ones, and a scene drawn again follows the images as they are then. An image shows a place only when the prompt it was drawn from, which its generation record keeps, carries the place's description as it is now: after the user describes a place again, the images of how it was lend only their look.
7. **Plans without places share the look.** Scenes of a plan made before places get the video's first image as a look reference too. The rule is one rule for every plan, and it helps the videos already made.
8. **An edited prompt can leave its place.** A scene whose prompt the user rewrote keeps its place only while the prompt still names it (as whole words), so writing a scene somewhere else does not drag the old place's description and images into it. The inspector says so.
9. **Careful requests.** `TextRequest` gains `effort`: `Everyday` (the model's default) and `Careful`. The Claude adapter sends `Careful` as `output_config.effort: "high"` with a larger token limit, and that request alone waits longer for the answer (ten minutes instead of three), since a non-streamed answer that outlives the wait is billed and lost. Only the script and the scene plan are careful; the model stays the same. The first cost guesses for those two calls go up accordingly.
10. **References are small.** A 2K drawing is several megabytes as saved, and five would pass the smaller of the two documented limits. A reference over 2 MB goes as a JPEG of the same size within 2 MB, made once by the media engine and kept beside the image in the project folder (it goes when the image goes). A request takes at most 12 MB of references (16 MB once in base64); one past that is left out, portraits first in line, and the prompt is written for the references actually sent. (Since ADR-0034 a portrait is never left out: a scene whose portrait cannot be sent is not drawn.)

## Consequences

- A place looks the same across its scenes, and a scene continues the one before it, at no extra image and almost no extra cost.
- The first image of a video matters more than the others: it sets the look, and drawing one scene again keeps the look of the rest. Choosing the look on purpose is the visual style's job (#259).
- Drawing a scene again can change with the state of the other scenes, which is what "drawn with the images as they are now" means; the generation record keeps the exact prompt sent.
- The script and the scene plan cost more and take longer (more thinking, billed as output). The plan of a long video can still pass the token limit or the ten minutes of a request that is not streamed, and such an answer is billed without being kept: streaming the Claude adapter and sizing the plan's limit from the narration are their own change (#267).
- Estimates for the script and the scene plan follow the profile's last calls, so on a profile with earlier, cheaper calls they catch up over the next few videos.
- Requests carry up to five reference images of at most 2 MB each. The real-key check on the regression pass confirms they go through on a home connection.
- Profiles whose script or image prompt template is still one of Bardo's earlier defaults move on to the new ones. Edited templates stay: their plans get places, from the schema alone, but not the storyboard rules until the user loads the default.
- Who the cast is still comes from one stateless request, so two videos of a channel can get similar people. Telling the planner which people the channel's other videos used is a separate change.
- Clips are still made one scene at a time from its image. Chaining clips (the last frame of one as the first frame of the next) is a separate decision.
