//! A recording's sound on its own: what a capture (#189) sends to be
//! transcribed and voiced, so the video itself never leaves the computer.

use std::path::Path;

use super::process::{self, Span};
use super::{Ffmpeg, MediaError, Monitor, partial_path, path_arg, remove_partial};

/// The sample rate of extracted sound: CD quality, which every speech
/// provider reads.
pub const SOUND_RATE: u32 = 44_100;

impl Ffmpeg {
    /// Writes the first audio stream of `source` to `destination` as WAV:
    /// 16-bit PCM, mono, [`SOUND_RATE`].
    pub fn extract_sound(
        &self,
        source: &Path,
        destination: &Path,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError> {
        let info = self.probe(source)?;
        if info.audio.is_none() {
            return Err(MediaError::Parse("the file has no audio stream".into()));
        }
        let partial = partial_path(destination);
        let mut command = self.ffmpeg();
        command
            .arg("-y")
            .arg("-i")
            .arg(path_arg(source))
            .args(["-map", "0:a:0", "-vn", "-sn", "-dn"])
            .args(["-ac", "1", "-ar", &SOUND_RATE.to_string()])
            .args(["-c:a", "pcm_s16le", "-f", "wav"])
            .arg(path_arg(&partial));
        match process::run(command, monitor, Span::whole(info.duration)) {
            Ok(_) => {
                std::fs::rename(&partial, destination)?;
                Ok(())
            }
            Err(error) => {
                remove_partial(&partial);
                Err(error)
            }
        }
    }
}
