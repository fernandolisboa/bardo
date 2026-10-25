# ADR-0026: Local takes with LivePortrait

- Status: Accepted (the default take model waits for the reference machine's checks)
- Date: 2026-10-25
- Issue: #242
- Builds on: ADR-0024 (local vision models), the performance capture spec (`docs/spec/performance-capture.md`), the cost research on #241
- Findings: [`docs/spikes/liveportrait.md`](../spikes/liveportrait.md)

## Context

Performed takes in the cloud cost about US$10 per finished minute before retries (#241). LivePortrait (MIT code and weights) animates a still portrait from a driving video of a face. The spike converted its five networks to ONNX, ran them through `ort` on the CPU with DirectML falling back as it should, cropped with MediaPipe in place of InsightFace, and compared every step with upstream's own code. It matched upstream: crops within about 2 % of the crop's size, the same self-reenactment error (22.1 dB on the face), the same pictures to the eye. On a CPU it is far too slow (6 to 9 s a frame, one to two hours for a 30 s take), so the speed on the reference machine's graphics card decides whether it becomes the default.

## Decision

1. **A local take model.** LivePortrait joins the take model list under a new local provider: no key, no cost, no draft. `Provider` lists keyed, metered services today, so the slice adds this one with no key entry and no meter. It runs as a local job, as take analysis (#190) does, in chunks of frames with progress, cancel and resume from a checkpoint (the frames written so far, the reference frames' motion, the look's features), not through the network-shaped `TakeGenerator`. It writes the performed clip itself, at the look's framing and size (`keeps_framing: false`); for the spec's 1080p finals the look must be at least 1080p.
2. **Recording mode only.** The motion extractor reads a real face, so it is driven by the recording; Mesh and Depth figures are no face to it, and the privacy reason for Mesh mode does not apply when nothing leaves the computer. Only the head moves: head-and-shoulders shots.
3. **In `crates/vision`, on ADR-0024's runtime.** The networks run where Bardo's other ONNX models run, so `crates/vision` stays the only crate on `ort` (ADR-0024's pin), with DirectML when it starts and the CPU otherwise, recording why. Every network is a separate ONNX file so each can fall back or be timed alone.
4. **The 5-D grid_sample as Gather.** The warping module's two trilinear `grid_sample` calls are rewritten at conversion into Gather and arithmetic, with int64 voxel indices, instead of ONNX GridSample, which DirectML registers only for 4-D input. The rewrite matches PyTorch as closely as the GridSample export and is as fast on the CPU. The GridSample export stays in the conversion script so the reference machine can compare them.
5. **The crop and the reference frames from MediaPipe.** The source and driving crops are upstream's, computed from the MediaPipe face mesh that `crates/vision` already ships, so no InsightFace model (non-commercial) and no 203-point model. The take's face analysis gives the driving crop, the head's pose reference (the first frame with a face) and the expression reference (a frame with the mouth closed and a neutral face), since an expression measured from a frame mid-word mutes the mouth for the whole take. The lip ratio that decides whether to close the look's mouth is mapped onto upstream's scale in the slice, since MediaPipe's points read it lower.
6. **Converted once, mirrored, downloaded on first use.** A script converts the pinned sources (upstream code by commit, weights by revision and SHA-256) and checks each file against PyTorch; the exports are byte-reproducible. Float16 weights (read back as float32) make 261 MB with one warping module. They are kept as assets of a `liveportrait-models-<n>` release of Bardo's repository, pinned by SHA-256, and downloaded the first time the user picks the local model, then kept with Bardo's other data and used offline. Unlike ADR-0024's models they stay out of the installer, which they would nearly double for a model not every user runs.
7. **Float16 compute only if it pays.** The warping module and generator also exist as copies that compute in float16. If the reference machine shows them faster on DirectML at the same quality, they replace the matching files in the download (about the same size); otherwise only the float32-compute files ship.
8. **The default take model** stays a cloud model until the checks on #236 show the local one on the reference machine: its speed on DirectML (a 30 s take within 5 minutes), and its quality on the user's own captures next to Kling's.

## Consequences

- A performed take costs nothing but time once the models are downloaded, and the recording never leaves the computer.
- Only the face moves; body motion and full-frame generation stay with the cloud models.
- The face is generated at 512 × 512 and pasted into the look; a look whose face is large in the frame comes out softer, so the look should be framed with room around the face, or a face restorer comes later.
- The capture guide asks for a second of neutral face with the mouth closed before speaking.
- Without a working graphics card the local model is too slow to offer for anything but short takes; the app says so from the measured speed rather than hiding it.
- A new LivePortrait version is a new conversion, a new mirror release and a manifest change.
