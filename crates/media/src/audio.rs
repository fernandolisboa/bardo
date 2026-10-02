//! Audio files the user brings (an imported narration): which format they
//! are in and how long they play, read from the file itself so a renamed
//! file is still recognized. MP3 and uncompressed WAV are read without
//! ffmpeg; other formats wait for it (ADR-0001).

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

fn wav_duration(bytes: &[u8]) -> Result<Duration, ProbeError> {
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
    let mut block: Option<(u32, u16)> = None;
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
                block = Some((rate, align));
            }
            b"data" => {
                let (rate, align) = block.ok_or(ProbeError::UnsupportedFormat)?;
                // Recorders that stream may leave the size unset or too big;
                // the bytes that are there are what plays.
                let len = (size as usize).min(bytes.len() - body);
                let samples = (len / usize::from(align)) as u64;
                if samples == 0 {
                    return Err(ProbeError::NoAudio);
                }
                return Ok(Duration::from_nanos(
                    samples * 1_000_000_000 / u64::from(rate),
                ));
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    Err(ProbeError::NoAudio)
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
