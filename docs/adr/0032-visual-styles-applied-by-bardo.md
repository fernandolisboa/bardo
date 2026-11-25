# ADR-0032: Visual styles from a catalog, applied by Bardo to every image

- Status: Accepted
- Date: 2026-11-25
- Issue: #259
- Builds on: the storyboard plan and reference shots (ADR-0029), the channel default with a per-video override of formats (ADR-0018) and production modes (#248), the template defaults that move on by themselves (#179)

## Context

How a video looked (a photograph, a cartoon, a manga drawing, a clay model) was decided by a free-text field of the channel, the aesthetic notes, which the scene planner was asked to repeat in every image prompt. That had four weaknesses:

- **A blank field left the look to chance.** With nothing to repeat, each prompt named a look of its own, or none.
- **Nothing enforced the repeat.** One prompt of thirty that said "photo" in a video of drawings gave one photograph in the middle of it, and the reference shots of ADR-0029 then carried that look on.
- **Cast portraits were drawn without the notes at all**, from the member's description, so a portrait could be a photograph in a cartoon video, and every scene drawn with it leaned back toward a photograph.
- **A look could not be picked for one video**, only by rewriting the channel's notes and writing them back afterwards.

ADR-0029 made the video's first image the reference for how every later one is rendered, which holds a look once it exists but does not choose it.

## Decision

1. **A catalog of styles in the domain.** `VisualStyle` is a fixed list of 26 styles, each a stable code and one fixed English text an image model draws it from: the words that name it, a colon, then how it is rendered. Styles are described by technique (medium, line, light, texture) and never name a studio, a brand, a franchise or an artist: a technique is not anyone's property, a named look is, and a test keeps the common names out. Names in the two interface languages live with the other strings.
2. **"From the channel's notes" is a choice.** `StyleChoice` is a catalog style or `FromNotes`, which is how every channel worked before and what existing channels and videos have. It is stored as the code `notes`; a code this version does not know (a style a newer version added) reads as `notes`, so the channel still opens and the video is still made.
3. **A channel default and a per-video override**, as with the format and the production mode: `channel.visual_style` (never empty) and `video_project.visual_style` (NULL follows the channel). A project can pick `notes` for itself under a channel that has a style.
4. **Bardo adds the style; the planner does not.** Every scene image prompt gets the style's line after the people and the place and before the lines about the reference shots; every cast portrait prompt and the thumbnail's background prompt get it after their own text. The line is "Render this whole image in this visual style:", the style's text, then one sentence that holds for every style: the picture fills the frame, with no border, bars, page or panels and nothing written on it, because a style named after an object (a film still, a print, a comic) tends to be drawn as the object. A portrait then says again what a portrait keeps (the framing, the neutral expression, the plain evenly lit background), since a style may ask for hard shadows, haze or a set, and every scene with the person is drawn from that portrait. A generation record tells which style an image was drawn in by the start of the line and the words that name the style, which never change (a test pins them); what follows the colon can be reworded without existing images changing style. With `FromNotes` nothing is added and every image and portrait request is byte for byte what it was.
5. **The planner is told, through a template variable.** `{{visual_style}}` (the image prompts template, which plans the scenes, and the thumbnail template) says that a style was chosen and the words Bardo adds for it, or that none was. The default templates tell Claude to leave every word about rendering out of the prompts and the descriptions when the prompt names a style and says Bardo adds it, and in every other case to take the look from the aesthetic notes and repeat it. The instructions are worded to hold with an earlier prompt part too, since the two parts of a template move on separately. The defaults from before move on by themselves; an edited template keeps working, because step 4 does not depend on it.
6. **One look per video.** An image whose generation prompt was not drawn in the video's current choice is not a reference: not a scene's image for the reference shots, and not a portrait. Where a scene's image in use is of another style and a new one in the current style waits for review, the waiting one is the reference, so a video drawn again in a new style still holds together from scene to scene (ADR-0029) before anything is accepted. A portrait in another style is drawn again before the next scene that shows its member, as a missing one is, and the estimate counts it. For `FromNotes` an image counts when its prompt carries no style line.
7. **Changing the style changes nothing drawn.** It applies to the next plan, images, portraits and thumbnail background. The scenes panel says how many scenes have an image in another style and can draw them all again in one job; each new image waits for review beside the current one, as any redrawn scene does. A scene whose new image already waits is not counted again.
8. **An image job draws in the style it was started with.** The style is read when the job is queued and travels in its payload, like the frame shape, so a job that runs for minutes does not change look halfway when the user changes the choice. A failed or cancelled job whose style is no longer the video's is not run again: its images would count as another style at once, and its portraits would replace the current ones with no estimate shown. The Jobs panel says to draw from the Scenes stage.
9. **A version in another language keeps the source's style.** It carries the source's images, so it starts with the source's effective style as its own, whatever its channel's default is.
10. **The settings file carries the channel's style**, by its code. A file from before styles, or one with a code this version does not know, brings the notes.

## Consequences

- Every image of a video is asked for the same look in the same words, and no image of another look can be a reference. Whether the image model then holds the look in every scene is what the real-key check is for.
- Picking a style and drawing the same plan again is how a look is tried. It costs the images again (and one portrait per recurring person), which the panel shows before it starts; nothing is paid for changing the choice.
- Scene prompts written under one style can still name another look in their own words (a plan made from the notes usually does). The style line comes after them and usually wins, but planning again under the new style is the clean way, and it discards the images. The guide says so.
- An edited scene plan template without `{{visual_style}}` still asks for the notes' look in every prompt while Bardo adds the chosen style after it. The guide tells the user to add the variable.
- The catalog's texts are the product's taste and will be tuned against real output. Rewording what follows a style's opening words keeps the images already drawn in it; the opening words and the start of the line stay as they are for good.
- A thumbnail background prompt written before the style changed can name the earlier look, as scene prompts can; writing it again fixes it.
- A cast member has one portrait. Drawing in a new style replaces it, and going back to the earlier style draws it again: a face a little different from the one the accepted scenes were drawn from, and one image paid twice. Keeping a portrait per style would avoid both and is left for when trying styles on finished videos proves common.
- A style code this version does not know reads as the notes and is written back as `notes` when the channel or the project is next saved, so a choice made in a newer version does not survive a round trip through an older one.
- Motion prompts and clips are untouched: a clip is animated from its scene's image, which already carries the style.
- Persona looks (performed clips) keep their own prompt and are not styled: a look is the persona's, across videos.
- The real-key check on the regression pass draws the same 30 second script in three styles and looks for each style in every scene.
