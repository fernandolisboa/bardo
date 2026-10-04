---
id: clips
title: Clips
group: production
place: projects/clips
tour: clips
---

# Clips

A clip animates a scene's image into a short video. Clips are optional: a scene without one shows its still image in the video. They live at the [Clips](bardo:go/projects/clips) stage, which opens once a scene has an image ([Scenes and images](scenes.md)). [Show me the stage](bardo:tour/clips).

<a id="animate"></a>
## Animating scenes

**Animate scenes without a clip** sends every scene that has an image and no clip to the video provider in one job; **Animate** on a scene does just that one. Each clip runs as long as its scene is narrated, within the lengths the model allows. A scene that fails does not stop the others, and retrying the job sends only the scenes still without a clip.

Clips are made through Higgsfield or Google, with your own key for each ([Your API keys](api-keys.md#providers)). Cancelling a run stops the waiting, not the provider: a clip already sent may still finish and be billed.

<a id="motion"></a>
## Motion

The **motion prompt** says how the image moves: the subject, the action, the camera. Until you write one, the scene uses its image prompt, marked **Same as the image prompt**. **Edit motion** changes it for the scene's next clip.

<a id="model"></a>
## Video model

Each scene uses its channel's video model, set on [Channels](bardo:go/channels), unless you pick another one for it under **Video model**. Models differ in look, in the lengths they accept and in price. If a scene's model is no longer offered, the scene says so; pick another one.

<a id="review"></a>
## Reviewing a clip

A new clip waits beside the current one as **New clip to review**. **Play** it first, then **Use new clip** or **Discard**. A scene's clip shows its length and model; **Use the still image** drops the clip and goes back to the image. When the scene's image changes after its clip was made, the clip is marked **Image changed**.

<a id="cost"></a>
## What a clip costs

Under the model, **Next clip** says how long the scene's next clip will run and what it costs at the provider's rate; the estimate under **Animate scenes without a clip** adds them up for the run. The provider bills each second. A model with no rate in [Costs](bardo:go/costs) says so, and you can add its price there.
