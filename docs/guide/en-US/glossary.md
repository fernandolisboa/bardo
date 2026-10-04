---
id: glossary
title: Glossary
group: reference
---

# Glossary

The words Bardo uses, in plain terms.

<a id="strategy"></a>
## Strategy

**Channel**: your brand: its niche, the themes it covers, its look, language, target country and default narrator. It has an account on each network it posts to.

**Niche**: a content market, like "space history".

**Market**: a channel's target country plus the language it speaks. Research is done per niche and market.

**Niche research**: a job that looks up recent YouTube uploads for your seed niches and scores each one. Results are kept for seven days.

**Competition**: from 0 to 100, how hard a niche is to stand out in. Many uploads, big channels and few views per video push it up.

**Trend**: from 0 to 100, how fast a niche's recent videos gather views.

**Opportunity**: the average of trend and the room competition leaves. Research ranks by it.

**Theme**: a video idea inside a niche: a title and an angle. Not to be confused with the interface theme, which is how Bardo looks.

**Theme ranking**: why the decision engine ranks an idea where it does: fit with the channel, trend and competition, each with a score and a confidence, plus past performance once the channel has published videos.

**Past performance**: how the channel's own videos did, as the ranking reads it: each video's first week next to the channel's usual.

**First week**: a video's views seven days after it went live.

<a id="production"></a>
## Production

**Persona**: a narrator you own: a voice, a tone, a writing style and voice settings. A channel has a default one, and each video can use another.

**Voice sample**: a short clip to hear a voice before narrating with it. The provider's stock preview is free; a reading of your own sentence with the persona's settings is a paid call.

**Persona package**: a persona saved to a `.bardo-persona` file to move to another computer. It never holds audio or keys.

**Video project**: one video in production, from the script to its posts. It starts when you approve an idea.

**Template**: the instructions Bardo sends the AI for one kind of text (the script, the image prompts, the post texts). Saving a change makes a new version; old ones stay readable.

**Script**: what the narrator says. Claude writes it, and you edit it as you like.

**Narration**: the script read aloud by the project's persona, with the timing of every word. A changed script makes it out of date.

**Word timings**: when each word of the narration is spoken. They drive the captions, the highlighted word and where cuts snap.

**Scene plan**: the narration split into scenes, each with an image prompt.

**Scene**: a stretch of the narration with one image, and optionally a clip.

**Clip**: a short video a video model makes from a scene's image. Optional.

**Motion prompt**: how a scene's image moves in its clip: subject, action, camera.

**Video model**: the model that makes clips, such as Kling through Higgsfield or Veo through Gemini. Set per channel, changeable per scene.

**Generation**: the record of how something was made: provider, model, prompt and template version. Editing the result leaves it as it was.

**Music prompt**: a prompt Claude writes for your own music tool. Bardo does not make music; you import the track.

<a id="editing"></a>
## Editing

**Timeline**: the video track and the audio tracks (narration, music, sound effects), with cuts, levels, fades and captions.

**Cut**: your edits to the timeline: split, trim, move, reorder and delete. Saved as you go, and undoable while the editor is open.

**Cut suggestion**: a place where the decision engine thinks the picture could cut, with a score and its reasons. Nothing changes until you accept one.

**Mix**: how loud each audio track plays, with mute and solo.

**Ducking**: lowering the music while the narrator speaks, by an amount you choose.

**Caption**: one line of on-screen text from the narration, timed to its words, with a style per channel or project.

**Framing**: the frame's shape (16:9 or 9:16) and how each clip fills it.

**Proxy**: a light copy of a clip that the preview plays, so editing stays smooth. The render always uses the originals.

**Render preset**: a network's output format: shape, resolution, codec, bitrate, longest length and loudness.

**Render**: making the final video file for each network.

**Render review**: what you check before a render: the cut, the mix, the captions and each target network.

**Quality gate**: a problem the render review flags, like a video too long for a network (which blocks that network) or captions turned off (a warning).

<a id="publishing"></a>
## Publishing

**Network**: a social platform: YouTube, TikTok, Instagram Reels, X or Kick.

**Network account**: a channel's profile on one network, with its post defaults and render settings.

**App credentials**: the id and secret of the app you register with a network so Bardo can post for you. Bardo ships none of its own.

**Network connection**: a network account signed in through that network, so Bardo can upload and read its numbers. Sign-ins are kept in Windows Credential Manager.

**Video metadata**: a network's post text for a video: title, description and tags, kept within the network's limits.

**Export**: a folder per network with the rendered file and a text file to copy from, for posting by hand.

**Upload review**: what you confirm before an upload: the file, the post text, the account, the visibility and the AI disclosure.

**Publication**: a video sent or scheduled to one network account, uploaded by Bardo or posted by you and linked by its address.

**Synthetic-content disclosure**: the label networks ask for on videos with a realistic AI voice. Bardo turns it on when the narration used a persona marked as a realistic voice.

**Due time**: when Bardo itself publishes a scheduled Instagram post. Bardo has to be open then.

**Missed post**: a scheduled post whose time passed while Bardo was closed. Bardo lists it when it opens, and you choose what to do.

<a id="numbers"></a>
## Numbers, jobs and money

**Metrics snapshot**: a post's numbers at one moment. Bardo keeps a history of them.

**Owner metrics**: what only the channel's owner sees on YouTube, like watch time and revenue, read through the connected account. They arrive two or three days late.

**Post insights**: the owner's numbers for an Instagram or TikTok post, read through the connected account.

**Metrics sync**: the job that reads your posts' numbers, when you ask and when Bardo opens.

**Job**: long work (a narration, images, a render, an upload) that runs in the background with progress, cancel and resume.

**Provider**: a paid AI or data service Bardo calls with your own key.

**Provider key**: your API key for one provider, kept in Windows Credential Manager.

**Budget**: a monthly spending limit per provider. Once reached, new jobs for that provider ask before starting.

**Decision engine**: the AI that ranks and scores (ideas, cuts) with typed answers and a confidence. It never writes content.

<a id="interface"></a>
## The app

**Layout**: where each screen places its parts: Workspace or Studio. It never changes what a screen does.

**Interface theme**: Bardo's colors, corners and font. Ten are included, from light and dark to high contrast and terminal looks.

**Guided tour**: a walk through Bardo on your own data: the window dims and one part at a time is lit, with a card that explains it. A tour never changes anything.

**Guide**: the place in the navigation for the tours, the user guide and the keyboard shortcuts.

**User guide**: this guide. The app shows it, and the same pages are in Bardo's documentation.

**Guide page**: one page of the user guide, in English and in Portuguese.
