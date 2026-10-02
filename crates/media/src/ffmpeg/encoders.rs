//! H.264 encoders: which ones this machine can use, best first. The build
//! lists hardware encoders whether or not the GPU and driver are there, so
//! each one is tried on a few blank frames before Bardo relies on it.

use std::process::Command;

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
}

impl VideoEncoder {
    /// Best first: dedicated GPU encoders, then the OS encoder, then software.
    pub const PREFERENCE: [VideoEncoder; 5] = [
        VideoEncoder::Nvenc,
        VideoEncoder::Amf,
        VideoEncoder::Qsv,
        VideoEncoder::MediaFoundation,
        VideoEncoder::OpenH264,
    ];

    /// ffmpeg's name for it.
    pub fn name(self) -> &'static str {
        match self {
            VideoEncoder::Nvenc => "h264_nvenc",
            VideoEncoder::Amf => "h264_amf",
            VideoEncoder::Qsv => "h264_qsv",
            VideoEncoder::MediaFoundation => "h264_mf",
            VideoEncoder::OpenH264 => "libopenh264",
        }
    }

    pub fn is_hardware(self) -> bool {
        matches!(
            self,
            VideoEncoder::Nvenc | VideoEncoder::Amf | VideoEncoder::Qsv
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
            VideoEncoder::Amf => &[
                "-quality",
                "quality",
                "-rc",
                "vbr_peak",
                "-profile:v",
                "high",
            ],
            VideoEncoder::Qsv => &["-preset", "slower", "-profile:v", "high"],
            VideoEncoder::MediaFoundation => &["-rate_control", "pc_vbr", "-scenario", "archive"],
            VideoEncoder::OpenH264 => &["-profile:v", "high", "-allow_skip_frames", "0"],
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
        args
    }
}

/// The encoders that worked when tried, in [`VideoEncoder::PREFERENCE`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoders {
    pub working: Vec<VideoEncoder>,
}

impl Encoders {
    /// The best working encoder.
    pub fn best(&self) -> Result<VideoEncoder, MediaError> {
        self.working.first().copied().ok_or(MediaError::NoEncoder)
    }

    /// The best working encoder that is not hardware, for the fallback.
    pub fn software(&self) -> Result<VideoEncoder, MediaError> {
        self.working
            .iter()
            .copied()
            .find(|encoder| !encoder.is_hardware())
            .ok_or(MediaError::NoEncoder)
    }
}

impl Ffmpeg {
    /// Tries every encoder on a few blank frames. Takes well under a second
    /// per encoder; the app runs it once per start and keeps the result.
    pub fn detect_encoders(&self) -> Encoders {
        let working = VideoEncoder::PREFERENCE
            .into_iter()
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
    fn software_fallback_skips_gpu_encoders() {
        let encoders = Encoders {
            working: vec![
                VideoEncoder::Nvenc,
                VideoEncoder::MediaFoundation,
                VideoEncoder::OpenH264,
            ],
        };
        assert_eq!(encoders.best().unwrap(), VideoEncoder::Nvenc);
        assert_eq!(encoders.software().unwrap(), VideoEncoder::MediaFoundation);
        assert!(matches!(
            Encoders { working: vec![] }.best(),
            Err(MediaError::NoEncoder)
        ));
    }
}
