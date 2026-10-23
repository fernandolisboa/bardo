//! Encoding control videos (#191): a take's frames, as recorded or drawn by
//! Bardo, with the take's stretch of the narration as their sound, into an
//! H.264 MP4 with AAC that every take model reads.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bardo_domain::{CONTROL_AUDIO_BITRATE, RgbFrame};

use super::process::{self, Span};
use super::vision::FfvWriter;
use super::{
    Ffmpeg, MediaError, Monitor, VideoEncoder, partial_path, path_arg, remove_partial, seconds,
};

/// The voice a control video carries: a stretch of the narration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCut {
    pub path: PathBuf,
    pub from: Duration,
    pub length: Duration,
}

/// How a control video is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlOutput {
    pub size: (u32, u32),
    /// Frames a second, in thousandths.
    pub fps_milli: u32,
    pub encoder: VideoEncoder,
    /// The video's bits a second.
    pub bitrate: u32,
    /// Frames between keyframes.
    pub keyframe_interval: u32,
}

/// RGBA frames going into a control video.
pub trait FrameSink: Send {
    /// One frame: four bytes a pixel, rows top to bottom.
    fn write(&mut self, frame: &[u8]) -> Result<(), MediaError>;
    /// Finishes the file; until then it is not at its path.
    fn finish(self: Box<Self>) -> Result<(), MediaError>;
}

impl FrameSink for FfvWriter {
    fn write(&mut self, frame: &[u8]) -> Result<(), MediaError> {
        self.write_frame(frame)
    }

    fn finish(self: Box<Self>) -> Result<(), MediaError> {
        (*self).finish_file()
    }
}

fn fps_arg(fps_milli: u32) -> String {
    format!("{fps_milli}/1000")
}

/// The output options every control video shares: the encoder, the voice
/// as AAC, the length and MP4 with its index first.
fn output_args(output: &ControlOutput, voice: &VoiceCut) -> Vec<String> {
    let mut args = vec!["-map".into(), "0:v:0".into(), "-map".into(), "1:a:0".into()];
    let mut video = output
        .encoder
        .args(output.bitrate, output.keyframe_interval.max(1));
    if output.encoder == VideoEncoder::OpenH264 {
        // OpenH264 aims at the bitrate only loosely and, on a busy
        // picture, can overshoot several times: the take models refuse a
        // file over their limit, so here it holds a frame now and then
        // instead.
        if let Some(skip) = video.iter().position(|arg| arg == "-allow_skip_frames") {
            video[skip + 1] = "1".into();
        }
        video.extend(["-rc_mode".into(), "bitrate".into()]);
    }
    args.extend(video);
    args.extend(
        [
            "-c:a",
            "aac",
            "-b:a",
            &CONTROL_AUDIO_BITRATE.to_string(),
            "-t",
            &seconds(voice.length),
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ]
        .map(String::from),
    );
    args
}

impl Ffmpeg {
    /// Starts a control video at `destination`: RGBA frames of
    /// `output.size` come in through the sink, the voice is read from its
    /// file. The file is there once the sink finishes.
    pub fn control_writer(
        &self,
        destination: &Path,
        output: &ControlOutput,
        voice: &VoiceCut,
    ) -> Result<Box<dyn FrameSink>, MediaError> {
        let (width, height) = output.size;
        let mut command = self.ffmpeg();
        command
            .args(["-y", "-f", "rawvideo", "-pix_fmt", "rgba"])
            .args([
                "-s",
                &format!("{width}x{height}"),
                "-r",
                &fps_arg(output.fps_milli),
            ])
            .args(["-i", "pipe:0"])
            .args(["-ss", &seconds(voice.from), "-t", &seconds(voice.length)])
            .arg("-i")
            .arg(path_arg(&voice.path))
            .args(output_args(output, voice))
            .arg(path_arg(&partial_path(destination)));
        Ok(Box::new(FfvWriter::start(
            command,
            width as usize * height as usize * 4,
            destination,
        )?))
    }

    /// Writes the stretch of `source` the voice covers, scaled to
    /// `output.size` at the recording's own frame rate, with the voice in
    /// place of its sound, as a control video at `destination`.
    pub fn cut_recording(
        &self,
        source: &Path,
        output: &ControlOutput,
        voice: &VoiceCut,
        destination: &Path,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError> {
        let (width, height) = output.size;
        let partial = partial_path(destination);
        let mut command = self.ffmpeg();
        command
            .arg("-y")
            .args(["-ss", &seconds(voice.from), "-t", &seconds(voice.length)])
            .arg("-i")
            .arg(path_arg(source))
            .args(["-ss", &seconds(voice.from), "-t", &seconds(voice.length)])
            .arg("-i")
            .arg(path_arg(&voice.path))
            .args([
                "-vf",
                &format!("scale={width}:{height}:flags=bicubic,setsar=1"),
                "-sn",
                "-dn",
            ])
            .args(output_args(output, voice))
            .arg(path_arg(&partial));
        match process::run(command, monitor, Span::whole(voice.length)) {
            Ok(_) => {
                std::fs::rename(&partial, destination)?;
                Ok(())
            }
            Err(error) => {
                remove_partial(&partial);
                Err(error)
            }
        }
    }

    /// The picture at `path` scaled to cover `width` × `height` and
    /// centred, as RGB: a take's set image.
    pub fn cover_picture(
        &self,
        path: &Path,
        (width, height): (u32, u32),
    ) -> Result<RgbFrame, MediaError> {
        let mut command = self.ffmpeg();
        command.arg("-i").arg(path_arg(path)).args([
            "-frames:v",
            "1",
            "-vf",
            &format!(
                "scale={width}:{height}:force_original_aspect_ratio=increase:flags=bicubic,\
                 crop={width}:{height},setsar=1,format=rgb24"
            ),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ]);
        let rgb = process::output(command)?;
        let expected = width as usize * height as usize * 3;
        if rgb.len() != expected {
            return Err(MediaError::Parse(format!(
                "a picture of {} bytes, expected {expected}",
                rgb.len()
            )));
        }
        Ok(RgbFrame { width, height, rgb })
    }
}
