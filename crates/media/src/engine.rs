//! What the editor asks of ffmpeg, behind a trait so the app's editor can
//! be tested without it, and the bundled build found on first use.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use crate::PcmStream;
use crate::ffmpeg::{
    Ffmpeg, FrameSize, FrameStream, MediaError, Monitor, ProxySettings, RenderPlan, Waveform,
};

/// The media work the editor needs: proxies, waveforms and preview.
pub trait MediaEngine: Send + Sync {
    /// A 540-line proxy of a video file (ADR-0007).
    fn build_proxy(
        &self,
        source: &Path,
        destination: &Path,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError>;

    /// A 540-line JPEG of an image file.
    fn build_still_proxy(&self, source: &Path, destination: &Path) -> Result<(), MediaError>;

    /// Peaks of an audio file's first stream.
    fn waveform(&self, path: &Path, peaks_per_second: u32) -> Result<Waveform, MediaError>;

    /// Frames of `plan` from `from` on, as they decode.
    fn preview(
        &self,
        plan: &RenderPlan,
        from: Duration,
        size: FrameSize,
        fps: (u32, u32),
    ) -> Result<FrameStream, MediaError>;

    /// The audio mix of `plan` from `from` on, as it decodes.
    fn preview_audio(&self, plan: &RenderPlan, from: Duration) -> Result<PcmStream, MediaError>;
}

/// The bundled ffmpeg, located the first time it is needed. A failed
/// lookup is tried again next time (the user may install it meanwhile).
#[derive(Debug, Default)]
pub struct BundledFfmpeg {
    found: OnceLock<Ffmpeg>,
}

impl BundledFfmpeg {
    pub fn new() -> BundledFfmpeg {
        BundledFfmpeg::default()
    }

    fn ffmpeg(&self) -> Result<&Ffmpeg, MediaError> {
        if let Some(found) = self.found.get() {
            return Ok(found);
        }
        let located = Ffmpeg::locate()?;
        Ok(self.found.get_or_init(|| located))
    }
}

impl MediaEngine for BundledFfmpeg {
    fn build_proxy(
        &self,
        source: &Path,
        destination: &Path,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError> {
        self.ffmpeg()?
            .build_proxy(source, destination, ProxySettings::DEFAULT, monitor)
    }

    fn build_still_proxy(&self, source: &Path, destination: &Path) -> Result<(), MediaError> {
        self.ffmpeg()?
            .build_still_proxy(source, destination, ProxySettings::DEFAULT.height)
    }

    fn waveform(&self, path: &Path, peaks_per_second: u32) -> Result<Waveform, MediaError> {
        self.ffmpeg()?.waveform(path, peaks_per_second)
    }

    fn preview(
        &self,
        plan: &RenderPlan,
        from: Duration,
        size: FrameSize,
        fps: (u32, u32),
    ) -> Result<FrameStream, MediaError> {
        self.ffmpeg()?.preview(plan, from, size, fps)
    }

    fn preview_audio(&self, plan: &RenderPlan, from: Duration) -> Result<PcmStream, MediaError> {
        self.ffmpeg()?.preview_audio(plan, from)
    }
}
