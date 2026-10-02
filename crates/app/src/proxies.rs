//! Preview proxies (ADR-0007): small copies of a project's media that the
//! editor's preview decodes instead of the originals. A clip gets a
//! 540-line MJPEG copy, a still image a 540-line JPEG, and the narration a
//! file of waveform peaks for its track. Each is named after its source
//! file, which never changes once written (a new image or clip is a new
//! file), so a proxy that exists is current.
//!
//! One job per project builds the missing ones, one file after another.
//! A file that fails does not stop the others; the job fails at the end
//! naming each, and keeps the list in its checkpoint for the editor's
//! banner. Running the job again builds only what is still missing.

use std::sync::Arc;

use bardo_domain::{
    JobFailure, JobFailureKind, Progress, ProjectFiles, VideoProjectId, VideoSource,
};
use bardo_media::MediaEngine;
use bardo_media::ffmpeg::{MediaError, Monitor};
use serde::{Deserialize, Serialize};

use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, parse, to_json};

/// Waveform peaks kept per second of narration: enough to draw words.
pub(crate) const PEAKS_PER_SECOND: u32 = 50;

/// What a source file gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProxyKind {
    /// A video clip: an MJPEG copy.
    Clip,
    /// A still image: a JPEG copy.
    Still,
    /// An audio file: waveform peaks.
    Audio,
}

impl ProxyKind {
    pub(crate) const ALL: [ProxyKind; 3] = [ProxyKind::Clip, ProxyKind::Still, ProxyKind::Audio];

    pub(crate) fn of(source: &VideoSource) -> Option<ProxyKind> {
        match source {
            VideoSource::Clip { .. } => Some(ProxyKind::Clip),
            VideoSource::Still(_) => Some(ProxyKind::Still),
            VideoSource::Missing => None,
        }
    }
}

/// The name of what `kind` makes of the project file `file`.
pub(crate) fn proxy_name(file: &str, kind: ProxyKind) -> String {
    match kind {
        ProxyKind::Clip => format!("proxy-{file}.mkv"),
        ProxyKind::Still => format!("proxy-{file}.jpg"),
        ProxyKind::Audio => format!("peaks-{file}.json"),
    }
}

/// Removes a project file and anything made from it.
pub(crate) fn remove_media(files: &dyn ProjectFiles, project: VideoProjectId, file: &str) {
    let _ = files.remove(project, file);
    for kind in ProxyKind::ALL {
        let _ = files.remove(project, &proxy_name(file, kind));
    }
}

/// Peaks as the proxy file stores them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Peaks {
    pub(crate) peaks_per_second: u32,
    pub(crate) peaks: Vec<f32>,
}

/// The files a proxies job works on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProxiesPayload {
    pub(crate) project: String,
    pub(crate) files: Vec<ProxyOrder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProxyOrder {
    pub(crate) file: String,
    pub(crate) kind: ProxyKind,
}

/// What a finished run left out, saved as the job's checkpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProxiesCheckpoint {
    pub(crate) failed: Vec<ProxyFailure>,
}

/// One file the run could not make a proxy of, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProxyFailure {
    pub(crate) file: String,
    pub(crate) detail: String,
}

pub(crate) struct ProxyHandler {
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
}

/// One file's share of the job's progress, and its cancel.
struct FileMonitor<'a> {
    cx: &'a JobContext,
    done: usize,
    total: usize,
}

impl Monitor for FileMonitor<'_> {
    fn should_stop(&self) -> bool {
        self.cx.should_stop()
    }

    fn progress(&self, fraction: f32) {
        let share = (self.done as f32 + fraction.clamp(0.0, 1.0)) / self.total.max(1) as f32;
        self.cx
            .report_progress(Progress::from_permille((share * 1000.0) as u16));
    }
}

impl ProxyHandler {
    fn build(
        &self,
        project: VideoProjectId,
        order: &ProxyOrder,
        monitor: &FileMonitor<'_>,
    ) -> Result<(), MediaError> {
        let source = self.files.path(project, &order.file);
        let output = proxy_name(&order.file, order.kind);
        let destination = self.files.path(project, &output);
        match order.kind {
            ProxyKind::Clip => self.media.build_proxy(&source, &destination, monitor),
            ProxyKind::Still => self.media.build_still_proxy(&source, &destination),
            ProxyKind::Audio => {
                let waveform = self.media.waveform(&source, PEAKS_PER_SECOND)?;
                let peaks = Peaks {
                    peaks_per_second: waveform.peaks_per_second,
                    peaks: waveform.peaks,
                };
                let json = serde_json::to_vec(&peaks)
                    .map_err(|error| MediaError::Parse(error.to_string()))?;
                self.files
                    .write(project, &output, &json)
                    .map_err(|error| MediaError::Io(error.source))
            }
        }
    }
}

impl JobHandler for ProxyHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: ProxiesPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let total = payload.files.len();
        let mut failed = Vec::new();
        for (done, order) in payload.files.iter().enumerate() {
            if cx.should_stop() {
                return Ok(());
            }
            if self
                .files
                .exists(project, &proxy_name(&order.file, order.kind))
            {
                continue;
            }
            if !self.files.exists(project, &order.file) {
                failed.push(ProxyFailure {
                    file: order.file.clone(),
                    detail: "the file is missing from the project folder".into(),
                });
                continue;
            }
            let monitor = FileMonitor { cx, done, total };
            match self.build(project, order, &monitor) {
                Ok(()) => {}
                Err(MediaError::Cancelled) => return Ok(()),
                Err(error) => failed.push(ProxyFailure {
                    file: order.file.clone(),
                    detail: error.to_string(),
                }),
            }
            cx.report_progress(Progress::of(done as u64 + 1, total as u64));
        }
        let summary = failed
            .iter()
            .map(|failure| format!("{}: {}", failure.file, failure.detail))
            .collect::<Vec<_>>()
            .join(" | ");
        let checkpoint = to_json(&ProxiesCheckpoint { failed });
        cx.save_checkpoint(checkpoint, Progress::of(total as u64, total as u64))
            .map_err(|error| JobFailure::unexpected(error.to_string()))?;
        if summary.is_empty() {
            Ok(())
        } else {
            Err(JobFailure::new(JobFailureKind::Media, summary))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_names_its_own_file() {
        assert_eq!(
            proxy_name("clip-1.mp4", ProxyKind::Clip),
            "proxy-clip-1.mp4.mkv"
        );
        assert_eq!(
            proxy_name("scene-1.png", ProxyKind::Still),
            "proxy-scene-1.png.jpg"
        );
        assert_eq!(
            proxy_name("narration-1.mp3", ProxyKind::Audio),
            "peaks-narration-1.mp3.json"
        );
    }

    #[test]
    fn removing_a_file_takes_its_proxies_along() {
        let files = bardo_storage::MemoryProjectFiles::default();
        let project = VideoProjectId::new();
        for name in ["scene-1.png", "proxy-scene-1.png.jpg", "scene-2.png"] {
            files.write(project, name, b"x").unwrap();
        }
        remove_media(&files, project, "scene-1.png");
        assert!(!files.exists(project, "scene-1.png"));
        assert!(!files.exists(project, "proxy-scene-1.png.jpg"));
        assert!(files.exists(project, "scene-2.png"));
    }
}
