---
id: exporting
title: Exporting
group: publishing
---

# Exporting

An export is a ready-to-post package: for each network, the rendered file and a text file with everything to paste in the network's upload form. It is how you post on Kick, which Bardo doesn't upload to, and it works for every network. Exports start at the [Publish](bardo:go/projects/publish) stage.

<a id="choose"></a>
## Choosing the networks

Each network's row has a box: tick the networks to export. Networks not exported yet, or whose last export is out of date, come ticked. A network can be exported once it has a rendered file and its metadata is written and within the network's limits ([Metadata and its limits](uploading.md#metadata)); until then its row says what it still needs, and saving or reverting unsaved edits comes first.

<a id="export"></a>
## Exporting

**Export (n)** runs one job for the chosen networks, with its progress on the stage and in Jobs, so you can keep working. **Cancel** stops it; **Resume** exports the networks left and keeps the finished ones. An export can't start until a render of the project ends, since the render rewrites the files it copies.

<a id="folder"></a>
## The folder

Each project's export has a folder, with one folder per network inside, named after the network. Each holds:

- the rendered video, in the network's preset;
- `metadata.txt`, in the interface language: the title, description or caption and tags to paste in each field, the visibility to pick, and, when the voice is synthetic, where to set the network's label; for an ad, its commercial disclosure and what to turn on when you post, and the items you acknowledged before exporting ([Before an ad goes out](uploading.md#ad-review));
- in the YouTube folder, `thumbnail.jpg` when the project has a saved [thumbnail](uploading.md#thumbnail), to set in YouTube Studio.

**Show in folder** opens it, and **Details** shows its path. The inspector also has **Copy** beside each field, to paste straight from Bardo.

<a id="outdated"></a>
## Out of date

A network's last export reads **Exported** while it matches the render, the metadata and the YouTube thumbnail, **Out of date** once one of them changed after it, and **Not exported** before the first one. Export again to bring the folder up to date. A file whose render is out of date exports as it is; render again at the [Render](render.md) stage to include the changes.

<a id="ads"></a>
## Export for ads

An ad has a second export at the Publish stage, **Export for ads**, for the ad managers where you run it as a paid ad. Bardo never sends anything to an ad manager: you upload the files yourself.

- **Shapes.** It renders the cut in 4:5 (1080 × 1350) and 1:1 (1080 × 1080), the shapes ad managers ask for, and in the editor's other shape (16:9 for a vertical cut, 9:16 for a landscape one). A shape not exported yet, or out of date, comes ticked.
- **Composed pictures.** Product shots and the end card are composed again for each shape, so the product, its text and the button stay whole.
- **Windows.** Every other picture and clip fills the frame through a window, centered unless you move it. **Window per clip** shows each clip's window in the picked shape; the arrows move it and **Center** puts it back. A window is kept per clip and per shape. In 16:9 every picture shows whole, with bars where it does not reach the frame.
- **Captions** are laid out again for the shape, and none show over the end card.
- **Texts for the ad manager.** The job that writes the post texts also writes a primary text (at most 125 characters, shown above the video) and a headline (at most 40, beside the button), once for every shape, and the [check of the post texts](uploading.md#ad-review) reads them too. Edit them on the card and select **Save texts**; the export waits while one is empty or too long.

The export waits for the product-in-use scenes you have yet to check and for a composed picture that has to be made again, and goes with the items you acknowledged in the ad's review. **Export for ads** runs one job, with its progress on the stage and in Jobs; **Cancel** stops it, and exporting again finishes the shapes left.

Each shape gets a folder in the project's export folder, `Ads/4x5`, `Ads/1x1` and `Ads/16x9` or `Ads/9x16`, with the video and `ad.txt`: the file, its size and shape, the primary text, the headline, the call to action and link from the brief, the commercial disclosure to set, the synthetic-content label when the video needs it ([Synthetic content](uploading.md#disclosure)), and the items you acknowledged. **Show in folder** opens it. A shape reads **Exported**, **Out of date** once the cut, a composed picture or the texts changed after it, or **Not exported**.

<a id="post"></a>
## After posting by hand

Once the post is up, paste its link in the network's **Post** section and select **Mark as posted**, so Bardo follows its numbers on [Performance](bardo:go/performance). See [The post](uploading.md#post).
