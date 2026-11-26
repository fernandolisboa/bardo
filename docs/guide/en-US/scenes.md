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

**Generate N images** then draws every scene still without an image, in one job, in order: each image is drawn with [earlier images of the video](#same-place) beside its prompt, so the scenes look like one video. Nano Banana draws each at 2K in the shape of the video's [format](projects.md#format), 9:16 for a Short and 16:9 for a long video, and Gemini bills each one; a scene the provider declines does not stop the others, and retrying the job draws only the scenes it has not drawn yet: one whose image the job already delivered is not drawn again, even if you have rejected or replaced that image since, and neither is one you drew on its own after the job was started. The estimate under the leading button says what the run costs. Planning needs a Claude key, and drawing a Gemini key ([Your API keys](api-keys.md#providers)).

<a id="list"></a>
## The scenes

The scenes appear in order, each with its time, its image and where it stands: **To draw**, **Review**, **Failed** or ready. Pick a scene to see it in full; ↑ and ↓ move through them, and Enter uses the new image of a scene under review.

<a id="scene"></a>
## A scene

The picked scene shows the narration it covers, its image and its **image prompt**. **Edit prompt** changes what the next drawing shows (up to 4,000 characters); a scene whose prompt you changed is marked **Edited**. **Generation details** says which model drew the image, with how many tokens and which template version.

<a id="ad"></a>
## The scenes of an ad

The scenes of an [ad](brief.md) are planned from the **Ad scene plan** template ([Templates](templates.md)) instead: a scene for every sentence, of about 2 to 4 seconds each, the first one showing the hook and the last one being the end card, over the call to action. Each scene of an ad has a **kind**:

| Kind | What it shows | Who makes it |
| --- | --- | --- |
| **Drawn** | A picture from the scene's prompt, like the scenes of any video | The image model |
| **Product in use** | The product being used, from the scene's prompt and the product's photo, [checked by you](#product-check) | The image model |
| **Product shot** | One of the product's own images, as it is | Bardo, for free |
| **End card** | The ad's last picture, over the call to action | Bardo, for free, from a product image; the image model when the product has none |

Which kinds a plan may use follows the product's [images](products.md#pictures): a product shot needs an image, and the product in use needs a **Photo**, since a **Screen** is only ever shown as it is. A product with no image gets drawn scenes and a drawn end card. Claude is told which kinds it may use and, by number, which images the product has; a kind the product does not allow comes back as a drawn scene, and a product shot that names an image the product does not have shows its first one. When the product has an image, the plan is asked to have the product on screen by the time the narration names it, with at least one product shot.

Bardo composes a product shot itself, on your computer, as soon as the plan is saved: the product's image, whole, over a plain backdrop in a color taken from the image ([how it looks](#shot-look) is yours to change), 1080 × 1920 for a vertical video and 1920 × 1080 for a wide one. The image is scaled to fit and never stretched, cut or redrawn, and it sits where neither the cut's motion nor a caption covers it. Composing costs nothing and needs no key, and the product's image never goes to an image or video model from a product shot. A composed picture is never animated ([Clips](clips.md#animate)), is never a reference for drawing another scene, is not offered as the set of a performed take, and in the editor it [only zooms](editor.md#motion).

**Scene kind** in the picked scene changes what it is, among the kinds the product allows; **End card** is offered on the last scene only. A scene made a product shot is composed at once, and **Product image** picks which of the product's images it shows, numbered as on the product. When that image is small for the frame, the scene says so: enlarged, it may look soft. A scene made **Drawn** again goes back to the drawing it had; when it never had one, nothing is drawn until you ask with **Generate N images**, which shows the cost first. When the video changes shape, the composed pictures no longer fit and read **To compose**: **Compose product shots** (or **Compose picture** on the scene) makes them again, for free, and **Generate N images** makes them along with the drawings. A scene whose image was removed from the product says so and keeps the picture it has; when that picture has to be made again, the scene fails until you pick another image or kind for it. A render in the other shape (a long ad posted to a 9:16 network, or the other way round) shows a composed picture whole, with bars.

The video's [estimate](projects.md#mode) counts composed scenes as free. Before an ad has a plan, it is estimated with a scene about every 3 seconds, of which two product shots and the end card are free when the product has an image.

<a id="product-check"></a>
## The product in use

A **Product in use** scene is drawn with the product's first **Photo** as a reference, right after the portraits of the people in it and before the other shots of the video, together with how the product looks (from its sheet) and a fixed sentence: keep the product's shape, colors, label and proportions exactly as in the photo. A **Screen** is never sent to a model: a product whose images are all screens has no such scene, and its drawn scenes show screens unreadable. A scene made **Product in use** whose drawing was made without the photo reads **To draw**, and **Generate N images** draws it again with the photo, after showing the cost.

Every picture of the product in use waits for your check: the scene reads **Check product**, and its inspector shows it beside **The real product**. Compare shape, colors, label and proportions:

- **It matches** checks it. Only then is the scene animated on [Clips](clips.md#product-check), and only then may the video [render](render.md#gates).
- **Draw again** draws a new picture, paid, with its estimate; the new one waits for review, then for its own check.
- **Use the real image instead** makes the scene a [product shot](#ad), composed from the product's own image, for free.

Bardo does not judge likeness: the check is yours, by eye. A picture drawn again and used loses the check. A scene waiting for its check holds back only its own clip and the render, never the scenes around it. For a product of kind *idea*, the reference is its kept [concept image](products.md#concept), a drawing: the inspector names it **Concept image**, asks whether the product matches it, and offers **Use the concept image instead**.

<a id="shot-look"></a>
## How a product shot looks

A product shot (and an end card composed from a product image) has its own controls under **Scene kind**. Choosing a layout, a position or a backdrop composes the picture again, on your computer, for free; only drawing a new backdrop is paid.

- **Layout**, when the image offers more than one:
  - **Card**: the image with rounded corners. Every image may be a card.
  - **Cut out**: a **Photo** whose background is see-through (a PNG with transparency) stands on the backdrop with a soft shadow. It is what such a photo gets first. A photo with no see-through pixel is shown as a card.
  - **Device**: a **Screen** inside a plain outline Bardo draws: a phone for a tall screen, a browser window for a wide or square one, with no maker's marks. It is what a screen gets first.
- **Position**: **Left**, **Center** or **Right** within the room the frame leaves. The margins and the caption band stay clear, and the product stays on screen under the strongest zoom.
- **Backdrop**:
  - **From the image**: a deep or pale tone of the image's own color, as at first.
  - **Color**: one of the image's tones, white or black. Pick a second one under **Gradient to** for a gradient between the two, and its direction: top to bottom, left to right or diagonal. Picking the first color again under **Gradient to** makes it one color.
  - **Draw a backdrop** has the image generator draw an empty surface or set in the video's [style](#style) and shape, with no product, text or logo. It sees none of the product's images, and is told only what kind of product it is. It costs one scene image: the estimate is beside the button, it counts toward the video's [cap](projects.md#mode), and it shows in Costs as **Product shot backdrop**. The new backdrop waits for you under **New backdrop to review**: **Keep and use** puts it behind this scene, **Discard** deletes it. While one waits, **Draw a backdrop** waits too. Kept backdrops are listed under **Drawn backdrops** for every product shot of the project and cost nothing more; pick one to show it. **Discard** under a kept backdrop deletes it, unless a product shot still shows it.

Whatever the look, the product's image is only scaled: never stretched, cut or redrawn. A picture composed before looks existed reads **To compose** and is made again, for free, with **Compose product shots**.

<a id="redraw"></a>
## Drawing again

**Draw again** makes a new image from the scene's prompt, its place, its people and the other images of the video as they are now. The new image waits beside the current one: **Use new image** replaces it, **Keep current** throws the new one away. Until you choose, the scene reads **Review** and the stage says how many wait.

<a id="cast"></a>
## People who come back

When the same person appears in more than one scene (the chef, the detective), the plan lists them once with a fixed look: sex, age, skin tone, hair, build and clothes. Before the first scene that shows them is drawn, Nano Banana draws their **reference portrait**, and every scene with them is drawn with that portrait and their look, so they keep the same face from scene to scene. The portrait is billed like a scene image, and the estimate counts it.

A scene that shows someone who comes back lists them under **People in this scene**, with the portrait. **Edit look** changes the description; the portrait no longer matches it, so it goes, and the next scene drawn with that person draws a new one first. **Draw portrait again** makes a new portrait from the same look. Scenes already drawn keep their image either way, and Bardo marks them: a scene whose image was drawn with an older portrait of someone in it, or without their portrait, reads **Older face** and says so when you pick it, and the stage counts them under **Scenes showing an older face**, on the Clips stage too, so you see it before paying to animate one. **Draw them again** there draws all of them in one job, after showing what it costs; each new image waits for review beside the current one. While an image shows an older face, the next scenes are not drawn from it: only images that match the portraits hold the others together. They are not counted as missing images, so **Generate N images** leaves them alone.

Bardo does not draw a scene without the portrait of everyone in it who comes back: the image would be paid for and show another face. When a portrait cannot be drawn (the provider declines it, for instance), the person says why, the scenes that show them are left undrawn and say they wait for the portrait, and the other scenes are drawn as usual; the job's failure lists the scenes that wait. A portrait whose file can no longer be read is let go the same way: the person has none again, and it is drawn like any missing portrait the next time. Once one portrait a scene needs has failed, Bardo does not buy the other portraits that scene needs in the same job. Draw the portrait again, after editing the look if the provider declined it, then draw the scenes, or retry the job, which does both. If you would rather go without a portrait for a person, **Draw without a portrait**, offered for everyone, has their scenes drawn from the description alone: the person reads **No portrait**, no portrait is drawn, sent or billed for them, their face may change from scene to scene, and the scenes that waited for their portrait no longer do. **Use a portrait** takes that choice back, and the images drawn without one then read **Older face**.

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
