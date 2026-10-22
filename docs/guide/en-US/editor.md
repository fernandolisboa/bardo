---
id: editor
title: The editor
group: editing
place: projects/edit
tour: editor
---

# The editor

The editor is where the scenes and the narration become a cut: you trim it, mix its sound, caption it and frame it, then send it to the render. It opens over the whole window from the [Edit](bardo:go/projects/edit) stage of a project, once the scenes are planned and narrated, and **Projects** at its top left takes you back. The cut saves itself after every edit. [Show me the editor](bardo:tour/editor), or [part 2 of the tour](bardo:tour/editor-more) for the mix, captions, framing and cut suggestions.

F1 opens this guide over the editor, and **Back to the editor** returns to it as you left it. Esc closes a tour; the **Editor tour** button in the top bar (Shift+F1) takes it back to the step it closed on.

<a id="preview"></a>
## Preview and playback

The preview in the middle plays the cut from light copies of your images, clips and narration (**Proxy**), so it starts at once while the render uses the full files. Bardo builds those copies in the background when the editor opens; you can edit meanwhile, and the preview plays once they are ready. **Space** plays and pauses, **←** and **→** step one frame, and **Home** and **End** jump to the start and the end.

A clip whose file is gone, or whose copy failed, shows in red with the reason above the timeline; **Retry proxy** builds the copy again.

<a id="timeline"></a>
## Timeline and tracks

The cut runs left to right on five tracks:

| Track | Holds |
| --- | --- |
| CC | Captions, following the narration's words |
| V1 | The video: one clip per scene, then any footage you add |
| A1 | The narration, with its waveform and words |
| A2 | Music |
| A3 | Sound effects |

Click the timeline to move the playhead, and click an item to select it: the playhead goes there and the inspector on the right shows its properties. **Esc** clears the selection. Ctrl and the scroll wheel zoom, or use − and + at the end of the toolbar.

The bin on the left lists the project's **Scenes** (click one to jump to its clip) and its **Media**: **Import files…** adds video and audio files from your computer, and each file's buttons put it on a track at the playhead. Music generated at the Script stage shows here once you use it ([Music from the prompt](script.md#generated-music)).

<a id="cuts"></a>
## Cutting

- **Split (S)** cuts the selected item at the playhead, or the clip under the playhead when nothing is selected.
- **Trim** an item by dragging either end, or press **[** or **]** to move its start or end to the playhead.
- **Reorder** a clip by dragging it between two others, or with **Alt+←** and **Alt+→**. On the audio tracks, the same keys move the selected piece one frame.
- **Delete** (or **Remove from the cut** in the inspector) takes the selection out.

The video track closes its gaps: trimming or removing a clip moves the clips after it. Audio pieces move freely and stop at their neighbours, and removing one leaves silence.

<a id="transitions"></a>
## Transitions

A cut goes straight from one clip to the next. A transition blends them instead, for a moment around the cut:

| Transition | What it does |
| --- | --- |
| Crossfade | The next clip fades in over the one before |
| Through black, Through white | Down to black (or up to white), then into the next clip |
| Wipe ← → | An edge moving across uncovers the next clip |
| Slide ← → ↑ ↓ | The next clip pushes the one before out of the frame |
| Zoom | The clip before zooms in and gives way to the next |

**Transition (T)** in the toolbar puts the channel's default transition (set on the [channel](channels.md#defaults)) on the cut nearest the playhead and selects the clip after it; a cut that already has a transition keeps it. With a clip selected, the inspector's **Transition in** picks how the picture comes into it: **Cut** or one of the transitions, its **Length** (0.1 to 2 s), and **Use on every cut** to put the same choice on all of them. A transition shows on V1 as a band across its cut; click it to select the clip it leads into.

A transition is centered on its cut: the clip before plays on for half of it and the clip after starts half of it early. The cut does not move, so the video stays as long as it was and in sync with the narration. Where a clip's file has no picture past its edge, its edge frame holds for that half. A transition never runs longer than the clips beside it: next to a short clip it plays shorter, and the inspector says for how long. The first clip opens the video and has no transition in. Splitting a clip leaves a plain cut between the halves, and a transition moves with its clip when you reorder.

The preview and the render draw transitions the same way.

<a id="snapping"></a>
## Snapping to words

With **Snap to words** on, cuts, trims and caption edges land between the narration's words when they come close, so a cut never clips a word; a thin line marks the word while you drag. Hold **Alt** while dragging to place freely, or turn the switch off.

<a id="undo"></a>
## Undo and redo

Every edit, including transitions, the mix, captions and framing, can be undone with **Ctrl+Z** and redone with **Ctrl+Y** (or **Ctrl+Shift+Z**), or with the arrows at the top. The history lasts while the editor is open. If the scenes or the narration change after you edited, the editor says so and the cut starts over from the rough cut.

<a id="mix"></a>
## The mix

Click an audio track's header to pick the track: the inspector shows its **Level** (−30 to +12 dB), **Mute** and **Solo**. **M** and **S** on the header do the same. A track that will not play, muted or left out by another track's solo, dims its name.

Select an audio piece to set its **Fades**: a fade in at its start and a fade out at its end, drawn as ramps over the piece.

<a id="ducking"></a>
## Ducking

**Duck music under narration** lowers the music while the narration speaks and brings it back in the pauses, following the words. Pick the music track (A2) to set **Duck by**, from 1 to 30 dB; the track's header shows the depth while ducking is on.

<a id="captions"></a>
## Captions

Captions are made from the narration's words and sit on the CC track. Select one to edit it in the inspector:

- **Text**: Enter applies it; the timing stays.
- **In** and **Out**: drag the caption, or either of its ends, on the track; its edges snap to the words.
- **Style**: pick one of the caption styles. The style applies to every caption in the project.

**Remove from the cut** takes a caption out.

**Show captions** turns them all on or off in the preview and the render; the [Render stage](render.md#review) says when they are off.

<a id="framing"></a>
## Framing: 16:9 or 9:16

The switch over the preview sets the cut's frame shape: 16:9 (landscape) or 9:16 (vertical). A new cut starts in the shape of the video's [format](projects.md#format): 9:16 for a Short, 16:9 for a long video. Changing it is an edit you can undo.

In 9:16, a clip whose picture is 16:9 is cropped from it. Select a clip and keep the playhead on it: the preview shows the whole picture, dimmed outside the 9:16 window. Drag the window to choose what stays in view, or use the inspector's **Framing**: **Fit** shows the whole picture with bars above and below, **Fill** crops at the center, and moving the window makes it **Custom**. A 9:16 render for a network goes through each clip's crop window too; a 16:9 render shows each picture whole, a tall one with bars at the sides.

<a id="suggestions"></a>
## Cut suggestions

**AI cut suggestions** shows where a cut would land well. **Suggest cuts** sends the points where a sentence ends, the narrator pauses or the scene changes to the decision engine (TypeSafe, with your own key), which scores each one; the estimate beside the button says what it costs. Nothing changes until you accept one.

Each suggestion is a pin on the ruler with its score. Click a pin to see why (**Sentence end**, a pause, **Scene change**, **Topic shift**), then **Accept (A)** to split there or **Reject (R)** to drop it; **Tab** moves to the next one. While suggestions show, the list replaces the inspector: **Accept all above** a score, **Show from** to hide weak ones, and **Undo** on any row you decided. **Suggest again** scores the points without a cut and replaces the list.

<a id="render"></a>
## Leaving to render

**Review & render** at the top right closes the editor and opens the project's [Render stage](render.md), where the cut is checked and rendered for each network. It shows once the cut has something in it.
