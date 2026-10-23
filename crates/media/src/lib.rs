//! Media files: MP3 frames (duration, joining parts), the format and
//! length of audio the user brings, and audio playback, without ffmpeg;
//! probing, frames, waveforms, proxies, preview and render through the
//! bundled ffmpeg (ADR-0007); the control videos' frames drawn on the
//! processor (`control`).

pub mod audio;
pub mod control;
mod engine;
pub mod ffmpeg;
pub mod mp3;
mod pcm;
mod playback;

pub use audio::{AudioFormat, AudioInfo, ProbeError};
pub use engine::{BundledFfmpeg, MediaEngine};
pub use pcm::{Chunk, PcmStream};
pub use playback::{
    AudioOutput, DeviceAudio, PcmClock, PcmSource, Playback, PlaybackError, StreamPlayback,
};
