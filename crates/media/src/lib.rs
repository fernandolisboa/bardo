//! Media files: MP3 frames (duration, joining parts) and audio playback.
//! ffmpeg-based probing, proxies, preview and render join it with their
//! slices (ADR-0001).

pub mod mp3;
mod playback;

pub use playback::{AudioOutput, DeviceAudio, Playback, PlaybackError};
