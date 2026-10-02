//! Decoded audio arriving as it is decoded: interleaved 32-bit float
//! samples in chunks, a few seconds ahead of the listener at most. The
//! preview's audio comes this way from ffmpeg, and the player
//! ([`crate::AudioOutput::stream`]) counts what it has played, which is the
//! clock the preview's pictures follow.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// What a stream has for its player right now.
#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    /// Interleaved samples, whole frames.
    Ready(Vec<f32>),
    /// Nothing yet: the producer is behind.
    Late,
    /// The stream is over.
    Ended,
}

/// A stream of interleaved samples. Dropping it stops whatever produces
/// them (ffmpeg is killed).
pub struct PcmStream {
    pub channels: u16,
    pub sample_rate: u32,
    chunks: mpsc::Receiver<Vec<f32>>,
    buffered: Arc<AtomicBool>,
    /// Kept for as long as the stream: the producer (an ffmpeg child that
    /// dies with it).
    _producer: Option<Box<dyn Send>>,
}

impl std::fmt::Debug for PcmStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcmStream")
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

/// The producing side of a [`PcmStream`].
pub(crate) struct PcmSender {
    sender: mpsc::SyncSender<Vec<f32>>,
    buffered: Arc<AtomicBool>,
}

impl PcmSender {
    /// Hands over a chunk, waiting while the listener is far enough behind.
    /// False once the stream is dropped.
    pub(crate) fn send(&self, chunk: Vec<f32>) -> bool {
        let sent = self.sender.send(chunk).is_ok();
        self.buffered.store(true, Ordering::Release);
        sent
    }
}

impl Drop for PcmSender {
    /// A producer that stops early (a failed decode) still releases the
    /// player waiting for sound; the stream then reads as ended.
    fn drop(&mut self) {
        self.buffered.store(true, Ordering::Release);
    }
}

impl PcmStream {
    /// A stream fed by a producer through the returned sender, holding up
    /// to `ahead` chunks. `producer` lives as long as the stream.
    pub(crate) fn channel(
        channels: u16,
        sample_rate: u32,
        ahead: usize,
        producer: Option<Box<dyn Send>>,
    ) -> (PcmStream, PcmSender) {
        let (sender, chunks) = mpsc::sync_channel(ahead);
        let buffered = Arc::new(AtomicBool::new(false));
        (
            PcmStream {
                channels,
                sample_rate,
                chunks,
                buffered: Arc::clone(&buffered),
                _producer: producer,
            },
            PcmSender { sender, buffered },
        )
    }

    /// All of `samples` at once (tests, short sounds).
    pub fn from_samples(channels: u16, sample_rate: u32, samples: Vec<f32>) -> PcmStream {
        let (stream, sender) = PcmStream::channel(channels, sample_rate, 1, None);
        if !samples.is_empty() {
            sender.send(samples);
        }
        // Dropping the sender ends the stream after these samples.
        drop(sender);
        stream.buffered.store(true, Ordering::Release);
        stream
    }

    /// Whether the first samples have arrived (or the stream ended
    /// without any), so playing can start without a gap.
    pub fn is_buffered(&self) -> bool {
        self.buffered.load(Ordering::Acquire)
    }

    pub(crate) fn buffered_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.buffered)
    }

    /// The next chunk if one has arrived.
    pub fn try_next_chunk(&self) -> Chunk {
        match self.chunks.try_recv() {
            Ok(chunk) => Chunk::Ready(chunk),
            Err(mpsc::TryRecvError::Empty) => Chunk::Late,
            Err(mpsc::TryRecvError::Disconnected) => Chunk::Ended,
        }
    }

    /// The next chunk, waiting for it; `None` at the end.
    pub fn next_chunk(&self) -> Option<Vec<f32>> {
        self.chunks.recv().ok()
    }

    /// How long `samples` interleaved samples of this stream play.
    pub fn duration_of(&self, samples: u64) -> Duration {
        let frames_per_second =
            u64::from(self.channels.max(1)) * u64::from(self.sample_rate.max(1));
        Duration::from_nanos(
            (u128::from(samples) * 1_000_000_000 / u128::from(frames_per_second)) as u64,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_given_at_once_arrive_then_end() {
        let stream = PcmStream::from_samples(2, 48_000, vec![0.5; 8]);
        assert!(stream.is_buffered());
        assert_eq!(stream.try_next_chunk(), Chunk::Ready(vec![0.5; 8]));
        assert_eq!(stream.try_next_chunk(), Chunk::Ended);
    }

    #[test]
    fn a_fed_stream_waits_then_ends_when_the_producer_stops() {
        let (stream, sender) = PcmStream::channel(1, 8_000, 2, None);
        assert!(!stream.is_buffered());
        assert_eq!(stream.try_next_chunk(), Chunk::Late);
        sender.send(vec![1.0]);
        assert!(stream.is_buffered());
        drop(sender);
        assert_eq!(stream.next_chunk(), Some(vec![1.0]));
        assert_eq!(stream.next_chunk(), None);
    }

    #[test]
    fn durations_count_frames_of_all_channels() {
        let stream = PcmStream::from_samples(2, 48_000, Vec::new());
        assert_eq!(stream.duration_of(96_000), Duration::from_secs(1));
        assert_eq!(stream.duration_of(48), Duration::from_micros(500));
    }
}
