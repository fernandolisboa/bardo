//! H.264 and HEVC encoders: which ones this machine can use, best first.
//! The build lists hardware encoders whether or not the GPU and driver are
//! there, so each one is tried on a few blank frames before Bardo relies on
//! it.

use std::process::Command;

use bardo_domain::VideoCodec;

use super::{Ffmpeg, MediaError, process};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoEncoder {
    /// NVIDIA GPUs (driver 570 or newer for the bundled ffmpeg 8.1).
    Nvenc,
    /// AMD GPUs.
    Amf,
    /// Intel GPUs (Quick Sync).
    Qsv,
    /// Windows' own Media Foundation encoder; uses the GPU when the driver
    /// offers one, software otherwise. Missing on Windows "N" editions
    /// without the Media Feature Pack.
    MediaFoundation,
    /// Cisco's OpenH264, built into the LGPL ffmpeg: software, everywhere.
    OpenH264,
    /// HEVC on NVIDIA GPUs.
    HevcNvenc,
    /// HEVC on AMD GPUs.
    HevcAmf,
    /// HEVC on Intel GPUs.
    HevcQsv,
    /// HEVC through Media Foundation.
    HevcMediaFoundation,
    /// Kvazaar, the LGPL build's software HEVC encoder: slower than
    /// OpenH264, everywhere.
    Kvazaar,
}

impl VideoEncoder {
    /// H.264, best first: dedicated GPU encoders, then the OS encoder, then
    /// software.
    pub const PREFERENCE: [VideoEncoder; 5] = [
        VideoEncoder::Nvenc,
        VideoEncoder::Amf,
        VideoEncoder::Qsv,
        VideoEncoder::MediaFoundation,
        VideoEncoder::OpenH264,
    ];

    /// HEVC, best first, in the same order.
    pub const HEVC_PREFERENCE: [VideoEncoder; 5] = [
        VideoEncoder::HevcNvenc,
        VideoEncoder::HevcAmf,
        VideoEncoder::HevcQsv,
        VideoEncoder::HevcMediaFoundation,
        VideoEncoder::Kvazaar,
    ];

    /// The encoders of `codec`, best first.
    pub fn preference(codec: VideoCodec) -> [VideoEncoder; 5] {
        match codec {
            VideoCodec::H264 => Self::PREFERENCE,
            VideoCodec::Hevc => Self::HEVC_PREFERENCE,
        }
    }

    /// ffmpeg's name for it.
    pub fn name(self) -> &'static str {
        match self {
            VideoEncoder::Nvenc => "h264_nvenc",
            VideoEncoder::Amf => "h264_amf",
            VideoEncoder::Qsv => "h264_qsv",
            VideoEncoder::MediaFoundation => "h264_mf",
            VideoEncoder::OpenH264 => "libopenh264",
            VideoEncoder::HevcNvenc => "hevc_nvenc",
            VideoEncoder::HevcAmf => "hevc_amf",
            VideoEncoder::HevcQsv => "hevc_qsv",
            VideoEncoder::HevcMediaFoundation => "hevc_mf",
            VideoEncoder::Kvazaar => "libkvazaar",
        }
    }

    /// The encoder ffmpeg calls `name`.
    pub fn from_name(name: &str) -> Option<VideoEncoder> {
        Self::PREFERENCE
            .into_iter()
            .chain(Self::HEVC_PREFERENCE)
            .find(|encoder| encoder.name() == name)
    }

    /// What people call it: the GPU vendor's technology or the library.
    pub fn label(self) -> &'static str {
        match self {
            VideoEncoder::Nvenc | VideoEncoder::HevcNvenc => "NVIDIA NVENC",
            VideoEncoder::Amf | VideoEncoder::HevcAmf => "AMD AMF",
            VideoEncoder::Qsv | VideoEncoder::HevcQsv => "Intel Quick Sync",
            VideoEncoder::MediaFoundation | VideoEncoder::HevcMediaFoundation => "Media Foundation",
            VideoEncoder::OpenH264 => "OpenH264",
            VideoEncoder::Kvazaar => "Kvazaar",
        }
    }

    pub fn codec(self) -> VideoCodec {
        if Self::HEVC_PREFERENCE.contains(&self) {
            VideoCodec::Hevc
        } else {
            VideoCodec::H264
        }
    }

    pub fn is_hardware(self) -> bool {
        matches!(
            self,
            VideoEncoder::Nvenc
                | VideoEncoder::Amf
                | VideoEncoder::Qsv
                | VideoEncoder::HevcNvenc
                | VideoEncoder::HevcAmf
                | VideoEncoder::HevcQsv
        )
    }

    /// Output options for a target bitrate (bits per second) and keyframe
    /// interval (frames). Peak and buffer follow the usual 1.5x / 2x.
    pub fn args(self, bitrate: u32, keyframe_interval: u32) -> Vec<String> {
        let mut args: Vec<String> = vec!["-c:v".into(), self.name().into()];
        let quality: &[&str] = match self {
            VideoEncoder::Nvenc => &[
                "-preset",
                "p5",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-profile:v",
                "high",
            ],
            VideoEncoder::HevcNvenc => &[
                "-preset",
                "p5",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-profile:v",
                "main",
            ],
            VideoEncoder::Amf => &[
                "-quality",
                "quality",
                "-rc",
                "vbr_peak",
                "-profile:v",
                "high",
            ],
            VideoEncoder::HevcAmf => &[
                "-quality",
                "quality",
                "-rc",
                "vbr_peak",
                "-profile:v",
                "main",
            ],
            VideoEncoder::Qsv => &["-preset", "slower", "-profile:v", "high"],
            VideoEncoder::HevcQsv => &["-preset", "slower", "-profile:v", "main"],
            VideoEncoder::MediaFoundation | VideoEncoder::HevcMediaFoundation => {
                &["-rate_control", "pc_vbr", "-scenario", "archive"]
            }
            VideoEncoder::OpenH264 => &["-profile:v", "high", "-allow_skip_frames", "0"],
            VideoEncoder::Kvazaar => &["-kvazaar-params", "preset=fast"],
        };
        args.extend(quality.iter().map(|arg| arg.to_string()));
        args.extend([
            "-b:v".into(),
            bitrate.to_string(),
            "-maxrate".into(),
            (bitrate / 2 * 3).to_string(),
            "-bufsize".into(),
            (bitrate * 2).to_string(),
            "-g".into(),
            keyframe_interval.to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]);
        // Apple's players and most uploaders read HEVC in MP4 only under
        // the `hvc1` tag.
        if self.codec() == VideoCodec::Hevc {
            args.extend(["-tag:v".into(), "hvc1".into()]);
        }
        args
    }
}

/// The encoders that worked when tried, each codec's in preference order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoders {
    pub working: Vec<VideoEncoder>,
}

impl Encoders {
    /// The best working encoder of `codec`.
    pub fn best(&self, codec: VideoCodec) -> Result<VideoEncoder, MediaError> {
        self.of(codec).next().ok_or(MediaError::NoEncoder)
    }

    /// The best working encoder of `codec` that is not hardware, for the
    /// fallback.
    pub fn software(&self, codec: VideoCodec) -> Result<VideoEncoder, MediaError> {
        self.of(codec)
            .find(|encoder| !encoder.is_hardware())
            .ok_or(MediaError::NoEncoder)
    }

    fn of(&self, codec: VideoCodec) -> impl Iterator<Item = VideoEncoder> + '_ {
        self.working
            .iter()
            .copied()
            .filter(move |encoder| encoder.codec() == codec)
    }
}

impl Ffmpeg {
    /// Tries every encoder on a few blank frames. Takes well under a second
    /// per encoder; the app runs it once per start and keeps the result.
    pub fn detect_encoders(&self) -> Encoders {
        let working = VideoEncoder::PREFERENCE
            .into_iter()
            .chain(VideoEncoder::HEVC_PREFERENCE)
            .filter(|encoder| self.encoder_works(*encoder))
            .collect();
        Encoders { working }
    }

    pub fn encoder_works(&self, encoder: VideoEncoder) -> bool {
        let mut command: Command = self.ffmpeg();
        command
            .args(["-f", "lavfi", "-i", "color=c=black:s=256x256:r=30:d=0.2"])
            .args(encoder.args(1_000_000, 30))
            .args(["-f", "null", "-"]);
        process::output(command).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_set_codec_rate_and_keyframes() {
        let args = VideoEncoder::Nvenc.args(12_000_000, 60);
        let joined = args.join(" ");
        assert!(joined.starts_with("-c:v h264_nvenc "));
        assert!(joined.contains("-b:v 12000000 -maxrate 18000000 -bufsize 24000000 -g 60"));
        assert!(joined.ends_with("-pix_fmt yuv420p"));
    }

    #[test]
    fn hevc_is_tagged_for_mp4_players() {
        let joined = VideoEncoder::HevcNvenc.args(8_000_000, 60).join(" ");
        assert!(joined.starts_with("-c:v hevc_nvenc "));
        assert!(joined.contains("-profile:v main"));
        assert!(joined.ends_with("-pix_fmt yuv420p -tag:v hvc1"));
    }

    #[test]
    fn every_encoder_belongs_to_one_codec_and_reads_back_by_name() {
        for codec in VideoCodec::ALL {
            for encoder in VideoEncoder::preference(codec) {
                assert_eq!(encoder.codec(), codec, "{}", encoder.name());
                assert_eq!(VideoEncoder::from_name(encoder.name()), Some(encoder));
            }
        }
        assert_eq!(VideoEncoder::from_name("libx264"), None);
    }

    #[test]
    fn the_best_and_the_software_fallback_are_per_codec() {
        let encoders = Encoders {
            working: vec![
                VideoEncoder::Nvenc,
                VideoEncoder::MediaFoundation,
                VideoEncoder::OpenH264,
                VideoEncoder::HevcNvenc,
            ],
        };
        assert_eq!(
            encoders.best(VideoCodec::H264).unwrap(),
            VideoEncoder::Nvenc
        );
        assert_eq!(
            encoders.software(VideoCodec::H264).unwrap(),
            VideoEncoder::MediaFoundation
        );
        assert_eq!(
            encoders.best(VideoCodec::Hevc).unwrap(),
            VideoEncoder::HevcNvenc
        );
        assert!(matches!(
            encoders.software(VideoCodec::Hevc),
            Err(MediaError::NoEncoder)
        ));
        assert!(matches!(
            Encoders { working: vec![] }.best(VideoCodec::H264),
            Err(MediaError::NoEncoder)
        ));
    }
}
