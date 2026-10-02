//! Proxies: low-resolution copies of the clips that preview decodes
//! instead of the originals, so scrubbing and playback stay light.

use std::path::Path;

use super::process::{self, Span};
use super::{Ffmpeg, MediaError, Monitor, VideoEncoder, partial_path, path_arg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxySettings {
    /// Height in pixels; the width follows the source's shape.
    pub height: u32,
    pub codec: ProxyCodec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyCodec {
    /// Every frame a keyframe: seeking lands on the exact frame with no
    /// decoding ahead, at the cost of larger files.
    Mjpeg,
    /// Smaller files; seeking decodes from the previous keyframe, so
    /// keyframes come every half second.
    H264(VideoEncoder),
}

impl ProxySettings {
    /// What the editor uses: 540 lines of intra-only MJPEG (ADR-0007).
    pub const DEFAULT: ProxySettings = ProxySettings {
        height: 540,
        codec: ProxyCodec::Mjpeg,
    };
}

impl Ffmpeg {
    /// Writes a proxy of `source` to `destination` (Matroska, whatever the
    /// extension), with the source's frame rate and audio.
    pub fn build_proxy(
        &self,
        source: &Path,
        destination: &Path,
        settings: ProxySettings,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError> {
        let info = self.probe(source)?;
        let keyframes = info
            .video
            .as_ref()
            .map(|video| (video.fps() / 2.0).round().max(1.0) as u32)
            .unwrap_or(15);
        let partial = partial_path(destination);
        let mut command = self.ffmpeg();
        command
            .arg("-y")
            .arg("-i")
            .arg(path_arg(source))
            .args(["-map", "0:v:0", "-map", "0:a:0?"])
            .args([
                "-vf",
                &format!("scale=-2:{}:flags=bilinear", settings.height),
            ]);
        match settings.codec {
            ProxyCodec::Mjpeg => {
                command.args(["-c:v", "mjpeg", "-q:v", "5", "-pix_fmt", "yuvj420p"]);
            }
            ProxyCodec::H264(encoder) => {
                command
                    .args(encoder.args(3_000_000, keyframes))
                    .args(["-bf", "0"]);
            }
        }
        command
            .args(["-c:a", "aac", "-b:a", "128k", "-f", "matroska"])
            .arg(path_arg(&partial));
        match process::run(command, monitor, Span::whole(info.duration)) {
            Ok(_) => {
                std::fs::rename(&partial, destination)?;
                Ok(())
            }
            Err(error) => {
                let _ = std::fs::remove_file(&partial);
                Err(error)
            }
        }
    }
}
