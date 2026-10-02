# Spike: ffmpeg for proxies, preview and render (issue #18)

Date: 2026-10-02. Decision: [ADR-0007](../adr/0007-ffmpeg-sidecar.md).

The spike ran in a 4-core Linux container with no GPU, using the same FFmpeg release Bardo pins for Windows (8.1.3, LGPL). Linux numbers are an approximation of the shape of the costs, not of the reference machine: the reference machine has 16 cores and an RTX 3080 Ti, and Windows process start and pipes behave differently. What only Windows can answer is listed at the end and tracked with the creation-phase validations (#51).

## What was built

`bardo-media` gains `media::ffmpeg`, a sidecar wrapper over the bundled executables:

| Call | What it does |
| --- | --- |
| `Ffmpeg::locate` | Finds `ffmpeg` and `ffprobe` (env var, next to `bardo.exe`, PATH) and checks the version |
| `probe` | Duration, first video stream (codec, size, frame rate), first audio stream, from `ffprobe` JSON |
| `frame_at` | One frame at a time position, scaled, as BGRA (thumbnails, scrubbing) |
| `frames` | A stream of BGRA frames of one file from a point, at a steady rate |
| `waveform` | Peaks per second of the first audio stream |
| `build_proxy` | 540-line MJPEG (or H.264) proxy with progress and cancel |
| `detect_encoders` | Which H.264 encoders work here, best first (trial encode each) |
| `render` | A `RenderPlan` (clips back to back, crop or fit per clip, audio tracks with placed clips and gains) to MP4, two-pass loudness (measure, then one gain and a true-peak limiter), progress and cancel |
| `preview` | The same plan from the playhead on, as BGRA frames at preview size |
| `measure_loudness` | Integrated loudness, true peak and range of a file (for the review screen) |

Tests: unit tests for every piece that is pure (version parsing, `ffprobe` JSON, progress lines, filter graphs, plan slicing, loudnorm parsing, peaks), and integration tests over four clips in `crates/media/tests/fixtures` (about 120 KB in all) that check probe, frames, waveform, both proxy codecs, a 9:16 render from two clips and two audio tracks (duration, size, codec, frame rate, AAC 48 kHz stereo, integrated loudness within 1 LU of -14 LUFS, true peak under the target), a render with every working encoder, cancel leaving no file, and preview from the middle of the timeline.

Tooling: `cargo xtask fetch-ffmpeg` downloads the pinned archive for the platform, checks its SHA-256 and unpacks `bin/` into `.ffmpeg/bin` (git-ignored). CI caches it by the manifest's hash.

## Integration options

Sidecar executables won over bindings (`ffmpeg-next`). The deciding points:

- **Build.** Bindings need FFmpeg headers and import libraries plus LLVM for bindgen on the Windows runner, at versions matching the shipped DLLs, and the binding crates trail new FFmpeg majors. The sidecar needs a download.
- **Isolation and cancel.** A decoder crash in a child process is an error message; in-process it is a crashed app. Cancel is a kill.
- **Features for free.** Every operation above is a filter graph or a couple of flags; with bindings each one is hand-written decode, filter and mux code.
- **What bindings would buy:** no process start per call, and frames that never leave the GPU. Process start is tens of milliseconds and calls are coarse; the GPU path is only needed if the CPU copy is too slow (below).

## Builds and licensing

| Source | License | Pinnable | Notes |
| --- | --- | --- | --- |
| BtbN/FFmpeg-Builds | GPL or LGPL, static or shared | Month-end builds kept 2 years, dailies 14 days; `checksums.sha256` per release | LGPL win64 for 8.1, 9.0 and master only; NVENC, AMF, QSV, Media Foundation, OpenH264, `loudnorm` |
| gyan.dev / GyanD/codexffmpeg | GPL only (includes x264) | Versioned tags, kept | No LGPL option |
| GitHub `windows-latest` image | — | — | No ffmpeg preinstalled |

Chosen: BtbN 8.1.3 LGPL shared, the September 2026 month-end build. LGPL keeps Bardo's license free (no x264; software H.264 comes from OpenH264 and Windows' Media Foundation). NVENC in FFmpeg 8.x needs NVIDIA driver 570 or newer; 9.0 needs 610, which would push more users to software encoding. Before the first installer ships, the archive is copied to Bardo's own release assets so the pin cannot disappear.

FFmpeg's LGPL checklist for distribution (ffmpeg.org/legal.html): no `--enable-gpl` or `--enable-nonfree` (true for this build), DLLs shipped unmodified and with their names, the matching source and configure line offered from the same place as the download, FFmpeg credited in the About box and the EULA, no reverse-engineering ban in the EULA.

## Measurements (Linux container, 4 cores, no GPU)

Run with `cargo run -p bardo-media --release --example measure`. Synthetic project: twelve 5 s 1080p30 clips (H.264, 12 Mbit/s, OpenH264), a 60 s voice and a 60 s music track.

**Proxies** (540 lines, all twelve clips one after another):

| Codec | Time | Per clip | Size |
| --- | --- | --- | --- |
| MJPEG (intra) | 7.1 s | 587 ms | 29.8 MB |
| H.264 OpenH264, keyframe every 0.5 s | 7.3 s | 605 ms | 7.6 MB |

**Preview start** (spawn to first decoded frame of the 60 s timeline, median of five):

| Source | Size | From 0 s | From 31.7 s |
| --- | --- | --- | --- |
| MJPEG proxies | 960x540 | 127 ms | 109 ms |
| H.264 proxies | 960x540 | 285 ms | 150 ms |
| Originals | 1920x1080 | 370 ms | 274 ms |

**Preview decode rate** (decode, scale, BGRA through the pipe; no display):

| Source | Size | Frames/s | MB/s through the pipe |
| --- | --- | --- | --- |
| MJPEG proxies | 960x540 | 608 | 1261 |
| H.264 proxies | 960x540 | 255 | 529 |
| Originals | 1920x1080 | 109 | 906 |

**Final render** (60 s, 1080x1920 at 30 fps, 12 Mbit/s, two audio tracks, -14 LUFS target): OpenH264 in 24.4 s (2.5x realtime), output measured at -14.0 LUFS. Encoder detection took 123 ms for five candidates.

Reading: preview start is an order of magnitude under the one-second target even on four cores, and decoding is far from the bottleneck at preview size. MJPEG proxies start faster and decode 2.4x faster than H.264 ones at the cost of 4x the disk; that is the right trade for scrubbing. Previewing the originals would also fit the target, so proxies are about smooth playback and scrubbing on slower machines, not about meeting the start target here.

## Frame to screen in GPUI

What `gpui-pre` 0.3.7 offers (read from the crate source and its docs):

- `RenderImage::new(frames)` holds BGRA pixels (in an RGBA-typed `image` buffer, no swizzle needed); `img(ImageSource::Render(Arc<RenderImage>))` draws it. Each new `RenderImage` gets a new id, so every video frame is a new sprite-atlas entry: on Windows a `UpdateSubresource` upload, and a new atlas texture when the frame does not fit an existing one (frames over 1024 pixels get a texture of their own; textures with no live entries are released). `App::drop_image` frees the previous frame.
- `window.request_animation_frame()` redraws on the next vsync, the hook for a playing video.
- `surface(…)` / `paint_surface` draw an external GPU buffer without a copy, but only take a macOS `CVPixelBuffer`; on Windows `draw_surfaces` is a no-op. Zero copy on Windows therefore needs a `gpui-pre-windows` patch (open an NT-shared D3D11 texture, draw it as a surface) and in-process decoding to produce that texture.

`cargo run -p bardo-ui --release --example preview_probe -- <video> [WxH] [seconds] [static]` plays a file through that path and reports presented and dropped frames, window frame times and the app's CPU. Under Xvfb with Mesa's software Vulkan (lavapipe) every window frame takes about 48 ms whatever is drawn, so the window caps near 21 fps; the comparison with the same window showing a still frame isolates what the per-frame upload adds:

| Size | Presented | Window frame p50 / p95 | App CPU | Same window, still frame |
| --- | --- | --- | --- | --- |
| 960x540 | 21.1 fps (window-bound) | 48.0 / 48.1 ms | 152% of a core | 48.0 / 48.1 ms, 136% |
| 1920x1080 | 19.9 fps (window-bound) | 48.0 / 64.1 ms | 193% of a core | 48.0 / 48.2 ms, 143% |

At preview size the copy path adds a sixth of a core and no frame time, even with a software rasterizer doing the upload; at 1080p it adds half a core and some p95 jitter. A real GPU uploads 2 MB (540p) or 8 MB (1080p) per frame trivially; the cost left is the CPU copy and atlas bookkeeping.

**Decision for #20:** preview through the stock image path at the preview panel's size from MJPEG proxies, one `RenderImage` per frame, previous one dropped. Keep frame production behind `app` so a zero-copy path can replace it later without touching the editor. Build the D3D11 path only if the Windows probe misses 30 fps at preview size or the CPU cost is visible.

## Changes to the editing slices

- **#20 (rough cut):** can start on this decision. The timeline model in `domain` maps onto `media::ffmpeg::RenderPlan`; preview is `Ffmpeg::preview` over proxies, restarted on seek (about 0.1 s); proxies build as a job per clip (`build_proxy`) when clips enter the timeline; waveforms from `Ffmpeg::waveform`. Audio playback during preview is not covered by this spike: decode the same plan's audio mix to PCM on a second pipe and play it with the existing `rodio` output, using the audio clock to pace video frames.
  As built in #20: one proxies job per project rather than per clip (it skips proxies that exist, keeps going past a failed file and lists the failures in its checkpoint); stills get a 540-line JPEG proxy and the narration a peaks file; `RenderPlan` clips are a video, a still (`-loop 1`) or black, and a clip shorter than its scene holds its last frame; `Ffmpeg::preview_audio` streams the mix as f32 PCM on a second pipe, and the preview waits for its first frame and buffered audio, then paces video by the samples the device has played (wall clock without a sound device).
- **#27 (final render):** done. Resume works per output file: each finished file is checkpointed and a resumed render makes only the ones left; segment rendering (fixed-length segments joined with the concat demuxer) stays an option if single files get long enough for that to matter. The second loudness pass became one gain plus a true-peak limiter (ADR-0007). The child in a job object that dies with Bardo is still open (ADR-0007).
- **#32 (imported narration):** other formats (M4A/AAC, FLAC, OGG/Opus) can now be converted to WAV on import with one `ffmpeg` call.
- **Long timelines:** past a few hundred clips the command line nears Windows' 32,767-character limit; move the graph to a file (`-/filter_complex`) and open each source once.

## Windows-only checks (for #51)

- [ ] `cargo xtask fetch-ffmpeg` on Windows, then `cargo test -p bardo-media`: all pass, including `renders_with_every_working_encoder` with NVENC listed.
- [ ] `cargo run -p bardo-media --release --example measure` on the reference machine; record proxy time, preview start and render time with `h264_nvenc` and the software encoder in `docs/spec/mvp.md`.
- [ ] `cargo run -p bardo-ui --release --example preview_probe -- <proxy.mkv> 960x540 10` and `… 1920x1080 10`, each also with `static`: presented fps at 30 with no drops at preview size, and the CPU cost of the copy (compare with `static`). If it misses, start the D3D11 shared-texture path (issue #5's decision order).
- [ ] Process start on Windows: `detect_encoders` time and preview start in the `measure` output; antivirus scanning of `ffmpeg.exe` can add to the first start.
- [ ] No console window flashes up when the app starts ffmpeg (release build, `windows_subsystem = "windows"`).
- [ ] Media Foundation (`h264_mf`) works or is skipped cleanly; NVENC with the installed driver (8.1 needs 570+).
