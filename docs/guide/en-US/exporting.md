---
id: exporting
title: Exporting
group: publishing
---

# Exporting

An export is a ready-to-post package: for each network, the rendered file and a text file with everything to paste in the network's upload form. It is how you post on X and Kick, which Bardo doesn't upload to, and it works for every network. Exports start at the [Publish](bardo:go/projects/publish) stage.

<a id="choose"></a>
## Choosing the networks

Each network's row has a box: tick the networks to export. Networks not exported yet, or whose last export is out of date, come ticked. A network can be exported once it has a rendered file and its metadata is written and within the network's limits ([Metadata and its limits](uploading.md#metadata)); until then its row says what it still needs, and saving or reverting unsaved edits comes first.

<a id="export"></a>
## Exporting

**Export (n)** runs one job for the chosen networks, with its progress on the stage and in Jobs, so you can keep working. **Cancel** stops it; **Resume** exports the networks left and keeps the finished ones. An export waits while the project is rendering, since the render rewrites the files it copies.

<a id="folder"></a>
## The folder

Each project's export has a folder, with one folder per network inside, named after the network. Each holds:

- the rendered video, in the network's preset;
- `metadata.txt`, in the interface language: the title, description or caption and tags to paste in each field, the visibility to pick, and, when the voice is synthetic, where to set the network's label.

**Show in folder** opens it, and **Details** shows its path. The inspector also has **Copy** beside each field, to paste straight from Bardo.

<a id="outdated"></a>
## Out of date

A network's last export reads **Exported** while it matches the render and the metadata, **Out of date** once either changed after it, and **Not exported** before the first one. Export again to bring the folder up to date. A file whose render is out of date exports as it is; render again at the [Render](render.md) stage to include the changes.

<a id="post"></a>
## After posting by hand

Once the post is up, paste its link in the network's **Post** section and select **Mark as posted**, so Bardo follows its numbers on [Performance](bardo:go/performance). See [The post](uploading.md#post).
