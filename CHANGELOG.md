# Changelog

What changed in each version of Bardo, newest first. Each version is a
[release on GitHub](https://github.com/fernandolisboa/bardo/releases) with the
installer, and its notes there are the section below.

## 0.3.0

### Cost of a video

- **Production modes** (#248): each channel picks how much of its videos
  moves, and a project can pick its own in its header. **Economy** keeps still
  images with motion and no clips; **Animated hook** animates only the scenes
  in the first 10 seconds, with Kling 2.6 at 720p; **All animated** animates
  every scene with Kling 2.6 at 720p; **Premium** every scene with Kling 3.0
  at 1080p; **Channel model** every scene with the channel's clip model.
- **Whole-video estimate** (#248): the mode menu shows an approximate price
  for every mode, counting only what is left to make: script and plan,
  narration, pictures, portraits and clips. A mode whose clips need a key you
  have not saved says so.
- **Spend cap per video** (#248): set a cap in the project header, and a paid
  step that would take the video past it asks first, with what the rest of the
  video would cost.
- **Cheaper performed takes** (#243):
  - Kling 3.0 Motion Control straight from Kling, on the Kling key, with an
    optional draft on Kling 2.6. The control video is hosted in your BytePlus
    bucket when that key is saved, or else with Higgsfield's free upload on
    the Higgsfield key.
  - **Make final without a draft** where the final is a new take.
  - A test take: the first 5 seconds of a take at the model's cheapest tier,
    approved before any draft or final for the persona's look.
  - Preflight checks: one person, in frame, face large enough, steady motion
    and framed like the look. A take that fails one waits for **Send anyway**.
  - A cap on what a video's takes cost (US$ 10 unless you change it), with
    what they cost so far and what they still need on the Capture stage.

### Scenes and clips

- **Recurring people** (#246): the scene plan names the people who come back
  across scenes, each with one fixed look. Bardo draws a reference portrait of
  each before their first scene and draws every scene that shows them from it,
  so they look the same throughout. In the Scenes inspector you can edit a
  person's look or draw the portrait again.
- **Planned motion** (#246): the plan gives each scene a motion prompt, and
  its clip uses it unless you wrote your own.

### Upgrading from 0.2.0

Projects, keys and connections carry over. Existing channels use the Channel
model mode, so their videos are animated as before; new channels start on
Animated hook. Scene plans made before this version have no recurring people,
and their clips keep using the image prompt until the scenes are planned
again.

## 0.2.0

### Performance capture

Record yourself reading the script, and a persona performs it (#186).

- **Persona look** (#188): draw an original character for a persona, or use a
  photo of yourself. Real, well-known people and children are refused.
- **Captures** (#189): import a recording of yourself reading the script.
  Bardo checks that it follows the script, voices it in the persona's voice or
  cleans your own, and splits it into takes of 4 to 30 seconds at sentence
  ends.
- **Take analysis** (#190): face mesh, depth and person mask run on your
  computer, on the graphics card when there is one, with no upload. The
  installer now carries these models, so it is larger.
- **Control video** (#191): each take gets a control video (your recording,
  the face covered by a mesh, or a depth figure over your room or a picture)
  that you review and approve before anything is paid.
- **Performed clips** (#192, #194): Kling 3.0 Motion Control and Seedance 2.5
  Video Edit through Higgsfield, or Seedance 2.5 straight from BytePlus
  ModelArk, with a cheap draft before the final where the model has one.
- **On the cut** (#193): a performed clip goes on the cut where its take's
  words are said, stays in sync when the cut around it changes, and can be
  covered by a scene picture for a moment.

### Creation and editing

- **Video format** (#179): each channel and project is a Short (9:16, under a
  minute) or a Long video (16:9, 8 to 10 minutes). The script, the pictures,
  the clips and the cut follow it.
- **Localized versions** (#184): make a version of a project for a channel in
  another language or country. The script is adapted sentence by sentence, the
  scenes, clips and music are reused, and the scenes are placed on the new
  narration.
- **Generated music** (#169): ElevenLabs composes an instrumental track as
  long as the narration from the music prompt; you review it before it goes on
  A2.
- **Video transitions** (#167): ten kinds between clips, with a default per
  channel.
- **Motion on stills** (#180): Ken Burns zooms and pans on scene pictures,
  per still or varied across the cut.
- **Spoken word in captions** (#181): the word being said lights up in the
  caption.
- **Kling direct** (#168): scene clips straight from Kling's own API, with a
  Kling key.
- **Decision engine** (#164): Settings › API keys names the engine that ranks
  themes and scores cuts (JEV for now).

### Publishing and metrics

- **YouTube thumbnail and chapters** (#182): design a thumbnail from a scene
  picture or a drawn background, and add chapters to the description of long
  videos.
- **Retention over the cut** (#183): YouTube's audience retention drawn over
  the editor's timeline, with the steepest drops tied to their scene and
  sentence.

### Fixes

- A `.jpg` imported into the editor no longer becomes a 0.04 s video (#175).
- The editor keeps its parts whole in the narrowest window (#177).
- A job whose final save failed no longer runs a second time (#199).
- Saved keys are read back from Windows Credential Manager before they count
  as saved (#200).

### Upgrading from 0.1.1

Projects, keys and connections carry over. Channels made before this version
keep how their videos looked: Long format, no motion on stills, and captions
without the spoken word. New channels start as Shorts, with varied motion and
the spoken word in yellow.

## 0.1.1

- **Settings file** (#162): Settings › Backup › Settings file exports the
  keys, app credentials, preferences, budgets, personas and channels you tick
  into a password-protected file, and imports one after you tick what comes
  in. A new copy of Bardo offers it on its first start, and the installer can
  take a settings file on a first install.
- **A copy built from source keeps its own data** (#162): it uses its own
  `Bardo Dev` folders and background agent task, and shows **Dev** in its
  title bar.

## 0.1.0

The first release: niche research and theme ranking, scripts, personas and
narration, scenes, images and clips, the editor with captions, mix and
framing, render and export, uploads and scheduling for YouTube, Instagram
Reels, TikTok and X, metrics, the background publishing agent, backup and
restore, diagnostics and updates from GitHub.
