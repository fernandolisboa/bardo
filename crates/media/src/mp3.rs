//! MP3 frame reading: duration and joining. A long narration is spoken in
//! parts, each its own MP3 stream; joining keeps only their audio frames,
//! so the result plays as one stream and each part starts exactly where
//! the frames before it end.
//!
//! Tags (ID3v2 at the start, ID3v1 at the end) and the encoder's info
//! frame (`Xing`/`Info`/`VBRI`) are dropped: an info frame describes only
//! its own part and would make a player stop or seek wrongly in the whole.

use std::ops::Range;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Mp3Error {
    #[error("no MP3 audio frames found")]
    NoFrames,
    /// Parts in different MPEG versions or sample rates cannot play as one
    /// stream.
    #[error("the parts differ in format and cannot be joined")]
    MixedFormats,
}

/// What every frame of a stream shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub sample_rate: u32,
    pub samples_per_frame: u32,
    pub mpeg1: bool,
}

/// The audio frames of one MP3 stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frames {
    pub format: Format,
    /// Byte ranges of the audio frames, in order.
    pub frames: Vec<Range<usize>>,
}

impl Frames {
    /// How long the frames play.
    pub fn duration(&self) -> Duration {
        frames_duration(self.frames.len(), self.format)
    }
}

fn frames_duration(frames: usize, format: Format) -> Duration {
    let samples = frames as u64 * u64::from(format.samples_per_frame);
    Duration::from_nanos(samples * 1_000_000_000 / u64::from(format.sample_rate))
}

/// One frame header, as far as reading frames needs.
#[derive(Debug, Clone, Copy)]
struct Header {
    format: Format,
    len: usize,
    mono: bool,
}

/// Layer III bitrates in kbit/s by index; 0 is "free", which Bardo never
/// gets and cannot size.
const MPEG1_BITRATES: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const MPEG2_BITRATES: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

fn header(bytes: &[u8]) -> Option<Header> {
    let b = bytes.get(..4)?;
    if b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (b[1] >> 3) & 0b11;
    let layer = (b[1] >> 1) & 0b11;
    if version == 0b01 || layer != 0b01 {
        // Reserved version, or not Layer III.
        return None;
    }
    let mpeg1 = version == 0b11;
    let bitrate_index = usize::from(b[2] >> 4);
    let rate_index = usize::from((b[2] >> 2) & 0b11);
    if bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }
    let bitrate = if mpeg1 {
        MPEG1_BITRATES[bitrate_index]
    } else {
        MPEG2_BITRATES[bitrate_index]
    } * 1000;
    let base_rate = [44_100, 48_000, 32_000][rate_index];
    let sample_rate = match version {
        0b11 => base_rate,
        0b10 => base_rate / 2,
        _ => base_rate / 4,
    };
    let padding = usize::from((b[2] >> 1) & 1);
    let samples_per_frame = if mpeg1 { 1152 } else { 576 };
    let len = (samples_per_frame / 8 * bitrate / sample_rate) as usize + padding;
    Some(Header {
        format: Format {
            sample_rate,
            samples_per_frame,
            mpeg1,
        },
        len,
        mono: b[3] >> 6 == 0b11,
    })
}

/// Bytes an ID3v2 tag takes at the start, if there is one.
fn id3v2_len(bytes: &[u8]) -> usize {
    match bytes.get(..10) {
        Some(head) if &head[..3] == b"ID3" => {
            let size = head[6..10]
                .iter()
                .fold(0usize, |size, b| (size << 7) | usize::from(b & 0x7F));
            let footer = if head[5] & 0x10 != 0 { 10 } else { 0 };
            (10 + size + footer).min(bytes.len())
        }
        _ => 0,
    }
}

/// Whether the frame is the encoder's info frame rather than audio.
fn is_info_frame(frame: &[u8], header: &Header) -> bool {
    let side_info = match (header.format.mpeg1, header.mono) {
        (true, false) => 32,
        (true, true) | (false, false) => 17,
        (false, true) => 9,
    };
    let tag_at = |at: usize| frame.get(at..at + 4);
    matches!(tag_at(4 + side_info), Some(b"Xing" | b"Info")) || tag_at(36) == Some(b"VBRI")
}

/// Finds the audio frames of an MP3 stream. Reading stops at the first
/// bytes that are not a frame of the same format (an ID3v1 tag, junk).
pub fn frames(bytes: &[u8]) -> Result<Frames, Mp3Error> {
    let mut at = id3v2_len(bytes);
    // Skip anything before the first frame that the next frame confirms.
    let first = loop {
        let Some(found) = header(&bytes[at.min(bytes.len())..]) else {
            at += 1;
            if at + 4 > bytes.len() {
                return Err(Mp3Error::NoFrames);
            }
            continue;
        };
        let next = bytes.get(at + found.len..).and_then(header);
        let confirmed =
            next.is_some_and(|next| next.format == found.format) || at + found.len == bytes.len();
        if confirmed {
            break found;
        }
        at += 1;
    };
    let format = first.format;
    let mut ranges = Vec::new();
    while let Some(found) = bytes.get(at..).and_then(header) {
        let end = at + found.len;
        if found.format != format || end > bytes.len() {
            break;
        }
        if !(ranges.is_empty() && is_info_frame(&bytes[at..end], &found)) {
            ranges.push(at..end);
        }
        at = end;
    }
    if ranges.is_empty() {
        return Err(Mp3Error::NoFrames);
    }
    Ok(Frames {
        format,
        frames: ranges,
    })
}

/// MP3 parts joined into one stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joined {
    pub bytes: Vec<u8>,
    /// When each part starts in the joined stream.
    pub starts: Vec<Duration>,
    pub duration: Duration,
}

/// Joins MP3 parts, in order, into one stream of their audio frames.
pub fn join(parts: &[&[u8]]) -> Result<Joined, Mp3Error> {
    let mut bytes = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    let mut starts = Vec::with_capacity(parts.len());
    let mut format: Option<Format> = None;
    let mut total_frames = 0;
    for part in parts {
        let found = frames(part)?;
        if format.is_some_and(|format| format != found.format) {
            return Err(Mp3Error::MixedFormats);
        }
        format = Some(found.format);
        starts.push(frames_duration(total_frames, found.format));
        total_frames += found.frames.len();
        for frame in &found.frames {
            bytes.extend_from_slice(&part[frame.clone()]);
        }
    }
    let format = format.ok_or(Mp3Error::NoFrames)?;
    Ok(Joined {
        bytes,
        starts,
        duration: frames_duration(total_frames, format),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-second 440 Hz tone, 44.1 kHz mono at 128 kbit/s, as ffmpeg
    /// writes it: an ID3v2 tag, an `Info` frame, then the audio frames.
    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone-1s.mp3");

    /// The same tone without tag or info frame, as a raw stream.
    const RAW_TONE: &[u8] = include_bytes!("../tests/fixtures/tone-1s-raw.mp3");

    /// The encoder adds up to about two frames of delay and padding.
    fn about_one_second(duration: Duration) -> bool {
        (1_000..=1_080).contains(&duration.as_millis())
    }

    #[test]
    fn reads_the_frames_and_their_duration() {
        let found = frames(TONE).unwrap();
        assert_eq!(
            found.format,
            Format {
                sample_rate: 44_100,
                samples_per_frame: 1152,
                mpeg1: true,
            }
        );
        assert!(about_one_second(found.duration()), "{:?}", found.duration());
        assert!(found.frames.iter().all(|f| matches!(f.len(), 417 | 418)));
    }

    #[test]
    fn tags_and_the_info_frame_are_not_audio() {
        assert!(TONE.starts_with(b"ID3"));
        let tagged = frames(TONE).unwrap();
        let raw = frames(RAW_TONE).unwrap();
        assert_eq!(tagged.frames.len(), raw.frames.len());
        let first = &TONE[tagged.frames[0].clone()];
        assert!(
            !first.windows(4).any(|w| w == b"Info"),
            "the info frame is skipped"
        );
    }

    #[test]
    fn trailing_bytes_that_are_not_frames_are_ignored() {
        let mut with_v1_tag = RAW_TONE.to_vec();
        with_v1_tag.extend_from_slice(b"TAG");
        with_v1_tag.extend_from_slice(&[0; 125]);
        assert_eq!(frames(&with_v1_tag).unwrap(), frames(RAW_TONE).unwrap());
    }

    #[test]
    fn something_else_is_not_mp3() {
        assert_eq!(frames(b"not audio at all"), Err(Mp3Error::NoFrames));
        assert_eq!(frames(&[]), Err(Mp3Error::NoFrames));
        assert_eq!(join(&[]), Err(Mp3Error::NoFrames));
    }

    #[test]
    fn joined_parts_play_back_to_back() {
        let one = frames(TONE).unwrap();
        let joined = join(&[TONE, RAW_TONE, TONE]).unwrap();

        assert_eq!(joined.starts.len(), 3);
        assert_eq!(joined.starts[0], Duration::ZERO);
        assert_eq!(joined.starts[1], one.duration());
        assert_eq!(joined.starts[2], one.duration() * 2);
        assert_eq!(joined.duration, one.duration() * 3);

        let again = frames(&joined.bytes).unwrap();
        assert_eq!(again.frames.len(), one.frames.len() * 3);
        assert_eq!(again.frames[0].start, 0, "no tag or info frame left");
        assert_eq!(again.duration(), joined.duration);
    }

    #[test]
    fn parts_in_different_formats_are_not_joined() {
        const TONE_48K: &[u8] = include_bytes!("../tests/fixtures/tone-48k-raw.mp3");
        assert_eq!(frames(TONE_48K).unwrap().format.sample_rate, 48_000);
        assert_eq!(join(&[RAW_TONE, TONE_48K]), Err(Mp3Error::MixedFormats));
    }
}
