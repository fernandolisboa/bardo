//! Audio files the user brings (an imported narration): which format they
//! are in and how long they play, read from the file itself so a renamed
//! file is still recognized. MP3 and uncompressed WAV are read without
//! ffmpeg; other formats wait for it (ADR-0001).

use std::ops::Range;
use std::time::Duration;

use crate::mp3;

/// An audio format Bardo plays and keeps as is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioFormat {
    Mp3,
    /// RIFF/WAVE with PCM or floating point samples.
    Wav,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 2] = [AudioFormat::Mp3, AudioFormat::Wav];

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Wav => "wav",
        }
    }

    /// The format with this extension (no dot), ignoring case.
    pub fn from_extension(extension: &str) -> Option<AudioFormat> {
        AudioFormat::ALL
            .into_iter()
            .find(|format| format.extension().eq_ignore_ascii_case(extension))
    }

    /// The format a file name's extension names, ignoring case.
    pub fn from_file_name(name: &str) -> Option<AudioFormat> {
        let (_, extension) = name.rsplit_once('.')?;
        AudioFormat::from_extension(extension)
    }

    /// The media type, for uploads.
    pub fn mime_type(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "audio/mpeg",
            AudioFormat::Wav => "audio/wav",
        }
    }
}

/// What a file turned out to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioInfo {
    pub format: AudioFormat,
    pub duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    /// Not MP3 or WAV (or WAV with compressed samples).
    #[error("not an MP3 or WAV file")]
    UnsupportedFormat,
    /// The right format, but no sound in it.
    #[error("the file has no audio")]
    NoAudio,
}

/// Reads the format and length of an audio file's bytes.
pub fn probe(bytes: &[u8]) -> Result<AudioInfo, ProbeError> {
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        return wav_duration(bytes).map(|duration| AudioInfo {
            format: AudioFormat::Wav,
            duration,
        });
    }
    let frames = mp3::frames(bytes).map_err(|_| ProbeError::UnsupportedFormat)?;
    // Real MP3 is frames from the first one to the end, bar a closing tag.
    // Anything else (a video, an AAC file) may hold a few bytes that look
    // like frames, but not that many.
    let first = frames.frames[0].start;
    let audio: usize = frames.frames.iter().map(|frame| frame.len()).sum();
    if audio * 2 < bytes.len() - first {
        return Err(ProbeError::UnsupportedFormat);
    }
    Ok(AudioInfo {
        format: AudioFormat::Mp3,
        duration: frames.duration(),
    })
}

/// PCM (1) and IEEE float (3), plain or wrapped in the extensible header.
const WAV_FORMATS: [u16; 2] = [1, 3];
const WAV_EXTENSIBLE: u16 = 0xFFFE;

/// Where a WAV file keeps its format and its samples.
struct WavLayout {
    /// The whole `fmt ` chunk, header included.
    format: Range<usize>,
    rate: u32,
    /// Bytes per sample frame (every channel).
    align: u16,
    /// The samples that are there.
    data: Range<usize>,
}

fn wav_layout(bytes: &[u8]) -> Result<WavLayout, ProbeError> {
    let u16_at = |at: usize| {
        bytes
            .get(at..at.checked_add(2)?)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        bytes
            .get(at..at.checked_add(4)?)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut at: usize = 12;
    let mut block: Option<(Range<usize>, u32, u16)> = None;
    while let (Some(id), Some(size)) = (
        bytes.get(at..at.saturating_add(4)),
        u32_at(at.saturating_add(4)),
    ) {
        let body = at + 8;
        match id {
            b"fmt " => {
                let mut format = u16_at(body).ok_or(ProbeError::UnsupportedFormat)?;
                if format == WAV_EXTENSIBLE {
                    // The real format leads the sub-format GUID.
                    format = u16_at(body + 24).ok_or(ProbeError::UnsupportedFormat)?;
                }
                let rate = u32_at(body + 4).ok_or(ProbeError::UnsupportedFormat)?;
                let align = u16_at(body + 12).ok_or(ProbeError::UnsupportedFormat)?;
                if !WAV_FORMATS.contains(&format) || rate == 0 || align == 0 {
                    return Err(ProbeError::UnsupportedFormat);
                }
                let end = body.saturating_add(size as usize).min(bytes.len());
                block = Some((at..end, rate, align));
            }
            b"data" => {
                let (format, rate, align) = block.ok_or(ProbeError::UnsupportedFormat)?;
                // Recorders that stream may leave the size unset or too big;
                // the bytes that are there are what plays.
                let len = (size as usize).min(bytes.len() - body);
                let len = len - len % usize::from(align);
                if len == 0 {
                    return Err(ProbeError::NoAudio);
                }
                return Ok(WavLayout {
                    format,
                    rate,
                    align,
                    data: body..body + len,
                });
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    Err(ProbeError::NoAudio)
}

fn wav_duration(bytes: &[u8]) -> Result<Duration, ProbeError> {
    let layout = wav_layout(bytes)?;
    let samples = (layout.data.len() / usize::from(layout.align)) as u64;
    Ok(Duration::from_nanos(
        samples * 1_000_000_000 / u64::from(layout.rate),
    ))
}

/// The stretch `span` of a WAV file, as a WAV file of the same format:
/// what a provider that takes a limited length at once is sent in parts.
/// Cut on whole sample frames; a span past the end stops at it.
pub fn wav_slice(bytes: &[u8], span: Range<Duration>) -> Result<Vec<u8>, ProbeError> {
    if !(bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE")) {
        return Err(ProbeError::UnsupportedFormat);
    }
    let layout = wav_layout(bytes)?;
    let align = usize::from(layout.align);
    let frames = layout.data.len() / align;
    let frame_at = |at: Duration| {
        let frame = at.as_nanos() * u128::from(layout.rate) / 1_000_000_000;
        usize::try_from(frame).unwrap_or(usize::MAX).min(frames)
    };
    let (first, last) = (frame_at(span.start), frame_at(span.end));
    let samples =
        &bytes[layout.data.start + first * align..layout.data.start + last.max(first) * align];
    let format = &bytes[layout.format.clone()];
    let mut wav = Vec::with_capacity(12 + format.len() + 8 + samples.len() + 1);
    wav.extend_from_slice(b"RIFF");
    let riff = 4 + format.len() + (format.len() & 1) + 8 + samples.len() + (samples.len() & 1);
    wav.extend_from_slice(&(riff as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(format);
    if format.len() & 1 == 1 {
        wav.push(0);
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(samples);
    if samples.len() & 1 == 1 {
        wav.push(0);
    }
    Ok(wav)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone-1s.mp3");

    /// A WAV file of `seconds` of silence: 16-bit PCM, 8 kHz, mono, with
    /// an extra chunk before the samples as some recorders write.
    pub(crate) fn silent_wav(seconds: u32) -> Vec<u8> {
        let rate: u32 = 8_000;
        let data = vec![0u8; (rate * 2 * seconds) as usize];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + 12 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits
        wav.extend_from_slice(b"JUNK");
        wav.extend_from_slice(&3u32.to_le_bytes());
        wav.extend_from_slice(b"abc\0"); // odd size, padded
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    #[test]
    fn mp3_is_recognized_with_its_length() {
        let info = probe(TONE).unwrap();
        assert_eq!(info.format, AudioFormat::Mp3);
        assert_eq!(info.duration, mp3::frames(TONE).unwrap().duration());
    }

    #[test]
    fn wav_is_recognized_with_its_length() {
        let info = probe(&silent_wav(3)).unwrap();
        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(info.duration, Duration::from_secs(3));
    }

    #[test]
    fn a_wav_whose_size_was_never_written_plays_what_is_there() {
        let mut wav = silent_wav(2);
        let data_size_at = wav.len() - 32_000 - 4;
        wav[data_size_at..data_size_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(probe(&wav).unwrap().duration, Duration::from_secs(2));
    }

    #[test]
    fn wav_without_samples_has_no_audio() {
        let mut wav = silent_wav(1);
        wav.truncate(wav.len() - 16_000);
        let data_size_at = wav.len() - 4;
        wav[data_size_at..].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(probe(&wav), Err(ProbeError::NoAudio));
    }

    #[test]
    fn compressed_wav_and_other_files_are_not_supported() {
        let mut adpcm = silent_wav(1);
        adpcm[20..22].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(probe(&adpcm), Err(ProbeError::UnsupportedFormat));
        assert_eq!(probe(b"not audio"), Err(ProbeError::UnsupportedFormat));
        assert_eq!(probe(&[]), Err(ProbeError::UnsupportedFormat));
        // A container with a little MP3 inside is not an MP3 file.
        let mut boxed = vec![0x42u8; TONE.len() * 3];
        boxed[100..100 + TONE.len()].copy_from_slice(TONE);
        assert_eq!(probe(&boxed), Err(ProbeError::UnsupportedFormat));
    }

    /// One second of silence in the extensible header, its sub-format
    /// GUID led by `format`.
    fn extensible_wav(format: u16) -> Vec<u8> {
        let plain = silent_wav(1);
        let mut wav = plain[..20].to_vec();
        wav[16..20].copy_from_slice(&40u32.to_le_bytes());
        wav.extend_from_slice(&0xFFFEu16.to_le_bytes());
        wav.extend_from_slice(&plain[22..36]);
        wav.extend_from_slice(&22u16.to_le_bytes()); // extension size
        wav.extend_from_slice(&16u16.to_le_bytes()); // valid bits
        wav.extend_from_slice(&4u32.to_le_bytes()); // channel mask
        wav.extend_from_slice(&format.to_le_bytes());
        wav.extend_from_slice(&[0; 14]); // rest of the GUID
        wav.extend_from_slice(&plain[36..]);
        wav
    }

    #[test]
    fn the_extensible_header_is_read_for_its_real_format() {
        let pcm = probe(&extensible_wav(1)).unwrap();
        assert_eq!(pcm.duration, Duration::from_secs(1));
        assert_eq!(
            probe(&extensible_wav(0x55)),
            Err(ProbeError::UnsupportedFormat),
            "MP3 inside a WAV header"
        );
    }

    #[test]
    fn a_chunk_claiming_more_than_the_file_ends_the_walk() {
        let mut wav = silent_wav(1);
        wav[40..44].copy_from_slice(&u32::MAX.to_le_bytes()); // JUNK size
        assert_eq!(probe(&wav), Err(ProbeError::NoAudio));
    }

    #[test]
    fn a_wav_is_sliced_on_sample_frames_keeping_its_format() {
        let mut wav = silent_wav(3);
        // Mark each second so the slice can be checked: 8,000 frames of
        // two bytes a second.
        let data = wav.len() - 48_000;
        for second in 0..3u8 {
            let at = data + usize::from(second) * 16_000;
            wav[at..at + 16_000].fill(second + 1);
        }
        let ms = Duration::from_millis;
        let middle = wav_slice(&wav, ms(1_000)..ms(2_000)).unwrap();
        let info = probe(&middle).unwrap();
        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(info.duration, ms(1_000));
        assert!(middle.ends_with(&[2u8; 16_000]));
        assert_eq!(&middle[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(middle[4..8].try_into().unwrap()) as usize,
            middle.len() - 8
        );

        // Past the end stops at it; an empty span is an empty file.
        assert_eq!(
            probe(&wav_slice(&wav, ms(2_500)..ms(9_000)).unwrap())
                .unwrap()
                .duration,
            ms(500)
        );
        assert_eq!(
            probe(&wav_slice(&wav, ms(500)..ms(500)).unwrap()),
            Err(ProbeError::NoAudio)
        );
        assert_eq!(
            wav_slice(TONE, ms(0)..ms(100)),
            Err(ProbeError::UnsupportedFormat)
        );
    }

    #[test]
    fn formats_follow_file_extensions() {
        assert_eq!(
            AudioFormat::from_file_name("take 2.WAV"),
            Some(AudioFormat::Wav)
        );
        assert_eq!(
            AudioFormat::from_file_name("narration-1.mp3"),
            Some(AudioFormat::Mp3)
        );
        assert_eq!(AudioFormat::from_file_name("voice.m4a"), None);
        assert_eq!(AudioFormat::from_file_name("mp3"), None);
    }
}
