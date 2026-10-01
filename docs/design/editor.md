# Editor screen: design handoff

- Status: Approved by the owner, 2026-10-01 (#19)
- Design canvas: https://claude.ai/artifact/5S2wfsNpN3Mx2MihVAoKep (six artboards, 1440×900)
- Input for: #20 rough cut, #21 cut editing, #22 audio mix, #23 captions, #24 framing, #25 imported media, #30 AI cut suggestions

The canvas is the visual reference. This file is the contract the editing slices implement: layout, components, states, interactions and shortcuts. Where the two disagree, update both.

## Artboards

| Artboard | Shows |
| --- | --- |
| Editor · 16:9 · en-US | Populated project, V1 clip selected (clip inspector), one AI cut suggestion focused with its popover |
| Editor · 9:16 · pt-BR | Same project in Portuguese, 9:16 preview with crop window over the dimmed 16:9 source, caption selected (caption inspector), music ducking readout |
| Empty: no narration yet | No clips; empty state in the timeline, inspector placeholder, render disabled |
| Loading: proxies building | Timeline editable while proxies build; preview shows progress; clips without proxy are hatched |
| Error: missing media and failed proxy | Banner above the timeline, missing and failed clips marked, preview shows "Media offline" |
| AI cut suggestions: review | Suggestions mode: pins on the timeline, inspector replaced by the suggestions list |

## Layout

Top to bottom, filling the window:

1. **Top bar** (44 px): app name, breadcrumb `Channel › Video title`, save state and duration (`Saved · 01:04 · 30 fps`), jobs indicator (opens the jobs panel, #8), undo/redo, primary button **Review & render** (opens the review screen, #27; disabled when the timeline is empty).
2. **Middle row**: three columns `260 px | 1fr | 320 px`, separated by 1 px hairlines.
   - **Bin** (left): tabs Scenes · Media · Caption styles; search; scene rows (thumbnail, `Scene N`, duration, prompt summary). Dragging a scene or media item onto a track inserts it.
   - **Preview** (center): header with aspect switch `16:9 | 9:16`, fit menu and a `Proxy` quality badge; the frame; transport row (previous frame, play/pause, next frame, loop, volume, full screen) with timecode `HH:MM:SS:FF / duration`.
   - **Inspector** (right): properties of the current selection, or a placeholder ("Select a clip to edit its properties").
3. **Timeline** (bottom, about 40% of the height):
   - Toolbar: playhead timecode (in the 200 px header column), Select (V) and Split (S) tools, toggles **Snap to words**, **AI cut suggestions** (with pending count) and **Duck music under narration**, zoom out / slider / zoom in.
   - Ruler with timecodes and AI suggestion pins.
   - Tracks, each with a 200 px header (type chip, name, M/S, gain readout):

     | Chip | Track | Height | Content |
     | --- | --- | --- | --- |
     | CC | Captions | 30 px | caption chips with text |
     | V1 | Video | 64 px | clips with thumbnail strip and `N · scene name` |
     | A1 | Narration | 82 px | waveform plus word markers (labels on two alternating rows; hidden when zoomed out) |
     | A2 | Music | 54 px | waveform plus ducking envelope dipping under narration; header shows `−6.0 dB · Duck −12 dB` |
     | A3 | SFX | 34 px | short clips |

   - Playhead: one accent line across ruler and all tracks.

## Visual tokens

| Token | Value | Use |
| --- | --- | --- |
| app | `#0F1113` | window ground |
| panel | `#16191C` | panels, track headers |
| raised | `#1E2226` / `#262B30` | inputs, buttons, popovers |
| border | `#2A2F35` / `#3A4048` | hairlines / control outlines |
| text | `#E7E9EC` / `#A3AAB3` / `#8A929C` | primary / secondary / tertiary |
| accent | `#F2A33A` (ink `#1A1206`) | playhead, selection, focused suggestion, primary button. Nothing else. |
| error | `#E5534B` on `#2A1416` | error banner, broken clips |
| video | fill `#2B3D54`, edge `#4F6E94` | V1 clips |
| narration | `#2FA295`, fill `#14302D` | A1 |
| music | `#BBAEF7`, fill `#231F36` | A2 |
| sfx | fill `#7A4230`, edge `#C8664A` | A3 |
| captions | `#D2D6DC`, ink `#121417` | CC chips |

Track colors differ in lightness as well as hue, so they stay distinguishable for color-blind users. Fonts: IBM Plex Sans (UI, 12–13 px), IBM Plex Mono (timecodes, gains, scores). Icons are stroke icons. Toolbar targets are 32 px.

## Inspector contents

- **Clip (V1)**: name; source provenance (e.g. "Nano Banana image · Animated with Higgsfield · Kling 2.5", file, resolution, fps); In / Out / Duration; Crop position `Fit | Fill | Custom` with X/Y (applies in 9:16; in 16:9 the clip fits the frame); Speed; **Replace asset**, **Regenerate** (#15, #16).
- **Caption (CC)**: text; In / Out / Duration with "snapped to words" hint; channel caption styles as swatches (three per channel); vertical position (Top / Center / Bottom plus offset); "Apply style to all captions".
- **Audio region (A1–A3)**: gain, fade in / fade out; on A2, ducking amount (#22).
- **Missing media**: alert at the top of the clip inspector with **Relink…**; crop and speed hidden.

## States

| State | Timeline | Preview | Other |
| --- | --- | --- | --- |
| Empty | "Nothing to cut yet. The rough cut is built from your scenes and narration. Generate narration to start." with **Go to narration** and **Import media** | neutral empty frame, timecode `00:00:00:00` | render disabled, inspector placeholder |
| Proxies building | fully editable; clips without proxy hatched with a spinner | "Building preview proxies… 7 of 18 clips", progress bar, "You can keep editing while this runs" | jobs indicator `Proxies 7/18`, preview badge `Proxy 7/18` |
| Error | banner: "2 clips need attention" naming each file and cause, with **Relink…**, **Retry proxy**, **Show details**; missing clip: solid red outline, darker fill, "Media missing"; failed proxy: dashed red outline, hatched, "Proxy failed" | "Media offline" with path and **Relink…** / **Regenerate** | jobs indicator "1 job failed" |

Editing never waits on proxies or jobs (story 69).

## AI cut suggestions (#30)

- Each candidate is a diamond pin on the ruler with its score, plus a dashed guide down the tracks.
- The focused pin is accent-colored and opens a popover: "Cut here?", timecode, score, typed reasons from the decision engine (e.g. "Sentence end + 420 ms pause", "Scene boundary in script", "Topic shift (keyword: 'Ohio')"), **Accept (A)**, **Reject (R)**, **Next (Tab)**.
- Accepting splits the clip at that point (drawn as a solid cut with a check). Rejecting removes the pin.
- Suggestions mode replaces the inspector with a list: timecode, score bar, reasons, accept/reject per row; accepted rows show a check, rejected rows are struck through with **Undo**; **Accept all above 0.80** at the top.
- The toolbar toggle hides pins without discarding them; its badge counts pending suggestions.

## Interactions

- Snapping (on by default) snaps cuts, trims and caption edges to narration word boundaries; Alt while dragging disables it.
- Split, trim, reorder and delete on V1 and audio tracks; every edit is undoable (#21).
- The aspect switch changes the preview and enables per-clip crop; the crop window can be dragged in the preview.
- Selecting a caption chip, a clip or an audio region drives the inspector; clicking empty timeline clears the selection.

## Keyboard shortcuts

Shortcuts are scoped to the focused panel (timeline or preview), never global, so text fields keep normal typing.

| Key | Action |
| --- | --- |
| Space | Play / pause |
| S | Split at playhead |
| V | Select tool |
| Delete | Remove selection |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| J / K / L | Shuttle back / stop / forward |
| ← / → | Step one frame |
| Alt+drag | Disable snapping while dragging |
| A / R | Accept / reject the focused AI suggestion |
| Tab | Next suggestion |

## Strings

The pt-BR artboard checks that labels fit at Portuguese length (e.g. "Sugestões de corte da IA", "Encaixar nas palavras", "Abafar música sob a narração", "Revisar e renderizar"). All of them go to `crates/app/locales/*.toml`, never inline.
