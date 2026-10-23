# ADR-0024: Local vision models through ONNX Runtime

- Status: Accepted
- Date: 2026-10-23
- Spike: issue #187, findings in `docs/spikes/vision.md`
- Builds on: the performance capture spec (`docs/spec/performance-capture.md`)

## Context

Performance capture (#186) analyses each take on the computer: a face mesh for Mesh and Depth modes, and a depth map and person mask for Depth mode. The spec chose local analysis (free, offline, the performer's face stays on the computer) and Apache-2.0 models, and left the runtime, the exact models and how they ship to a spike. Bardo forbids `unsafe` in its own crates, runs on Windows machines with any graphics card or none, and pins every binary it ships (ffmpeg in ADR-0007).

## Decision

1. **ONNX Runtime through `ort`, pinned exactly** (`=2.0.0-rc.12`, ONNX Runtime 1.24.2), linked statically from pyke's builds. Only the new `vision` crate depends on it. Its API needs no `unsafe` in Bardo's code.
2. **DirectML when it starts, the CPU otherwise.** Each model's session asks for DirectML on Windows and fails loudly if it cannot start; Bardo then builds that session on the CPU and records why, so analysis never depends on a graphics driver. The small face models run on one CPU thread (faster than several); the depth models use all cores.
3. **The models**, all Apache-2.0:
   - **Face**: MediaPipe Face Landmarker (float16 v1): BlazeFace short range, the 478-point mesh, the 52 blendshapes, run as MediaPipe runs them (detector, rotated crop, tracking from the landmarks, one-euro smoothing). When the whole frame shows no face, the detector searches again around the person mask's face; a face as small as in a standing shot (13 % of the frame's height in the spike) is otherwise missed. So Mesh mode runs the person mask too, but only on the frames where it searches and on a sample of frames (about one a second) where it counts faces in each person region.
   - **Person mask**: MediaPipe selfie multiclass 256×256. No SAM-class segmenter unless reviews show masks failing.
   - **Depth**: **Video Depth Anything Small** at 280×504 (504×280 for vertical takes): five times less flicker than Depth Anything V2 Small per frame, for 1.3× the time. It needs about 4.5 GB of memory on the CPU; where that is not free, Depth Anything V2 Small smoothed over time is used instead.
4. **Converted once, mirrored, bundled.** A script converts the pinned sources (each by SHA-256, upstream code by commit) to ONNX and checks every file against TFLite or PyTorch. The ONNX files, about 455 MB with the vertical depth files, are kept as assets of a `vision-models-<n>` release of Bardo's repository and pinned by SHA-256 in `crates/vision/models.toml`; an xtask fetches them and the installer bundles them. No Python, TensorFlow or PyTorch at build or run time.
5. **Fixed input sizes.** Each depth model file has one input size; vertical takes have their own files.

## Consequences

- Mesh mode costs 12 to 20 s per 30 s take on a 4-core CPU and Depth mode 4.5 to 11 min: analysis is a job with progress, cancel and resume, as the spec planned. The reference machine's GPU numbers are checked on #51.
- The installer grows by about 455 MB for a module not every user opens (each orientation's two depth files are 215 MB); fp16 depth weights would bring it to about 240 MB and are worth trying in #190. #190 ships float16 weights for every model: the bundle is about 228 MB, mirrored as `vision-models-1`. ONNX Runtime is linked into `bardo.exe`; only `DirectML.dll` ships beside it. Bundling keeps Bardo working offline.
- A new model or version is a new conversion, a new mirror release and a manifest change. tf2onnx's output is not byte-for-byte reproducible, so the mirrored files, not a re-conversion, are the pinned artifact.
- MediaPipe's mesh leaves the eyes and mouth open and stops at the brows: the Mesh control video (#191) must cover those too before it can promise that no frame shows the performer's face.
- Depth Anything V2 Base/Large and Video Depth Anything Base/Large (CC-BY-NC) stay out, and so does Video Depth Anything at its native 518 lines (it does not fit in memory on a CPU).
