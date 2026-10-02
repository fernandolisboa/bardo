//! Waveform peaks for the timeline's audio tracks: ffmpeg decodes to mono
//! 32-bit float samples on a pipe, and the peaks are taken here.

use std::path::Path;

use super::{Ffmpeg, MediaError, path_arg, process};

/// Sample rate the audio is decoded at for peaks: plenty for a drawing,
/// and cheap (a minute is under 2 MB of samples).
const SAMPLE_RATE: u32 = 8_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    pub peaks_per_second: u32,
    /// Loudest absolute sample in each slice, 0.0 to 1.0.
    pub peaks: Vec<f32>,
}

impl Ffmpeg {
    /// Peaks of the file's first audio stream, `peaks_per_second` of them
    /// for each second of audio (at most 8000).
    pub fn waveform(&self, path: &Path, peaks_per_second: u32) -> Result<Waveform, MediaError> {
        let peaks_per_second = peaks_per_second.clamp(1, SAMPLE_RATE);
        let mut command = self.ffmpeg();
        command
            .arg("-i")
            .arg(path_arg(path))
            .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string()])
            .args(["-f", "f32le", "pipe:1"]);
        let bytes = process::output(command)?;
        Ok(Waveform {
            peaks_per_second,
            peaks: peaks(&bytes, (SAMPLE_RATE / peaks_per_second) as usize),
        })
    }
}

/// Max absolute value of each `slice` samples of little-endian f32.
fn peaks(bytes: &[u8], slice: usize) -> Vec<f32> {
    let samples: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|sample| f32::from_le_bytes(*sample))
        .collect();
    samples
        .chunks(slice.max(1))
        .map(|chunk| {
            chunk
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
                .min(1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn peaks_take_the_loudest_sample_of_each_slice() {
        let bytes = le(&[0.1, -0.5, 0.2, 0.0, 0.3, -0.25, 2.0]);
        assert_eq!(peaks(&bytes, 3), vec![0.5, 0.3, 1.0]);
    }

    #[test]
    fn no_samples_no_peaks() {
        assert!(peaks(&[], 100).is_empty());
    }
}
