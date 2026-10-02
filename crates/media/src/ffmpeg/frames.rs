//! Decoded frames for thumbnails and preview: ffmpeg scales and converts
//! to BGRA (the order GPUI's images use) and writes raw frames to a pipe.

use std::io::Read as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::captions::CaptionFiles;
use super::{Ffmpeg, MediaError, path_arg, process, seconds};

/// A frame size in pixels. ffmpeg needs even sizes for 4:2:0 video, so
/// preview sizes stay even too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

impl FrameSize {
    pub const fn new(width: u32, height: u32) -> FrameSize {
        FrameSize { width, height }
    }

    /// Bytes of one BGRA frame.
    pub fn bgra_len(self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// One decoded frame, BGRA, rows top to bottom with no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub size: FrameSize,
    /// Where the frame sits in the source or timeline.
    pub at: Duration,
    pub bgra: Vec<u8>,
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("size", &self.size)
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

impl Ffmpeg {
    /// The frame shown at `at`, scaled to `size` (thumbnails, the frame
    /// under the playhead while scrubbing).
    pub fn frame_at(
        &self,
        path: &Path,
        at: Duration,
        size: FrameSize,
    ) -> Result<VideoFrame, MediaError> {
        let mut command = self.ffmpeg();
        command
            .args(["-ss", &seconds(at), "-i"])
            .arg(path_arg(path))
            .args(["-frames:v", "1", "-an", "-vf", &scale_to_bgra(size, None)])
            .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"]);
        let bgra = process::output(command)?;
        if bgra.len() != size.bgra_len() {
            return Err(MediaError::Parse(format!(
                "expected one {}x{} frame, got {} bytes",
                size.width,
                size.height,
                bgra.len()
            )));
        }
        Ok(VideoFrame { size, at, bgra })
    }

    /// Frames of one file from `from` on, at `fps`, scaled to `size`.
    pub fn frames(
        &self,
        path: &Path,
        from: Duration,
        size: FrameSize,
        fps: (u32, u32),
    ) -> Result<FrameStream, MediaError> {
        let mut command = self.ffmpeg();
        command
            .args(["-ss", &seconds(from), "-i"])
            .arg(path_arg(path))
            .args(["-an", "-vf", &scale_to_bgra(size, Some(fps))])
            .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"]);
        FrameStream::spawn(command, size, fps, from)
    }
}

/// Scale (keeping the picture's aspect, padding the rest black) and convert
/// to BGRA, optionally resampling to a steady frame rate.
pub(super) fn scale_to_bgra(size: FrameSize, fps: Option<(u32, u32)>) -> String {
    let FrameSize { width, height } = size;
    let rate = fps
        .map(|(numerator, denominator)| format!("fps={numerator}/{denominator},"))
        .unwrap_or_default();
    format!(
        "{rate}scale={width}:{height}:force_original_aspect_ratio=decrease:flags=bilinear,\
         pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,format=bgra"
    )
}

/// Frames as ffmpeg decodes them, a few ahead of the reader. ffmpeg blocks
/// on the full pipe when the reader falls behind, so a paused preview costs
/// no decoding. Dropping the stream kills the child.
pub struct FrameStream {
    /// `None` for frames made in memory.
    child: Option<Child>,
    frames: mpsc::Receiver<VideoFrame>,
    stderr: Option<thread::JoinHandle<String>>,
    /// Files the child reads while it runs (a caption script).
    _files: Option<CaptionFiles>,
}

/// Frames decoded ahead of the reader.
const AHEAD: usize = 3;

/// What [`FrameStream::poll_frame`] found.
#[derive(Debug)]
pub enum FramePoll {
    Ready(VideoFrame),
    /// Nothing decoded yet.
    Waiting,
    /// No more frames: the stream ended or ffmpeg failed (see `finish`).
    Ended,
}

impl FrameStream {
    pub(super) fn spawn(
        mut command: Command,
        size: FrameSize,
        fps: (u32, u32),
        from: Duration,
    ) -> Result<FrameStream, MediaError> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let program = process::program_name(&command);
        let mut child = command
            .spawn()
            .map_err(|source| MediaError::Spawn { program, source })?;
        let stderr = process::collect_stderr(&mut child);
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let (sender, frames) = mpsc::sync_channel(AHEAD);
        let frame_len = size.bgra_len();
        let (numerator, denominator) = (u64::from(fps.0.max(1)), u64::from(fps.1.max(1)));
        thread::spawn(move || {
            let mut index = 0u64;
            loop {
                let mut bgra = vec![0; frame_len];
                if stdout.read_exact(&mut bgra).is_err() {
                    break;
                }
                let at =
                    from + Duration::from_nanos(index * denominator * 1_000_000_000 / numerator);
                if sender.send(VideoFrame { size, at, bgra }).is_err() {
                    break;
                }
                index += 1;
            }
        });
        Ok(FrameStream {
            child: Some(child),
            frames,
            stderr: Some(stderr),
            _files: None,
        })
    }

    /// Keeps `files` until the stream is dropped, after its child.
    pub(super) fn keeping(mut self, files: Option<CaptionFiles>) -> FrameStream {
        self._files = files;
        self
    }

    /// A stream of frames already decoded (tests, other decoders).
    pub fn from_frames(frames: Vec<VideoFrame>) -> FrameStream {
        let (sender, receiver) = mpsc::sync_channel(frames.len().max(1));
        for frame in frames {
            let _ = sender.send(frame);
        }
        FrameStream {
            child: None,
            frames: receiver,
            stderr: None,
            _files: None,
        }
    }

    /// The next frame, waiting for ffmpeg; `None` at the end.
    pub fn next_frame(&self) -> Option<VideoFrame> {
        self.frames.recv().ok()
    }

    /// The next frame if one is decoded already.
    pub fn try_next_frame(&self) -> Option<VideoFrame> {
        self.frames.try_recv().ok()
    }

    /// The next frame if one is decoded already, telling a stream that is
    /// over (ffmpeg exited) from one that is only behind.
    pub fn poll_frame(&self) -> FramePoll {
        match self.frames.try_recv() {
            Ok(frame) => FramePoll::Ready(frame),
            Err(mpsc::TryRecvError::Empty) => FramePoll::Waiting,
            Err(mpsc::TryRecvError::Disconnected) => FramePoll::Ended,
        }
    }

    /// Waits for ffmpeg to exit after the last frame was read; reports a
    /// failed run (a broken file, a missing input) with ffmpeg's message.
    pub fn finish(mut self) -> Result<(), MediaError> {
        while self.frames.recv().is_ok() {}
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        let status = child.wait()?;
        let log = self
            .stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(MediaError::Failed {
                program: "ffmpeg".into(),
                status: status.to_string(),
                log,
            })
        }
    }
}

impl Iterator for FrameStream {
    type Item = VideoFrame;

    fn next(&mut self) -> Option<VideoFrame> {
        self.next_frame()
    }
}

impl Drop for FrameStream {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_keeps_aspect_and_pads() {
        assert_eq!(
            scale_to_bgra(FrameSize::new(960, 540), Some((30, 1))),
            "fps=30/1,scale=960:540:force_original_aspect_ratio=decrease:flags=bilinear,\
             pad=960:540:(ow-iw)/2:(oh-ih)/2,setsar=1,format=bgra"
        );
        assert!(scale_to_bgra(FrameSize::new(2, 2), None).starts_with("scale=2:2"));
    }

    #[test]
    fn frames_made_in_memory_stream_in_order_then_end() {
        let frame = |ms| VideoFrame {
            size: FrameSize::new(2, 2),
            at: Duration::from_millis(ms),
            bgra: vec![0; 16],
        };
        let stream = FrameStream::from_frames(vec![frame(0), frame(33)]);
        assert_eq!(stream.try_next_frame().map(|f| f.at), Some(Duration::ZERO));
        assert_eq!(
            stream.next_frame().map(|f| f.at),
            Some(Duration::from_millis(33))
        );
        assert!(stream.next_frame().is_none());
        assert!(stream.finish().is_ok());
    }

    #[test]
    fn bgra_frames_are_four_bytes_a_pixel() {
        assert_eq!(FrameSize::new(1920, 1080).bgra_len(), 8_294_400);
    }
}
