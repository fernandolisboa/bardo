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

**Plan scenes** has Claude split the narration into scenes on its sentences and write an image prompt and a [motion prompt](clips.md#motion) for each one, from the **image prompt template** ([Templates](templates.md)) and the channel's aesthetic notes. It also lists the [people who come back](#cast) in more than one scene.

**Generate N images** then draws every scene still without an image, in one job. Nano Banana draws each at 2K in the shape of the video's [format](projects.md#format), 9:16 for a Short and 16:9 for a long video, and Gemini bills each one; a scene the provider declines does not stop the others. The estimate under the leading button says what the run costs. Planning needs a Claude key, and drawing a Gemini key ([Your API keys](api-keys.md#providers)).

<a id="list"></a>
## The scenes

The scenes appear in order, each with its time, its image and where it stands: **To draw**, **Review**, **Failed** or ready. Pick a scene to see it in full; ↑ and ↓ move through them, and Enter uses the new image of a scene under review.

<a id="scene"></a>
## A scene

The picked scene shows the narration it covers, its image and its **image prompt**. **Edit prompt** changes what the next drawing shows (up to 4,000 characters); a scene whose prompt you changed is marked **Edited**. **Generation details** says which model drew the image, with how many tokens and which template version.

<a id="redraw"></a>
## Drawing again

**Draw again** makes a new image from the scene's prompt. The new image waits beside the current one: **Use new image** replaces it, **Keep current** throws the new one away. Until you choose, the scene reads **Review** and the stage says how many wait.

<a id="cast"></a>
## People who come back

When the same person appears in more than one scene (the chef, the detective), the plan lists them once with a fixed look: sex, age, skin tone, hair, build and clothes. Before the first scene that shows them is drawn, Nano Banana draws their **reference portrait**, and every scene with them is drawn with that portrait and their look, so they keep the same face from scene to scene. The portrait is billed like a scene image, and the estimate counts it.

A scene that shows someone who comes back lists them under **People in this scene**, with the portrait. **Edit look** changes the description; the portrait no longer matches it, so it goes, and the next scene drawn with that person draws a new one first. **Draw portrait again** makes a new portrait from the same look. Scenes already drawn keep their image either way: draw them again to use the new portrait. A portrait the provider declines is marked on the person, and their scenes are drawn from the description alone.

<a id="filter"></a>
## All or pending

Above the scenes, **All** shows every one and **Pending** only those with something left: an image to draw or to review (at the Clips stage, a clip to make or to review). Each tab says how many it holds.

<a id="replan"></a>
## Planning again

When the narration is made again, the scenes' times no longer match and the stage reads **Out of date**. **Plan again** replaces the scenes with new ones. If they already have images or clips, Bardo asks first, since planning again discards those files.

<a id="place"></a>
## Placing the scenes on a version

A [version in another language](script.md#versions) starts with the original's scenes, still on the original's times. Once its own narration is made (generated or imported), the stage reads **Scenes to place** and offers **Place scenes on this narration**. Each scene keeps its image, clip and prompts and moves to the new time of the sentence it started at; its text becomes the new narration's words. Sentences are found by matching the words of the adaptation with what was narrated, so editing the script first is fine.

Placing also builds the version's cut from the original's: each scene's framing, motion and transition, the scenes it left out, the caption style and highlight, the mix and the frame shape. Music and SFX move to the same point of the narration (music keeps its start and end on the words, an SFX its length). Captions come from the new narration. Splits and trims inside a scene, imported footage placed by hand and edited caption text are not carried; redo them in the [editor](editor.md).

Scenes stretch or shrink with their sentences. **Worth a look** lists the scenes that now run past their clip (the clip holds its last frame), those that grew by more than half or shrank by more than a third, and those the version does not narrate, which are left out of the cut. After a new narration, placing is offered again; **Plan again** replaces the scenes as in any project.
