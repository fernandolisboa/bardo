//! Audio playback on the default output device (rodio). Playback opens
//! paused at the start; the caller plays, pauses, seeks and reads the
//! position, which is what drives word highlighting. Files are MP3 unless
//! their name says WAV. A [`PcmStream`] plays as it arrives, and its
//! position (what has been played of it) is the editor preview's clock.

use std::fs::File;
use std::io::BufReader;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

use crate::{AudioFormat, Chunk, PcmStream};

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

/// A stream being played. Not `Send`, like [`Playback`]; dropping it stops
/// the sound.
pub trait StreamPlayback {
    fn play(&mut self);
    fn pause(&mut self);
    /// Whether the stream's first samples have arrived.
    fn is_buffered(&self) -> bool;
    /// How much of the stream has been played. Waiting for samples that
    /// have not arrived yet plays silence and does not count.
    fn position(&self) -> Duration;
    /// Whether the stream ended and everything in it was played.
    fn has_ended(&self) -> bool;
}

/// Opens files and streams for playback. Shared by the app; each `open`
/// is independent.
pub trait AudioOutput: Send + Sync {
    /// Loads `path` paused at the start.
    fn open(&self, path: &Path) -> Result<Box<dyn Playback>, PlaybackError>;

    /// Plays `stream` as it arrives, paused until `play`.
    fn stream(&self, stream: PcmStream) -> Result<Box<dyn StreamPlayback>, PlaybackError>;
}

/// Plays through the system's default output device.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeviceAudio;

impl AudioOutput for DeviceAudio {
    fn open(&self, path: &Path) -> Result<Box<dyn Playback>, PlaybackError> {
        let (sink, player) = DeviceAudio::open_player()?;
        let mut playback = DevicePlayback {
            _sink: sink,
            player,
            path: path.to_owned(),
        };
        playback.load()?;
        Ok(Box::new(playback))
    }

    fn stream(&self, stream: PcmStream) -> Result<Box<dyn StreamPlayback>, PlaybackError> {
        let (sink, player) = DeviceAudio::open_player()?;
        let source = PcmSource::new(stream);
        let clock = source.clock();
        player.append(source);
        Ok(Box::new(DeviceStream {
            _sink: sink,
            player,
            clock,
        }))
    }
}

/// What has been played of a stream, shared between the audio thread and
/// whoever reads the position.
#[derive(Debug)]
pub struct PcmClock {
    played: AtomicU64,
    ended: AtomicBool,
    buffered: Arc<AtomicBool>,
    channels: u16,
    sample_rate: u32,
}

impl PcmClock {
    pub fn position(&self) -> Duration {
        let samples = self.played.load(Ordering::Acquire);
        let per_second = u128::from(self.channels.max(1)) * u128::from(self.sample_rate.max(1));
        Duration::from_nanos((u128::from(samples) * 1_000_000_000 / per_second) as u64)
    }

    pub fn has_ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }

    pub fn is_buffered(&self) -> bool {
        self.buffered.load(Ordering::Acquire)
    }
}

/// A [`PcmStream`] as a rodio source: plays chunks as they arrive, and
/// silence while the next one is late (without counting it as played).
pub struct PcmSource {
    stream: PcmStream,
    chunk: Vec<f32>,
    next: usize,
    /// Silent samples left to finish a frame of silence.
    silence: u16,
    clock: Arc<PcmClock>,
}

impl PcmSource {
    pub fn new(stream: PcmStream) -> PcmSource {
        let clock = Arc::new(PcmClock {
            played: AtomicU64::new(0),
            ended: AtomicBool::new(false),
            buffered: stream.buffered_flag(),
            channels: stream.channels,
            sample_rate: stream.sample_rate,
        });
        PcmSource {
            stream,
            chunk: Vec::new(),
            next: 0,
            silence: 0,
            clock,
        }
    }

    pub fn clock(&self) -> Arc<PcmClock> {
        Arc::clone(&self.clock)
    }
}

impl Iterator for PcmSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.silence > 0 {
            self.silence -= 1;
            return Some(0.0);
        }
        if self.next >= self.chunk.len() {
            match self.stream.try_next_chunk() {
                Chunk::Ready(chunk) => {
                    self.chunk = chunk;
                    self.next = 0;
                }
                // Late: a frame of silence, so the device keeps going and
                // the channels stay in place (chunks hold whole frames).
                Chunk::Late => {
                    self.silence = self.stream.channels.max(1) - 1;
                    return Some(0.0);
                }
                Chunk::Ended => {
                    self.clock.ended.store(true, Ordering::Release);
                    return None;
                }
            }
            if self.chunk.is_empty() {
                self.silence = self.stream.channels.max(1) - 1;
                return Some(0.0);
            }
        }
        let sample = self.chunk[self.next];
        self.next += 1;
        self.clock.played.fetch_add(1, Ordering::AcqRel);
        Some(sample)
    }
}

impl Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.stream.channels.max(1)).expect("at least one channel")
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.stream.sample_rate.max(1)).expect("a positive rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct DeviceStream {
    /// Dropping it closes the device; kept for as long as the player.
    _sink: MixerDeviceSink,
    player: Player,
    clock: Arc<PcmClock>,
}

impl StreamPlayback for DeviceStream {
    fn play(&mut self) {
        self.player.play();
    }

    fn pause(&mut self) {
        self.player.pause();
    }

    fn is_buffered(&self) -> bool {
        self.clock.is_buffered()
    }

    fn position(&self) -> Duration {
        self.clock.position()
    }

    fn has_ended(&self) -> bool {
        self.clock.has_ended()
    }
}

impl DeviceAudio {
    fn open_player() -> Result<(MixerDeviceSink, Player), PlaybackError> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|error| PlaybackError::NoOutput(error.to_string()))?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        player.pause();
        Ok((sink, player))
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
    fn a_stream_source_plays_its_samples_and_counts_them() {
        let stream = PcmStream::from_samples(2, 4, vec![0.1, 0.2, 0.3, 0.4]);
        let mut source = PcmSource::new(stream);
        let clock = source.clock();
        assert!(clock.is_buffered());
        let played: Vec<f32> = source.by_ref().collect();
        assert_eq!(played, vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(clock.position(), Duration::from_millis(500));
        assert!(clock.has_ended());
    }

    #[test]
    fn a_late_stream_plays_silence_without_moving_the_clock() {
        let (stream, sender) = PcmStream::channel(1, 10, 2, None);
        let mut source = PcmSource::new(stream);
        let clock = source.clock();
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(clock.position(), Duration::ZERO);
        sender.send(vec![0.5]);
        assert_eq!(source.next(), Some(0.5));
        assert_eq!(clock.position(), Duration::from_millis(100));
        drop(sender);
        assert_eq!(source.next(), None);
        assert!(clock.has_ended());
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
