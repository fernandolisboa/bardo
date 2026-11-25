---
id: scenes
title: Scenes and images
group: production
place: projects/scenes
tour: scenes
---

# Scenes and images

Scenes cut the narration into the shots the video shows, each with an image. They live at the [Scenes](bardo:go/projects/scenes) stage of a project, and the [Clips](clips.md) stage animates them. [Show me the stage](bardo:tour/scenes).

<a id="plan"></a>
## Planning and drawing

**Plan scenes** has Claude turn the narration into a storyboard, from the **image prompt template** ([Templates](templates.md)), the channel's aesthetic notes and the video's [visual style](#style). It first writes down the story the images tell from the first to the last, then splits the narration into scenes on its sentences, so that each scene follows from the one before it, and writes an image prompt and a [motion prompt](clips.md#motion) for each one. It also lists the [people who come back](#cast) and the [places the video returns to](#same-place).

The plan grows with the narration: a one-minute video has about ten scenes, a ten-minute one about a hundred, and that plan takes Claude a few minutes to write. Bardo reads the answer while it is being written, so a long plan is not given up on while it is still arriving, and the estimate under **Plan scenes** follows the narration's length. A narration that could take more than about 245 scenes (a little over 16 minutes of short sentences, more when they are long) is not planned: its plan might not fit in one answer, and an answer that is cut is paid for and lost. The stage says so in place of the button; use a shorter narration or split the video into more than one.

Cancelling the plan in [Jobs](jobs.md#cancel) stops it within moments: Bardo closes the request instead of waiting for the end of the answer, and [Costs](costs.md#spent) counts an estimate of what Claude may have charged. When the whole answer arrives just as you cancel, it is paid and kept: **Plan scenes** then makes the plan from it without asking Claude again. The same goes for an answer that arrived and whose plan Bardo could not save. While a cancelled plan or image job lets go of what it had already sent, the stage says **Cancelled. Finishing what was already sent…** and starts nothing new, so the same work is not paid twice; for an image being drawn this can take as long as that one image.

**Generate N images** then draws every scene still without an image, in one job, in order: each image is drawn with [earlier images of the video](#same-place) beside its prompt, so the scenes look like one video. Nano Banana draws each at 2K in the shape of the video's [format](projects.md#format), 9:16 for a Short and 16:9 for a long video, and Gemini bills each one; a scene the provider declines does not stop the others. The estimate under the leading button says what the run costs. Planning needs a Claude key, and drawing a Gemini key ([Your API keys](api-keys.md#providers)).

<a id="list"></a>
## The scenes

The scenes appear in order, each with its time, its image and where it stands: **To draw**, **Review**, **Failed** or ready. Pick a scene to see it in full; ↑ and ↓ move through them, and Enter uses the new image of a scene under review.

<a id="scene"></a>
## A scene

The picked scene shows the narration it covers, its image and its **image prompt**. **Edit prompt** changes what the next drawing shows (up to 4,000 characters); a scene whose prompt you changed is marked **Edited**. **Generation details** says which model drew the image, with how many tokens and which template version.

<a id="redraw"></a>
## Drawing again

**Draw again** makes a new image from the scene's prompt, its place, its people and the other images of the video as they are now. The new image waits beside the current one: **Use new image** replaces it, **Keep current** throws the new one away. Until you choose, the scene reads **Review** and the stage says how many wait.

<a id="cast"></a>
## People who come back

When the same person appears in more than one scene (the chef, the detective), the plan lists them once with a fixed look: sex, age, skin tone, hair, build and clothes. Before the first scene that shows them is drawn, Nano Banana draws their **reference portrait**, and every scene with them is drawn with that portrait and their look, so they keep the same face from scene to scene. The portrait is billed like a scene image, and the estimate counts it.

A scene that shows someone who comes back lists them under **People in this scene**, with the portrait. **Edit look** changes the description; the portrait no longer matches it, so it goes, and the next scene drawn with that person draws a new one first. **Draw portrait again** makes a new portrait from the same look. Scenes already drawn keep their image either way: draw them again to use the new portrait. A portrait the provider declines is marked on the person, and their scenes are drawn from the description alone.

<a id="same-place"></a>
## The same place from scene to scene

When more than one scene happens in the same place (the kitchen, the control room), the plan lists the place once with one fixed description: what it is, its period, its main objects, the time of day, the light and the colors. A scene that happens there shows it under **Place**. A scene somewhere no other scene happens has none, and describes its place in its own prompt.

Every image is drawn with other images of the video, which Nano Banana receives as references next to the prompt:

- **A scene in a place already drawn** gets the place's description, the first image drawn there and, when the scene right before it is in the same place, that image too, so it continues from it: the same room, light and time of day, seen from another angle.
- **The first scene of a place** gets the place's description and the video's first image, only to match how it is rendered (the medium, the line, the level of detail). **A scene in a place of its own** gets that image too.

The very first image has nothing to follow, so it sets the look of the video. A scene drawn again follows the images the video has at that moment, the first scene included, so drawing one scene again keeps the look of the others. An image waiting for your review is not a reference yet: accept it, and the scenes drawn after it follow it. References cost nothing extra to speak of: an image costs the same with or without them. A large image goes as a smaller copy (a JPEG of the same size), which Bardo keeps beside it in the project folder.

**Edit place** changes the description for every scene of the place; a place you changed is marked **Edited**. Scenes already drawn keep their image: draw them again to use the new description. The images drawn from the old description stop holding the place together, so draw the place's first scene again, accept the new image, and the scenes drawn after it follow it. A place cannot be edited while images are being drawn. If you rewrite a scene's prompt so that it no longer names its place (the name as whole words, with or without "the"), the scene leaves the place and is drawn without it.

A plan keeps at most eight places, the ones with the most scenes; a scene in any other carries the description in its own prompt. Scenes planned before places existed have none; they still get the video's first image for its look. **Plan again** gives them places. A scene plan [template](templates.md) you edited yourself gets places too, but not the default template's storyboard rules (one story in order, staying in a place for several scenes).

<a id="style"></a>
## One style for the whole video

The video's [visual style](projects.md#style) is added by Bardo to every scene image and every portrait, in the same words each time, after the scene's people and place. With **From the channel's notes**, nothing is added and the look is whatever the scene prompts say.

Changing the style, on the project or on its channel, leaves the images as they are. The stage then shows **Scenes in another style** with how many there are:

- **Draw them again** draws every one of them in the current style, after showing what it costs. A person's portrait in the old style is drawn again first, once, and counted in the estimate; it takes the place of the old one, so going back to the earlier style draws it once more and the person can come out a little different from the scenes already drawn. Each new image waits for review beside the current one, like any scene drawn again.
- An image in another style is never sent as a reference, so it cannot pull the next images back to the old look. The first scene drawn in the new style is drawn from its prompt, its place and its people alone; the ones after it go by the new images, even while those wait for review.
- Drawing a single scene again does the same for that scene. The scenes drawn after it follow its new image.

The scene prompts themselves were written for the style the video had when it was planned. If they name another look (a plan made from the notes often does), **Plan again** rewrites them for the new style; that discards the images, so do it before drawing.

<a id="filter"></a>
## All or pending

Above the scenes, **All** shows every one and **Pending** only those with something left: an image to draw or to review (at the Clips stage, a clip to make or to review). Each tab says how many it holds.

<a id="fit"></a>
## After a new narration

When the narration is made again, the scenes' times no longer match and the stage reads **Out of date**. Until the scenes match the narration again, Bardo makes no image or clip for them, since it would be timed for a narration that is gone: the buttons that pay for one are off and say why, and a job run again from the Jobs panel stops without sending anything new. A clip that was already sent to the provider is still collected and kept.

If the new narration reads the same text the scenes were planned on (the script did not change), the stage offers **Fit to the narration**. It keeps every scene with its prompts, people, places, images and clips, and reads when each one starts and ends again from the new narration. It costs nothing. A scene that now runs longer than its clip holds the clip's last frame (a clip made [slowed to fit](clips.md#stretch) first slows further, as far as its limit goes), and the cut in the [editor](editor.md) starts over from the rough cut on the new times, as after any new narration.

The two wait for each other: while a narration is being made, scenes cannot be planned, drawn or animated, and while scenes, images or clips are being made, a new narration waits for them.

<a id="replan"></a>
## Planning again

When the script changed too, the new narration reads other text and the scenes cannot be fitted to it: **Plan again** replaces them with new ones. If they already have images or clips, Bardo asks first, since planning again discards those files. **Plan again** is also there whenever you want a new storyboard for the same narration.

<a id="place"></a>
## Placing the scenes on a version

A [version in another language](script.md#versions) starts with the original's scenes, still on the original's times. Once its own narration is made (generated or imported), the stage reads **Scenes to place** and offers **Place scenes on this narration**. Each scene keeps its image, clip and prompts and moves to the new time of the sentence it started at; its text becomes the new narration's words. Sentences are found by matching the words of the adaptation with what was narrated, so editing the script first is fine.

Placing also builds the version's cut from the original's: each scene's framing, motion and transition, the scenes it left out, the caption style and highlight, the mix and the frame shape. Music and SFX move to the same point of the narration (music keeps its start and end on the words, an SFX its length). Captions come from the new narration. Splits and trims inside a scene, imported footage placed by hand and edited caption text are not carried; redo them in the [editor](editor.md).

Scenes stretch or shrink with their sentences. **Worth a look** lists the scenes that now run past their clip (the clip holds its last frame), those that grew by more than half or shrank by more than a third, and those the version does not narrate, which are left out of the cut. After a new narration, placing is offered again, and until the scenes are placed no image or clip is made for them; **Plan again** replaces the scenes as in any project.
