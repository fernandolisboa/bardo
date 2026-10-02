//! Media files: MP3 frames (duration, joining parts), the format and
//! length of audio the user brings, and audio playback, without ffmpeg;
//! probing, frames, waveforms, proxies, preview and render through the
//! bundled ffmpeg (ADR-0007).

pub mod audio;
pub mod ffmpeg;
pub mod mp3;
mod playback;

pub use audio::{AudioFormat, AudioInfo, ProbeError};
pub use playback::{AudioOutput, DeviceAudio, Playback, PlaybackError};
