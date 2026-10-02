//! What the editor and the render ask of ffmpeg, behind a trait so the app
//! can be tested without it, and the bundled build found on first use.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use crate::PcmStream;
use crate::ffmpeg::{
    Encoders, Ffmpeg, FrameSize, FrameStream, Loudness, LoudnessTarget, MediaError, MediaInfo,
    Monitor, Output, ProxySettings, RenderPlan, Waveform,
};

/// The media work the editor and the render need: probing, proxies,
/// waveforms, preview, loudness and the final render.
pub trait MediaEngine: Send + Sync {
    /// What a media file holds: its length and streams. Fails on a file
    /// that is not media ffmpeg reads.
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError>;

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

    /// The video encoders that work on this machine. Tried once, then
    /// remembered.
    fn encoders(&self) -> Result<Encoders, MediaError>;

    /// The loudness of `plan`'s mix before normalization.
    fn measure_mix(&self, plan: &RenderPlan, monitor: &dyn Monitor)
    -> Result<Loudness, MediaError>;

    /// Renders `plan` to the MP4 at `destination`, normalized to
    /// `loudness` when given.
    fn render(
        &self,
        plan: &RenderPlan,
        output: &Output,
        loudness: Option<LoudnessTarget>,
        destination: &Path,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError>;

    /// The loudness of a file's sound.
    fn measure_loudness(&self, path: &Path, monitor: &dyn Monitor) -> Result<Loudness, MediaError>;
}

/// The bundled ffmpeg, located the first time it is needed. A failed
/// lookup is tried again next time (the user may install it meanwhile).
#[derive(Debug, Default)]
pub struct BundledFfmpeg {
    found: OnceLock<Ffmpeg>,
    encoders: OnceLock<Encoders>,
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
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        self.ffmpeg()?.probe(path)
    }

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

    fn encoders(&self) -> Result<Encoders, MediaError> {
        if let Some(found) = self.encoders.get() {
            return Ok(found.clone());
        }
        let detected = self.ffmpeg()?.detect_encoders();
        Ok(self.encoders.get_or_init(|| detected).clone())
    }

    fn measure_mix(
        &self,
        plan: &RenderPlan,
        monitor: &dyn Monitor,
    ) -> Result<Loudness, MediaError> {
        self.ffmpeg()?.measure_mix_loudness(plan, monitor)
    }

    fn render(
        &self,
        plan: &RenderPlan,
        output: &Output,
        loudness: Option<LoudnessTarget>,
        destination: &Path,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError> {
        self.ffmpeg()?
            .render(plan, output, loudness, destination, monitor)
    }

    fn measure_loudness(&self, path: &Path, monitor: &dyn Monitor) -> Result<Loudness, MediaError> {
        self.ffmpeg()?.measure_loudness(path, monitor)
    }
}
