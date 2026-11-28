# Changelog

What changed in each version of Bardo, newest first. Each version is a
[release on GitHub](https://github.com/fernandolisboa/bardo/releases) with the
installer, and its notes there are the section below.

## 0.5.1

### Rendering

- **Render without an account** (#360): the Render stage lists every
  network. A network with no connected account gets a **File** target that
  renders a video to download, share or post by hand and never uploads, so a
  channel with no accounts can render now. Publish lists those files with
  "post it yourself or add an account".
- **This video's render settings** (#360): each target's inspector changes
  frame, resolution, codec, bitrate, length limit and loudness for this
  video only, over the account's or the network's preset, with **Save** and
  **Restore defaults**. Changing them marks the last file out of date.
  Project packages carry these settings and file renders.

### Updates

- **What's new in a popup** (#359): **See what's new** on the update banner
  opens the release notes over the current screen, with Download, install
  review and Release page. The expand button goes to Settings › About.

### Upgrading from 0.5.0

The database updates itself on the first start. Existing renders and
accounts carry over.

## 0.5.0

### Projects

Work is organized by project: one is open at a time, with its own channels,
folder, world and libraries (#332).

- **Project switcher** (#333): open, create and star projects from the title
  bar, with favorites and recents. Channels belong to a project and can move
  to another one; Jobs and Costs filter by project.
- **Navigation by project** (#334): **Overview** is the new start screen,
  with the open project's videos in progress and each one's next step, the
  ideas waiting, the channels and this month's spending. Research and Themes
  are tabs of **Ideas**; Personas is now **Narrators** and Projects is now
  **Videos**. The **Global library** holds what every project can use.
- **Project folders** (#335): each project gets a folder with `Videos` and
  `World`, under a base folder chosen in Settings › Storage (`Documents\Bardo`
  by default, or your user folder when OneDrive syncs Documents). A folder
  that cannot be found marks the project **Folder not found** with
  **Locate…**, and its jobs wait. Backups carry each project folder.
- **Archive and delete** (#336): archive a project or a video to hide it and
  keep it read-only, optionally packing its media into `Videos.zip`. Delete
  asks for the name and sends the folder to the Recycle Bin. Costs keeps
  deleted work under its name, and amounts never change.
- **World** (#337, #338): characters, places and objects that come back
  across videos, each with a reference picture per look, drawn once and
  kept. **Who appears** picks them per video, objects may belong to a
  character, and scenes are drawn with their pictures.
- **Project libraries** (#339): narrators, products and templates belong to
  a project or to the global library. **Bring into the project** copies a
  global item into the open project.
- **Sharing between projects** (#340): import another project's shareable
  item as a linked copy, which follows every change, or as an independent
  one. **Make global** moves an item to the global library, and a
  `.bardo-item` file carries one as a single file.
- **Videos with no project** (#341): **No project** in the switcher holds
  videos started from a typed idea, with settings of their own; they export
  a file per network and move into a project's channel to be published.
  Themes gains **Write my own idea**.
- **Project packages** (#342): **Export project** writes a `.bardo-project`
  package with its records, the global items it uses and its media, never
  keys, tokens, costs or unfinished jobs. **Import a project package** brings
  one in as a new project. **Open a project from a folder** relinks a moved
  project, or brings back a deleted one whose folder you restored from the
  Recycle Bin.
- **Keys and budgets per project** (#343): Settings › This project takes an
  optional key per provider for the open project, kept in Windows Credential
  Manager, with a monthly budget for what it pays. Jobs pay with the
  project's key when it has one, else the app's; Costs splits the spend by
  key.

### Creation and editing

- **Models picked in steps** (#329): the channel defaults, a scene's model
  and a take's model are picked by provider, model, version and variant.
- **Project header** (#330): video, production and spend settings sit in a
  bar under the title, compact in windows narrower than 1,000 px.

### Fixes

- The first idea card on Themes keeps its title in wide windows (#328).
- Two videos drawn at the same time pay for a shared world portrait once
  (#351).

### Upgrading from 0.4.0

Projects, keys and connections carry over, and the database updates itself on
the first start. Bardo makes a project for each channel, named after it;
channels linked by a localized version share one project, named after the
oldest. Existing narrators, products and templates join the global library,
so nothing a channel or video used changes. Media of existing videos moves
from `%LOCALAPPDATA%\Bardo\projects` into each project's folder as a job
you can follow, cancel and resume on Jobs; other jobs wait until it ends.
Keys stay app-wide until you add a project key.

## 0.4.0

### Ads for a product

Make an ad for a product from a brief and its pictures (#272).

- **Products** (#273): a library of things to advertise (digital, physical,
  a service or an idea), each with a brief, the facts you stand behind, up to
  three images and a logo.
- **Product sheet** (#274): Claude researches the product on the web (at most
  8 searches and 5 pages) or writes from your brief, and every claim it finds
  waits for you to confirm or reject it. An ad states only confirmed claims.
- **Ad angles** (#275): Claude proposes angles for a channel's audience, the
  decision engine ranks them, and approving one starts an ad project at its
  new **Brief** stage.
- **Ad script** (#276): its own template, a claims check that marks any
  sentence no confirmed claim backs, and a new 15-second length.
- **Product shots** (#278, #285): scenes of the real product are composed by
  Bardo from your photo, with layouts and plain or drawn backdrops; your photo
  never goes to a model for them.
- **Product in use** (#279): drawn with the photo as a reference, and checked
  by you against it before its clip and the render.
- **Concept image** (#280): an idea with no photo gets a drawn concept image,
  marked **Concept** wherever it shows.
- **End card** (#281): call to action, link, offer and logo composed from a
  design per project.
- **Commercial disclosure** (#282): each post says whether the product is your
  brand or a paid partnership, as each network documents it, and the upload
  review lists every sensitive claim and unchecked text to acknowledge first.
- **Exports for ad managers** (#283): the cut in 4:5 and 1:1 besides the
  video's own shape, with ad texts of 125 and 40 characters.

### Scenes and clips

- **Storyboard** (#258): the scene plan tells one story with recurring places,
  and each image is drawn from the ones before it, so scenes follow on.
- **Visual styles** (#259): 26 styles, a default per channel and an optional
  one per video, added by Bardo to every image, portrait and thumbnail.
- **No scene without its portraits** (#269): a scene whose person has no
  portrait in the video's style is not drawn and costs nothing.
- **Shorter clips** (#260): a scene a little longer than a clip length gets the
  shorter clip, slowed to fit, up to the channel's stretch limit.
- **Google clips** (#298): Veo was shut down in the Gemini API; Google clips
  are Gemini Omni 1.2 Flash at 720p or 1080p, and a retired model is replaced
  by the one of the same resolution.
- **Higgsfield plan** (#255): spend your higgsfield.ai plan credits on clips by
  signing in under Settings › API keys › Plans you sign in to.
- **Video length** (#253): 30 seconds, 1 minute or 3 minutes per video.
- **Default video model** (#251): follows a provider whose key is saved.

### Spending

- **Spend cap per video** (#268): a new video starts with a cap of US$ 5
  (Short) or US$ 20 (long). A model with no known price always asks first, and
  a retry checks the cap again for what it still pays for.
- **Paid calls sent once** (#263, #264, #265, #266, #270, #271, #289, #290):
  a paid call is never sent twice, a lost answer is recorded in Costs, a
  stopping job keeps what it saved, and a retry leaves what a job delivered.
- **Long plans and stopped requests** (#267, #295, #309): long videos' scene
  plans stream, a cancelled job gives up its Claude request, and a paid answer
  is kept until it is applied.
- **Narration again** (#262): remaking the narration keeps paid scenes.

### Upgrading from 0.3.0

Projects, keys and connections carry over, and the database updates itself on
the first start. Channels keep their look under **From the channel's notes**.
A video made before this version keeps its spend cap and is offered the
default once. A channel or scene set to a Veo model uses Gemini Omni 1.2
Flash.

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
