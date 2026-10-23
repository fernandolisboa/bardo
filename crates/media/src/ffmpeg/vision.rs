//! Frames for the take analysis (#190): a take decoded to RGB at a steady
//! frame rate for the vision models, and their depth maps and person
//! masks written to and read from lossless FFV1 in Matroska, which the
//! LGPL build writes.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

use bardo_domain::RgbFrame;

use super::{Ffmpeg, MediaError, partial_path, path_arg, seconds};

/// The longest side frames are decoded at for the analysis: a 4K
/// recording is analyzed at 1080p, which the models' crops never miss.
pub const ANALYSIS_MAX_SIDE: u32 = 1920;

/// A take's frames, one after another.
pub trait FrameSource: Send {
    /// The next frame, or `None` after the last.
    fn next_frame(&mut self) -> Result<Option<RgbFrame>, MediaError>;
}

/// Lossless gray frames going to a file.
pub trait GraySink: Send {
    /// One frame: a byte a pixel, or two (little-endian) for 16 bits.
    fn write(&mut self, frame: &[u8]) -> Result<(), MediaError>;
    /// Finishes the file; until then it is not at its path.
    fn finish(self: Box<Self>) -> Result<(), MediaError>;
}

/// Gray frames read back from a file.
pub trait GraySource: Send {
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>, MediaError>;
}

/// How many bits a gray pixel has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrayDepth {
    Eight,
    Sixteen,
}

impl GrayDepth {
    fn pix_fmt(self) -> &'static str {
        match self {
            GrayDepth::Eight => "gray",
            GrayDepth::Sixteen => "gray16le",
        }
    }

    fn bytes(self) -> usize {
        match self {
            GrayDepth::Eight => 1,
            GrayDepth::Sixteen => 2,
        }
    }
}

/// The size frames of a `width` × `height` recording are decoded at: no
/// side over [`ANALYSIS_MAX_SIDE`], even, the aspect kept.
pub fn analysis_size(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height).max(1);
    if long <= ANALYSIS_MAX_SIDE {
        return (width, height);
    }
    let scale = f64::from(ANALYSIS_MAX_SIDE) / f64::from(long);
    let even = |side: u32| (((f64::from(side) * scale) / 2.0).round() as u32 * 2).max(2);
    (even(width), even(height))
}

fn fps_arg(fps_milli: u32) -> String {
    format!("{fps_milli}/1000")
}

/// Keeps reading stderr so ffmpeg never blocks on it; its tail is the
/// error message.
fn drain_stderr(child: &mut Child) -> Option<thread::JoinHandle<String>> {
    let mut stderr = child.stderr.take()?;
    Some(thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        let tail = text.len().saturating_sub(4096);
        text[text.floor_char_boundary(tail)..].to_owned()
    }))
}

fn spawn(mut command: Command) -> Result<Child, MediaError> {
    let program = command.get_program().to_string_lossy().into_owned();
    command
        .spawn()
        .map_err(|source| MediaError::Spawn { program, source })
}

fn failed(status: String, log: String) -> MediaError {
    MediaError::Failed {
        program: "ffmpeg".to_owned(),
        status,
        log,
    }
}

/// Fills `buffer` from a pipe: `false` at a clean end before any byte.
fn read_exact_or_end(stdout: &mut ChildStdout, buffer: &mut [u8]) -> Result<bool, MediaError> {
    let mut filled = 0;
    while filled < buffer.len() {
        let n = stdout.read(&mut buffer[filled..])?;
        if n == 0 {
            if filled == 0 {
                return Ok(false);
            }
            return Err(MediaError::Parse(
                "ffmpeg ended in the middle of a frame".to_owned(),
            ));
        }
        filled += n;
    }
    Ok(true)
}

/// Frames read from a running ffmpeg; dropping it stops ffmpeg.
struct PipeReader {
    child: Child,
    stdout: ChildStdout,
    stderr: Option<thread::JoinHandle<String>>,
    frame_len: usize,
    done: bool,
}

impl PipeReader {
    fn start(mut command: Command, frame_len: usize) -> Result<PipeReader, MediaError> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn(command)?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = drain_stderr(&mut child);
        Ok(PipeReader {
            child,
            stdout,
            stderr,
            frame_len,
            done: false,
        })
    }

    fn next(&mut self) -> Result<Option<Vec<u8>>, MediaError> {
        if self.done {
            return Ok(None);
        }
        let mut frame = vec![0; self.frame_len];
        if read_exact_or_end(&mut self.stdout, &mut frame)? {
            return Ok(Some(frame));
        }
        // The end: ffmpeg must have ended well, or the frames are short.
        self.done = true;
        let status = self.child.wait()?;
        let log = self
            .stderr
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if !status.success() {
            return Err(failed(status.to_string(), log));
        }
        Ok(None)
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct RgbReader {
    pipe: PipeReader,
    width: u32,
    height: u32,
}

impl FrameSource for RgbReader {
    fn next_frame(&mut self) -> Result<Option<RgbFrame>, MediaError> {
        Ok(self.pipe.next()?.map(|rgb| RgbFrame {
            width: self.width,
            height: self.height,
            rgb,
        }))
    }
}

struct GrayReader(PipeReader);

impl GraySource for GrayReader {
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>, MediaError> {
        self.0.next()
    }
}

/// An ffmpeg encoding raw gray frames from its stdin into `partial`,
/// renamed to `destination` when it finishes well.
struct FfvWriter {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr: Option<thread::JoinHandle<String>>,
    frame_len: usize,
    partial: PathBuf,
    destination: PathBuf,
}

impl GraySink for FfvWriter {
    fn write(&mut self, frame: &[u8]) -> Result<(), MediaError> {
        if frame.len() != self.frame_len {
            return Err(MediaError::Parse(format!(
                "a gray frame of {} bytes, expected {}",
                frame.len(),
                self.frame_len
            )));
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| MediaError::Parse("the writer is closed".to_owned()))?;
        stdin.write_all(frame)?;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), MediaError> {
        drop(self.stdin.take());
        let status = self.child.wait()?;
        let log = self
            .stderr
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if !status.success() {
            let _ = std::fs::remove_file(&self.partial);
            return Err(failed(status.to_string(), log));
        }
        std::fs::rename(&self.partial, &self.destination)?;
        Ok(())
    }
}

impl Drop for FfvWriter {
    fn drop(&mut self) {
        if self.stdin.is_some() {
            // Dropped before `finish`: nothing is kept.
            drop(self.stdin.take());
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.partial);
        }
    }
}

impl Ffmpeg {
    /// The frames of `path` from `from` for `length`, at a steady
    /// `fps_milli` thousandths a second, as RGB at
    /// [`analysis_size`] of `width` × `height`.
    pub fn rgb_frames(
        &self,
        path: &Path,
        from: Duration,
        length: Duration,
        fps_milli: u32,
        (width, height): (u32, u32),
    ) -> Result<Box<dyn FrameSource>, MediaError> {
        let (w, h) = analysis_size(width, height);
        let mut command = self.ffmpeg();
        command
            .args(["-ss", &seconds(from), "-t", &seconds(length), "-i"])
            .arg(path_arg(path))
            .args([
                "-an",
                "-sn",
                "-vf",
                &format!(
                    "fps={},scale={w}:{h}:flags=bicubic,setsar=1,format=rgb24",
                    fps_arg(fps_milli)
                ),
            ])
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]);
        Ok(Box::new(RgbReader {
            pipe: PipeReader::start(command, w as usize * h as usize * 3)?,
            width: w,
            height: h,
        }))
    }

    /// Writes gray frames losslessly (FFV1 in Matroska) to `destination`.
    pub fn gray_writer(
        &self,
        destination: &Path,
        (width, height): (u32, u32),
        fps_milli: u32,
        depth: GrayDepth,
    ) -> Result<Box<dyn GraySink>, MediaError> {
        let partial = partial_path(destination);
        let mut command = self.ffmpeg();
        command
            .args(["-y", "-f", "rawvideo", "-pix_fmt", depth.pix_fmt()])
            .args([
                "-s",
                &format!("{width}x{height}"),
                "-r",
                &fps_arg(fps_milli),
            ])
            .args(["-i", "pipe:0", "-c:v", "ffv1", "-level", "3", "-g", "1"])
            .args(["-pix_fmt", depth.pix_fmt(), "-f", "matroska"])
            .arg(path_arg(&partial));
        // `self.ffmpeg()` closes stdin; this run reads it.
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = spawn(command)?;
        let stdin = child.stdin.take();
        let stderr = drain_stderr(&mut child);
        Ok(Box::new(FfvWriter {
            child,
            stdin,
            stderr,
            frame_len: width as usize * height as usize * depth.bytes(),
            partial,
            destination: destination.to_owned(),
        }))
    }

    /// The gray frames of a file [`Ffmpeg::gray_writer`] wrote.
    pub fn gray_frames(
        &self,
        path: &Path,
        (width, height): (u32, u32),
        depth: GrayDepth,
    ) -> Result<Box<dyn GraySource>, MediaError> {
        let mut command = self.ffmpeg();
        command.arg("-i").arg(path_arg(path)).args([
            "-f",
            "rawvideo",
            "-pix_fmt",
            depth.pix_fmt(),
            "pipe:1",
        ]);
        Ok(Box::new(GrayReader(PipeReader::start(
            command,
            width as usize * height as usize * depth.bytes(),
        )?)))
    }
}

/// A frame source over frames already in memory, for tests.
pub struct MemoryFrames(pub std::collections::VecDeque<RgbFrame>);

impl FrameSource for MemoryFrames {
    fn next_frame(&mut self) -> Result<Option<RgbFrame>, MediaError> {
        Ok(self.0.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_frames_keep_up_to_1080p_and_scale_larger_ones_down() {
        assert_eq!(analysis_size(1920, 1080), (1920, 1080));
        assert_eq!(analysis_size(1080, 1920), (1080, 1920));
        assert_eq!(analysis_size(3840, 2160), (1920, 1080));
        assert_eq!(analysis_size(2160, 3840), (1080, 1920));
        assert_eq!(analysis_size(2560, 1440), (1920, 1080));
    }
}
