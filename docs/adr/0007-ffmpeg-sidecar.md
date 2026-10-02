# ADR-0007: ffmpeg as bundled sidecar executables

- Status: Accepted
- Date: 2026-10-02
- Spike: issue #18, findings in `docs/spikes/ffmpeg.md`

## Context

Editing (#20 onward), render (#27) and export (#28) need probing, decoding for preview, waveforms, proxies, trimming, framing to 9:16 and 16:9, audio mixing, loudness normalization and H.264 encoding with the GPU when there is one. The spec requires ffmpeg bundled and pinned (`docs/spec/mvp.md`). Bardo is a single-user Windows desktop app; the workspace forbids `unsafe` in its own crates; CI builds and tests on `windows-latest`.

Two ways to use ffmpeg from Rust:

| | Sidecar executables (`ffmpeg.exe`, `ffprobe.exe` as child processes) | Bindings (`ffmpeg-next` / `ffmpeg-sys-next`, linking the libraries) |
| --- | --- | --- |
| Build on Windows CI | None: download a pinned archive | FFmpeg headers and import libraries, LLVM/clang for bindgen, matching versions; bindings lag new FFmpeg majors |
| Failure isolation | A crash or hang in a decoder stays in the child | Takes the app down |
| Cancel | Kill the child | Cooperative checks in our decode loop |
| Progress | `-progress pipe:1` (output time) | Our own loop |
| Features | Everything the CLI has: filter graphs, `loudnorm`, encoders, muxers, fast seeking | The same, but every graph, muxer and seek written by hand |
| Error surface | Exit status plus the end of stderr | Typed error codes |
| Per-call cost | A process start: tens of milliseconds on Linux (Windows to confirm) | None |
| Frames into the app | Raw frames on a pipe (CPU copy) | Can stay on the GPU (D3D11) |

## Decision

1. **Sidecar executables.** `bardo-media` runs the bundled `ffmpeg` and `ffprobe` as child processes (`media::ffmpeg`), never links them. Every run is blocking, takes a `Monitor` (progress 0.0 to 1.0, cancel by kill) and runs on a job thread. Children start with `CREATE_NO_WINDOW` so no console flashes up from the GUI app. Outputs are written as `name.partial.ext` and renamed when done.
2. **The build: FFmpeg 8.1.3, LGPL (v3), shared libraries, win64, from BtbN/FFmpeg-Builds**, pinned by URL and SHA-256 in `crates/media/ffmpeg.toml`. LGPL keeps Bardo's own license free; the shared build lets `ffmpeg.exe` and `ffprobe.exe` share one set of DLLs (about 155 MB unpacked, against about 270 MB for two static executables). 8.1 rather than 9.0 because 9.0's NVENC needs NVIDIA driver 610 or newer and 8.1's needs 570, so more machines get hardware encoding. The pin is the month-end autobuild, which BtbN keeps for two years; it is mirrored to Bardo's own release assets before the first installer ships.
3. **Where it is found:** the folder in `BARDO_FFMPEG_DIR`, then `ffmpeg\` next to `bardo.exe` (the installed layout), then PATH; the first with both executables at 7.1 or newer wins. `.cargo/config.toml` points `BARDO_FFMPEG_DIR` at `.ffmpeg/bin`, which `cargo xtask fetch-ffmpeg` fills, so tests and `cargo run` use the pinned build; CI does the same.
4. **Encoders:** H.264, tried in order NVENC, AMF, Quick Sync, Media Foundation, OpenH264; each is checked with a short trial encode (the build lists hardware encoders whether or not the GPU is there) and the first that works is used. OpenH264 is the software fallback present everywhere; Media Foundation is Windows' own encoder. The LGPL build has no x264.
5. **One graph for preview and render.** A `RenderPlan` (video clips back to back with a crop window or fit, audio tracks of placed clips with gains) becomes one ffmpeg filter graph. Preview writes it as raw BGRA frames on a pipe from the playhead on, from proxies; the render encodes it to MP4 from the originals. Loudness is two-pass: an audio-only `loudnorm` pass measures the mix, the render applies one gain and a 4× oversampled true-peak limiter. (Amended in #27: `loudnorm`'s own linear second pass falls back to dynamic compression when a mix's range is wide or the gain lifts its peaks, and its peaks still overshot the ceiling after AAC.)
6. **Proxies:** 540 lines, MJPEG, every frame a keyframe, in Matroska. Seeking lands on the exact frame with no decoding ahead, which is what scrubbing needs; the files are about four times the size of H.264 proxies, which is fine for clips of a few seconds.
7. **Frames to the screen: CPU copy first.** Preview frames reach GPUI as `RenderImage`s (BGRA), sized to the preview panel, one atlas upload per frame, the previous one dropped with `drop_image`. A D3D11 shared texture (zero copy) needs in-process decoding and a patch to `gpui-pre-windows` (`draw_surfaces` is a no-op there); it is built only if the Windows measurement shows the copy path missing 30 fps at preview size, following the order in issue #5 (upstream first, `[patch.crates-io]` last).

## Consequences

- No C toolchain or FFmpeg SDK in CI; a new FFmpeg is a manifest change plus `cargo xtask fetch-ffmpeg`.
- Distribution follows FFmpeg's LGPL checklist: ship the DLLs unmodified and unrenamed with `LICENSE.txt`, offer the matching source and configure line, credit FFmpeg in the About box. The installer slice carries this.
- Each call pays a process start; calls are coarse (a whole proxy, a whole render, one preview stream), so it does not add up. Preview opens one child per play or seek, not per frame.
- A crash of Bardo can leave a running child until it finishes. Putting children in a Windows job object that dies with Bardo needs the Win32 API: unsafe calls, which the workspace forbids, or a wrapper crate. The render slice (#27) left it open; a render interrupted that way resumes from its last finished file when Bardo starts again.
- Very long timelines make long command lines (one input per clip). Windows limits a command line to 32,767 characters; past a few hundred clips the graph goes into a file (`-/filter_complex`) and sources are deduplicated. Not needed for 60 s shorts.
- Zero-copy preview stays possible later: the shared DLLs Bardo ships are the same libraries bindings would load.
