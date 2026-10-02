//! What a media file holds, from `ffprobe`'s JSON.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::{Ffmpeg, MediaError, path_arg, process};

#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration: Duration,
    /// The first video stream, if any (cover art in audio files is not one).
    pub video: Option<VideoStream>,
    /// The first audio stream, if any.
    pub audio: Option<AudioStream>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoStream {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Frames per second, as a fraction (30000/1001 for 29.97).
    pub frame_rate: (u32, u32),
}

impl VideoStream {
    pub fn fps(&self) -> f64 {
        f64::from(self.frame_rate.0) / f64::from(self.frame_rate.1.max(1))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioStream {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
}

impl Ffmpeg {
    /// Reads duration and streams. Fast: ffprobe reads headers, not frames.
    pub fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        let mut command = self.ffprobe();
        command
            .args(["-print_format", "json", "-show_format", "-show_streams"])
            .arg(path_arg(path));
        let json = process::output(command)?;
        parse(&json)
    }
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Format,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    #[serde(default)]
    disposition: Disposition,
}

#[derive(Deserialize, Default)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

fn parse(json: &[u8]) -> Result<MediaInfo, MediaError> {
    let probe: Probe =
        serde_json::from_slice(json).map_err(|error| MediaError::Parse(error.to_string()))?;
    let duration = probe
        .format
        .duration
        .as_deref()
        .and_then(|seconds| seconds.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(Duration::from_secs_f64)
        .ok_or_else(|| MediaError::Parse("no duration".into()))?;
    let video = probe
        .streams
        .iter()
        .find(|stream| {
            stream.codec_type.as_deref() == Some("video") && stream.disposition.attached_pic == 0
        })
        .and_then(|stream| {
            Some(VideoStream {
                codec: stream.codec_name.clone().unwrap_or_default(),
                width: stream.width?,
                height: stream.height?,
                frame_rate: [&stream.avg_frame_rate, &stream.r_frame_rate]
                    .into_iter()
                    .find_map(|rate| rate.as_deref().and_then(fraction))?,
            })
        });
    let audio = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .map(|stream| AudioStream {
            codec: stream.codec_name.clone().unwrap_or_default(),
            sample_rate: stream
                .sample_rate
                .as_deref()
                .and_then(|rate| rate.parse().ok())
                .unwrap_or(0),
            channels: stream.channels.unwrap_or(0),
        });
    Ok(MediaInfo {
        duration,
        video,
        audio,
    })
}

/// "30000/1001" → (30000, 1001); "0/0" (unknown) → None.
fn fraction(text: &str) -> Option<(u32, u32)> {
    let (numerator, denominator) = text.split_once('/')?;
    let numerator: u32 = numerator.parse().ok()?;
    let denominator: u32 = denominator.parse().ok()?;
    (numerator > 0 && denominator > 0).then_some((numerator, denominator))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIP: &str = r#"{
        "streams": [
            {"codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080,
             "avg_frame_rate": "30000/1001", "r_frame_rate": "30000/1001",
             "disposition": {"attached_pic": 0}},
            {"codec_type": "audio", "codec_name": "aac", "sample_rate": "48000", "channels": 2}
        ],
        "format": {"duration": "5.005000"}
    }"#;

    #[test]
    fn reads_streams_and_duration() {
        let info = parse(CLIP.as_bytes()).unwrap();
        assert_eq!(info.duration, Duration::from_micros(5_005_000));
        let video = info.video.unwrap();
        assert_eq!((video.width, video.height), (1920, 1080));
        assert_eq!(video.frame_rate, (30000, 1001));
        assert!((video.fps() - 29.97).abs() < 0.01);
        let audio = info.audio.unwrap();
        assert_eq!(
            (audio.codec.as_str(), audio.sample_rate, audio.channels),
            ("aac", 48000, 2)
        );
    }

    #[test]
    fn cover_art_is_not_video() {
        let json = r#"{
            "streams": [
                {"codec_type": "audio", "codec_name": "mp3", "sample_rate": "44100", "channels": 1},
                {"codec_type": "video", "codec_name": "mjpeg", "width": 500, "height": 500,
                 "avg_frame_rate": "0/0", "r_frame_rate": "90000/1",
                 "disposition": {"attached_pic": 1}}
            ],
            "format": {"duration": "3.0"}
        }"#;
        let info = parse(json.as_bytes()).unwrap();
        assert!(info.video.is_none());
        assert_eq!(info.audio.unwrap().codec, "mp3");
    }

    #[test]
    fn unknown_average_rate_falls_back_to_the_base_rate() {
        assert_eq!(fraction("0/0"), None);
        let json = r#"{"streams": [{"codec_type": "video", "width": 2, "height": 2,
            "avg_frame_rate": "0/0", "r_frame_rate": "25/1"}], "format": {"duration": "1"}}"#;
        assert_eq!(
            parse(json.as_bytes()).unwrap().video.unwrap().frame_rate,
            (25, 1)
        );
    }

    #[test]
    fn no_duration_is_an_error() {
        let json = r#"{"streams": [], "format": {}}"#;
        assert!(matches!(parse(json.as_bytes()), Err(MediaError::Parse(_))));
    }
}
