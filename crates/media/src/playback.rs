//! Audio playback on the default output device (rodio). Playback opens
//! paused at the start; the caller plays, pauses, seeks and reads the
//! position, which is what drives word highlighting. Files are MP3 unless
//! their name says WAV.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

use crate::AudioFormat;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaybackError {
    /// No output device, or the system refused to open it.
    #[error("no audio output: {0}")]
    NoOutput(String),
    /// The file is missing or not audio this player decodes.
    #[error("cannot play the file: {0}")]
    Unreadable(String),
}

/// One file loaded for playback. Not `Send`: it lives with the view that
/// plays it.
pub trait Playback {
    fn play(&mut self) -> Result<(), PlaybackError>;
    fn pause(&mut self);
    /// Playing right now: not paused and not at the end.
    fn is_playing(&self) -> bool;
    /// Moves to `position`; past the end means the end.
    fn seek(&mut self, position: Duration) -> Result<(), PlaybackError>;
    /// Where playback is in the file.
    fn position(&self) -> Duration;
}

/// Opens files for playback. Shared by the app; each `open` is
/// independent.
pub trait AudioOutput: Send + Sync {
    /// Loads `path` paused at the start.
    fn open(&self, path: &Path) -> Result<Box<dyn Playback>, PlaybackError>;
}

/// Plays through the system's default output device.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeviceAudio;

impl AudioOutput for DeviceAudio {
    fn open(&self, path: &Path) -> Result<Box<dyn Playback>, PlaybackError> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|error| PlaybackError::NoOutput(error.to_string()))?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        player.pause();
        let mut playback = DevicePlayback {
            _sink: sink,
            player,
            path: path.to_owned(),
        };
        playback.load()?;
        Ok(Box::new(playback))
    }
}

struct DevicePlayback {
    /// Dropping it closes the device; kept for as long as the player.
    _sink: MixerDeviceSink,
    player: Player,
    path: PathBuf,
}

impl DevicePlayback {
    fn load(&mut self) -> Result<(), PlaybackError> {
        let file =
            File::open(&self.path).map_err(|error| PlaybackError::Unreadable(error.to_string()))?;
        let reader = BufReader::new(file);
        let name = self.path.file_name().and_then(|name| name.to_str());
        let decoder = match name.and_then(AudioFormat::from_file_name) {
            Some(AudioFormat::Wav) => Decoder::new_wav(reader),
            Some(AudioFormat::Mp3) | None => Decoder::new_mp3(reader),
        }
        .map_err(|error| PlaybackError::Unreadable(error.to_string()))?;
        self.player.append(decoder);
        Ok(())
    }

    /// Loads the file again once it played to the end, so play and seek
    /// work after the end as they did before it.
    fn reload_if_ended(&mut self) -> Result<(), PlaybackError> {
        if self.player.empty() {
            self.player.pause();
            self.load()?;
        }
        Ok(())
    }
}

impl Playback for DevicePlayback {
    fn play(&mut self) -> Result<(), PlaybackError> {
        self.reload_if_ended()?;
        self.player.play();
        Ok(())
    }

    fn pause(&mut self) {
        self.player.pause();
    }

    fn is_playing(&self) -> bool {
        !self.player.is_paused() && !self.player.empty()
    }

    fn seek(&mut self, position: Duration) -> Result<(), PlaybackError> {
        self.reload_if_ended()?;
        self.player
            .try_seek(position)
            .map_err(|error| PlaybackError::Unreadable(error.to_string()))
    }

    fn position(&self) -> Duration {
        self.player.get_pos()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use rodio::Source as _;

    use super::*;
    use crate::mp3;

    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone-1s.mp3");

    /// Decodes without an output device and measures what a player hears.
    fn decoded_duration(bytes: Vec<u8>) -> Duration {
        let decoder = Decoder::new_mp3(Cursor::new(bytes)).unwrap();
        let rate = decoder.sample_rate().get() as u64;
        let channels = decoder.channels().get() as u64;
        let samples = decoder.count() as u64;
        Duration::from_nanos(samples * 1_000_000_000 / (rate * channels))
    }

    #[test]
    fn wav_decodes_to_the_length_its_header_says() {
        let wav = crate::audio::tests::silent_wav(2);
        let decoder = Decoder::new_wav(Cursor::new(wav)).unwrap();
        let rate = decoder.sample_rate().get() as u64;
        let channels = decoder.channels().get() as u64;
        let samples = decoder.count() as u64;
        assert_eq!(
            Duration::from_nanos(samples * 1_000_000_000 / (rate * channels)),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn a_joined_stream_decodes_to_the_length_its_frames_say() {
        let joined = mp3::join(&[TONE, TONE, TONE]).unwrap();
        let heard = decoded_duration(joined.bytes);
        let diff = heard.abs_diff(joined.duration);
        // A decoder may trim the first frame's delay; never more.
        assert!(
            diff <= Duration::from_millis(60),
            "{heard:?} vs {:?}",
            joined.duration
        );
    }
}
