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

**Plan scenes** has Claude split the narration into scenes on its sentences and write an image prompt for each one, from the **image prompt template** ([Templates](templates.md)) and the channel's aesthetic notes.

**Generate N images** then draws every scene still without an image, in one job. Nano Banana draws each as a 16:9 image at 2K, and Gemini bills each one; a scene the provider declines does not stop the others. The estimate under the leading button says what the run costs. Planning needs a Claude key, and drawing a Gemini key ([Your API keys](api-keys.md#providers)).

<a id="list"></a>
## The scenes

The scenes appear in order, each with its time, its image and where it stands: **To draw**, **Review**, **Failed** or ready. Pick a scene to see it in full; ↑ and ↓ move through them, and Enter uses the new image of a scene under review.

<a id="scene"></a>
## A scene

The picked scene shows the narration it covers, its image and its **image prompt**. **Edit prompt** changes what the next drawing shows (up to 4,000 characters); a scene whose prompt you changed is marked **Edited**. **Generation details** says which model drew the image, with how many tokens and which template version.

<a id="redraw"></a>
## Drawing again

**Draw again** makes a new image from the scene's prompt. The new image waits beside the current one: **Use new image** replaces it, **Keep current** throws the new one away. Until you choose, the scene reads **Review** and the stage says how many wait.

<a id="filter"></a>
## All or pending

Above the scenes, **All** shows every one and **Pending** only those with something left: an image to draw or to review (at the Clips stage, a clip to make or to review). Each tab says how many it holds.

<a id="replan"></a>
## Planning again

When the narration is made again, the scenes' times no longer match and the stage reads **Out of date**. **Plan again** replaces the scenes with new ones. If they already have images or clips, Bardo asks first, since planning again discards those files.
