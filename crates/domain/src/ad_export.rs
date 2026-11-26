//! The export for ads (#283): an ad's cut rendered in the shapes ad
//! managers ask for (4:5 and 1:1, and the editor's other shape), with the
//! texts an ad manager asks for beside the video. The cut stays in its
//! own frame: each shape composes the composed pictures again, shows the
//! other pictures through a window the user may move per clip, and lays
//! the captions out for itself.

use std::time::{Duration, SystemTime};

use crate::{CropPosition, FrameShape, Picture, RepositoryError, VideoProjectId};

/// The most characters of an ad's primary text: what ad managers show
/// before they cut it.
pub const AD_PRIMARY_MAX: usize = 125;

/// The most characters of an ad's headline.
pub const AD_HEADLINE_MAX: usize = 40;

/// The texts an ad manager asks for, written once for every shape: the
/// primary text above the video and the headline beside its button.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdTexts {
    primary: String,
    headline: String,
}

/// What keeps an ad's texts from going out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdTextProblem {
    PrimaryEmpty,
    PrimaryTooLong,
    HeadlineEmpty,
    HeadlineTooLong,
}

/// One line, its spaces collapsed.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl AdTexts {
    /// The texts, each on one line with its spaces collapsed. Kept as
    /// written even past its limit, so an edit is never lost: the export
    /// waits on [`AdTexts::problems`].
    pub fn new(primary: &str, headline: &str) -> Self {
        Self {
            primary: one_line(primary),
            headline: one_line(headline),
        }
    }

    pub fn primary(&self) -> &str {
        &self.primary
    }

    pub fn headline(&self) -> &str {
        &self.headline
    }

    /// What an ad manager would refuse or cut.
    pub fn problems(&self) -> Vec<AdTextProblem> {
        let mut problems = Vec::new();
        let primary = self.primary.chars().count();
        let headline = self.headline.chars().count();
        if primary == 0 {
            problems.push(AdTextProblem::PrimaryEmpty);
        } else if primary > AD_PRIMARY_MAX {
            problems.push(AdTextProblem::PrimaryTooLong);
        }
        if headline == 0 {
            problems.push(AdTextProblem::HeadlineEmpty);
        } else if headline > AD_HEADLINE_MAX {
            problems.push(AdTextProblem::HeadlineTooLong);
        }
        problems
    }
}

/// Which clip of the cut a window belongs to: its picture and where its
/// source starts, so a window stays with its clip when clips around it
/// move, and two pieces of one scene keep a window each.
pub fn ad_clip_key(picture: &Picture, source_start: Duration) -> String {
    let millis = source_start.as_millis();
    match picture {
        Picture::Scene(index) => format!("scene:{index}@{millis}"),
        Picture::Footage(file) => format!("footage:{file}@{millis}"),
    }
}

/// Where the user put a clip's window in one shape of the export; a clip
/// without one is centered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdWindow {
    pub shape: FrameShape,
    /// The clip, by [`ad_clip_key`].
    pub clip: String,
    pub position: CropPosition,
}

/// One shape of an ad's last export for ads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdExport {
    pub project: VideoProjectId,
    pub shape: FrameShape,
    /// What it was made from: the shape's plan, its composed pictures and
    /// the ad's texts. Another fingerprint now means it is out of date.
    pub made_from: String,
    /// The video's name in the shape's folder.
    pub file: String,
    pub size_bytes: u64,
    pub exported_at: SystemTime,
}

/// Persistence port for the windows and the files of an ad's export for
/// ads. Shared with job worker threads.
pub trait AdExportRepository: Send + Sync {
    /// Every window the user moved, in every shape.
    fn ad_windows(&self, project: VideoProjectId) -> Result<Vec<AdWindow>, RepositoryError>;

    /// Keeps `window` in place of the clip's in its shape.
    fn save_ad_window(
        &self,
        project: VideoProjectId,
        window: &AdWindow,
    ) -> Result<(), RepositoryError>;

    /// Puts the clip's window in `shape` back in the middle.
    fn drop_ad_window(
        &self,
        project: VideoProjectId,
        shape: FrameShape,
        clip: &str,
    ) -> Result<(), RepositoryError>;

    /// The last export of each shape.
    fn ad_exports(&self, project: VideoProjectId) -> Result<Vec<AdExport>, RepositoryError>;

    /// Records a shape's export in place of its last one.
    fn save_ad_export(&self, export: &AdExport) -> Result<(), RepositoryError>;
}

impl<T: AdExportRepository + ?Sized> AdExportRepository for std::sync::Arc<T> {
    fn ad_windows(&self, project: VideoProjectId) -> Result<Vec<AdWindow>, RepositoryError> {
        (**self).ad_windows(project)
    }

    fn save_ad_window(
        &self,
        project: VideoProjectId,
        window: &AdWindow,
    ) -> Result<(), RepositoryError> {
        (**self).save_ad_window(project, window)
    }

    fn drop_ad_window(
        &self,
        project: VideoProjectId,
        shape: FrameShape,
        clip: &str,
    ) -> Result<(), RepositoryError> {
        (**self).drop_ad_window(project, shape, clip)
    }

    fn ad_exports(&self, project: VideoProjectId) -> Result<Vec<AdExport>, RepositoryError> {
        (**self).ad_exports(project)
    }

    fn save_ad_export(&self, export: &AdExport) -> Result<(), RepositoryError> {
        (**self).save_ad_export(export)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_one_line_and_kept_past_their_limits() {
        let texts = AdTexts::new("  Brew   better\ncoffee ", "Try it");
        assert_eq!(texts.primary(), "Brew better coffee");
        assert!(texts.problems().is_empty());
        let long = AdTexts::new(&"a".repeat(126), &"b".repeat(41));
        assert_eq!(long.primary().len(), 126);
        assert_eq!(
            long.problems(),
            vec![
                AdTextProblem::PrimaryTooLong,
                AdTextProblem::HeadlineTooLong
            ]
        );
        // Characters, not bytes: 125 accented letters fit.
        assert!(
            AdTexts::new(&"é".repeat(125), &"ç".repeat(40))
                .problems()
                .is_empty()
        );
        assert_eq!(
            AdTexts::new(" ", "").problems(),
            vec![AdTextProblem::PrimaryEmpty, AdTextProblem::HeadlineEmpty]
        );
    }

    #[test]
    fn a_clip_is_known_by_its_picture_and_where_it_starts() {
        let second = Duration::from_millis(1_500);
        assert_eq!(ad_clip_key(&Picture::Scene(3), second), "scene:3@1500");
        assert_eq!(
            ad_clip_key(&Picture::Footage("a.mp4".into()), Duration::ZERO),
            "footage:a.mp4@0"
        );
        assert_ne!(
            ad_clip_key(&Picture::Scene(3), Duration::ZERO),
            ad_clip_key(&Picture::Scene(3), second)
        );
    }
}
