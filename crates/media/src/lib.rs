//! Media files: MP3 frames (duration, joining parts), the format and
//! length of audio the user brings, and audio playback. ffmpeg-based
//! probing, proxies, preview and render join it with their slices
//! (ADR-0001).

pub mod audio;
pub mod mp3;
mod playback;

pub use audio::{AudioFormat, AudioInfo, ProbeError};
pub use playback::{AudioOutput, DeviceAudio, Playback, PlaybackError};
