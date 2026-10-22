//! Retention over the cut (#183, ADR-0022): the audience retention curve
//! of a project's YouTube post, as the last metrics sync read it through
//! the connected account (#79), read against the cut that made the video.
//! Its steepest drops (`bardo_domain::retention_drops`) are marked with
//! the scene shown and the sentence spoken where they happen.
//!
//! The curve's times are shares of the published file, so the drops are
//! placed on the cut only while its timing is the one rendered for the
//! post (`bardo_domain::cut_timing`, kept per rendered file in a
//! `RenderedCut`). A post rendered before the timing was recorded lines
//! up while the cut renders the same plan as then. Otherwise the drops
//! keep their times in the published video and the screens say the cut
//! changed.
//!
//! Only YouTube reports a retention curve: TikTok none, Instagram the
//! average watch time, X and Kick nothing. Drops are worked out when shown
//! and never stored.

use std::time::{Duration, SystemTime};

use bardo_domain::{
    Network, PublicationId, RenderedCut, RepositoryError, RetentionCurve, RetentionDrop, Sentence,
    VideoProjectId, cut_timing, place_drops, retention_drops, sentences, time_at,
};

use crate::{Bardo, EditorView};

/// A project's YouTube post with its retention curve, as kept: what the
/// editor reads once and lines up again with each change to the cut.
#[derive(Debug, Clone, PartialEq)]
pub struct PostedRetention {
    pub publication: PublicationId,
    pub project: VideoProjectId,
    /// When the sync read the curve.
    pub read_at: SystemTime,
    pub curve: RetentionCurve,
    /// What the posted file was cut from; `None` once it is gone.
    cut: Option<RenderedCut>,
    /// The narration's sentences and their words.
    sentences: Vec<(Sentence, String)>,
}

/// A drop of the curve as the screens show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropView {
    pub drop: RetentionDrop,
    /// Where it starts and ends in the published video (and on the cut,
    /// when lined up); `None` when the video's length is not known.
    pub span: Option<(Duration, Duration)>,
    /// The scene shown in its middle, by index in the scene plan; only
    /// when lined up.
    pub scene: Option<usize>,
    /// Whether a picture shows there (footage when no scene), or the
    /// video ended and black plays on; only when lined up.
    pub picture: bool,
    /// The sentence spoken there, or the last one said before a pause;
    /// only when lined up.
    pub sentence: Option<String>,
}

/// The curve over the cut as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct CutRetention {
    pub publication: PublicationId,
    pub project: VideoProjectId,
    pub read_at: SystemTime,
    pub curve: RetentionCurve,
    /// How long the published video runs, when known.
    pub length: Option<Duration>,
    /// Whether the curve lines up with the cut: its timing is the one
    /// rendered for the post.
    pub lined_up: bool,
    /// The steepest drops, in time order.
    pub drops: Vec<DropView>,
}

impl CutRetention {
    /// The drop under `time` of the cut, when lined up.
    pub fn drop_at(&self, time: Duration) -> Option<usize> {
        if !self.lined_up {
            return None;
        }
        self.drops.iter().position(|drop| {
            drop.span
                .is_some_and(|(start, end)| start <= time && time <= end)
        })
    }
}

impl Bardo {
    /// `project`'s YouTube post with the retention curve its last sync
    /// read; `None` without such a post or before YouTube reports the
    /// curve. Disconnecting the account or *Delete YouTube data* deletes
    /// the curve, so it is `None` after those too.
    pub fn posted_retention(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<PostedRetention>, RepositoryError> {
        let Some(project) = self
            .themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
        else {
            return Ok(None);
        };
        let Some(publication) = self
            .publications
            .publications(project.id)?
            .into_iter()
            .find(|publication| publication.network() == Network::YouTube)
        else {
            return Ok(None);
        };
        let Some(retention) = self
            .publications
            .retention(publication.id)?
            .filter(|retention| !retention.curve.is_empty())
        else {
            return Ok(None);
        };
        let cut = self.renders.rendered_cut(publication.render)?;
        let sentences = self
            .narrations
            .narration(project.id)?
            .map(|narration| {
                sentences(&narration)
                    .into_iter()
                    .map(|sentence| {
                        let words = narration.text.as_str()[sentence.text.clone()].to_owned();
                        (sentence, words)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Some(PostedRetention {
            publication: publication.id,
            project: project.id,
            read_at: retention.read_at,
            curve: retention.curve,
            cut,
            sentences,
        }))
    }

    /// `posted`'s curve over the cut of `view`, the project's editor as it
    /// is now.
    pub fn retention_over(&self, posted: &PostedRetention, view: &EditorView) -> CutRetention {
        self.over_cut(posted, Some(view))
    }

    /// `posted`'s curve over the cut of `view`; without a view, unplaced.
    fn over_cut(&self, posted: &PostedRetention, view: Option<&EditorView>) -> CutRetention {
        let length = posted.cut.as_ref().map(|cut| cut.length);
        let timeline = view
            .and_then(|view| view.timeline.as_ref())
            .filter(|timeline| !timeline.is_empty());
        let lined_up = match (&posted.cut, view, timeline) {
            (Some(cut), Some(view), Some(timeline)) => match &cut.timing {
                Some(timing) => *timing == cut_timing(timeline),
                // Rendered before the timing was recorded: the same plan
                // renders the same cut.
                None => self.cut_fingerprint(view, cut.aspect).as_ref() == Some(&cut.cut),
            },
            _ => false,
        };
        let found = retention_drops(&posted.curve);
        let drops = match (length, timeline.filter(|_| lined_up)) {
            (Some(length), Some(timeline)) => {
                let spans: Vec<Sentence> = posted
                    .sentences
                    .iter()
                    .map(|(sentence, _)| sentence.clone())
                    .collect();
                place_drops(&found, length, timeline, &spans)
                    .into_iter()
                    .map(|placed| DropView {
                        drop: placed.drop,
                        span: Some((placed.start, placed.end)),
                        scene: placed.scene,
                        picture: placed.picture,
                        sentence: placed
                            .sentence
                            .and_then(|index| posted.sentences.get(index))
                            .map(|(_, words)| words.clone()),
                    })
                    .collect()
            }
            _ => found
                .into_iter()
                .map(|drop| DropView {
                    drop,
                    span: length
                        .map(|length| (time_at(drop.from, length), time_at(drop.to, length))),
                    scene: None,
                    picture: false,
                    sentence: None,
                })
                .collect(),
        };
        CutRetention {
            publication: posted.publication,
            project: posted.project,
            read_at: posted.read_at,
            curve: posted.curve.clone(),
            length,
            lined_up,
            drops,
        }
    }

    /// The curve of `project`'s YouTube post over the project's cut as
    /// saved, for Performance and the Publish stage.
    pub fn project_retention(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<CutRetention>, RepositoryError> {
        let Some(posted) = self.posted_retention(project)? else {
            return Ok(None);
        };
        let over = match self.editor_view(project) {
            Ok(view) => self.over_cut(&posted, Some(&view)),
            Err(crate::EditorError::Repository(error)) => return Err(error),
            // No cut to read it against.
            Err(_) => self.over_cut(&posted, None),
        };
        Ok(Some(over))
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        AudioLane, Edge, ItemRef, LaneMix, PostLink, PostRetention, Publication, PublicationKind,
        PublicationRepository, RenderRepository, RetentionPoint, Share,
    };

    use super::*;
    use crate::EditAction;
    use crate::scenes::tests::Harness;

    /// A curve losing half a point per hundredth from 90%, and six more
    /// points from 30% to 31%.
    fn curve() -> RetentionCurve {
        let mut watch = 9_000;
        RetentionCurve::of(
            (1..=100)
                .map(|n| {
                    let point = RetentionPoint {
                        elapsed: Share::from_ten_thousandths(n * 100),
                        watch: Share::from_ten_thousandths(watch),
                        relative: None,
                    };
                    watch -= if n == 30 { 650 } else { 50 };
                    point
                })
                .collect(),
        )
    }

    struct Posted {
        h: Harness,
        app: Bardo,
        project: VideoProjectId,
        publication: Publication,
    }

    /// A rendered project with its YouTube post and the post's curve.
    fn posted() -> Posted {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let youtube = app
            .add_network_account(
                project.channel,
                Network::YouTube,
                bardo_domain::NetworkAccountDraft {
                    handle: "archives".into(),
                    ..bardo_domain::NetworkAccountDraft::default()
                },
            )
            .unwrap();
        crate::render::tests::render_all(&app, project.id);
        let render = h.db.renders(project.id).unwrap().remove(0);
        let publication = Publication {
            id: PublicationId::new(),
            owner: project.owner,
            project: project.id,
            account: youtube.id,
            network: Network::YouTube,
            render: render.id,
            link: Some(PostLink::parse(Network::YouTube, "https://youtu.be/dQw4w9WgXcQ").unwrap()),
            kind: PublicationKind::Manual,
            posted_at: SystemTime::UNIX_EPOCH,
            linked_at: SystemTime::UNIX_EPOCH,
            checked_at: None,
            missing_since: None,
            insights_id: None,
        };
        h.db.save_publication(&publication).unwrap();
        h.db.save_retention(&[PostRetention {
            publication: publication.id,
            read_at: SystemTime::UNIX_EPOCH + Duration::from_secs(9),
            curve: curve(),
        }])
        .unwrap();
        Posted {
            h,
            app,
            project: project.id,
            publication,
        }
    }

    fn render_length(s: &Posted) -> Duration {
        s.h.db.renders(s.project).unwrap()[0].duration
    }

    #[test]
    fn the_curves_drops_are_placed_on_the_cut_it_was_rendered_from() {
        let s = posted();
        let over = s.app.project_retention(s.project).unwrap().unwrap();
        assert_eq!(over.publication, s.publication.id);
        assert_eq!(
            over.read_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(9)
        );
        assert_eq!(over.curve, curve());
        let length = render_length(&s);
        assert_eq!(over.length, Some(length));
        assert!(over.lined_up);
        assert_eq!(over.drops.len(), 1);
        let drop = &over.drops[0];
        assert_eq!(drop.drop.lost(), Share::from_ten_thousandths(650));
        let (start, end) = drop.span.unwrap();
        assert_eq!(start, time_at(Share::from_ten_thousandths(3_000), length));
        assert_eq!(end, time_at(Share::from_ten_thousandths(3_100), length));
        assert!(drop.scene.is_some(), "a scene plays there");
        assert!(drop.picture);
        let sentence = drop
            .sentence
            .as_deref()
            .expect("a sentence is spoken there");
        assert!(!sentence.is_empty());
        assert_eq!(over.drop_at(start), Some(0));
        assert_eq!(over.drop_at(end + Duration::from_secs(1)), None);
    }

    #[test]
    fn the_mix_keeps_it_lined_up_and_a_trim_does_not() {
        let s = posted();
        let posted = s.app.posted_retention(s.project).unwrap().unwrap();
        let mut editor = s.app.open_editor(s.project).unwrap();
        assert!(s.app.retention_over(&posted, editor.view()).lined_up);

        let music = LaneMix {
            muted: true,
            ..LaneMix::default()
        };
        s.app
            .edit(
                &mut editor,
                EditAction::SetLane {
                    lane: AudioLane::Music,
                    mix: music,
                },
            )
            .unwrap();
        s.app
            .edit(&mut editor, EditAction::ShowCaptions(false))
            .unwrap();
        assert!(s.app.retention_over(&posted, editor.view()).lined_up);

        let (_, length) = editor.view().span(ItemRef::video(0)).unwrap();
        s.app
            .edit(
                &mut editor,
                EditAction::Trim {
                    item: ItemRef::video(0),
                    edge: Edge::End,
                    to: length / 2,
                    reach: Duration::ZERO,
                },
            )
            .unwrap();
        let over = s.app.retention_over(&posted, editor.view());
        assert!(!over.lined_up);
        let drop = &over.drops[0];
        assert!(drop.span.is_some(), "still timed in the published video");
        assert_eq!((drop.scene, drop.sentence.as_deref()), (None, None));
        assert_eq!(over.drop_at(drop.span.unwrap().0), None);

        s.app.edit(&mut editor, EditAction::Undo).unwrap();
        assert!(s.app.retention_over(&posted, editor.view()).lined_up);
    }

    #[test]
    fn rendering_again_keeps_what_the_post_was_cut_from() {
        let s = posted();
        let mut editor = s.app.open_editor(s.project).unwrap();
        let (_, length) = editor.view().span(ItemRef::video(0)).unwrap();
        s.app
            .edit(
                &mut editor,
                EditAction::Trim {
                    item: ItemRef::video(0),
                    edge: Edge::End,
                    to: length / 2,
                    reach: Duration::ZERO,
                },
            )
            .unwrap();
        let length = render_length(&s);
        crate::render::tests::render_all(&s.app, s.project);
        assert_ne!(
            s.h.db.renders(s.project).unwrap()[0].id,
            s.publication.render
        );
        let over = s.app.project_retention(s.project).unwrap().unwrap();
        assert_eq!(over.length, Some(length), "the posted file's length");
        assert!(!over.lined_up, "the post was cut before the trim");

        s.app.edit(&mut editor, EditAction::Undo).unwrap();
        assert!(
            s.app
                .project_retention(s.project)
                .unwrap()
                .unwrap()
                .lined_up
        );
    }

    #[test]
    fn a_post_rendered_before_timings_were_kept_lines_up_while_the_plan_is_the_same() {
        let s = posted();
        let render = s.h.db.renders(s.project).unwrap().remove(0);
        s.h.db
            .save_render(&bardo_domain::Render {
                timing: None,
                ..render
            })
            .unwrap();
        assert!(
            s.app
                .project_retention(s.project)
                .unwrap()
                .unwrap()
                .lined_up
        );
        // Unlike the timing, the plan changes with the mix.
        let mut editor = s.app.open_editor(s.project).unwrap();
        s.app
            .edit(&mut editor, EditAction::ShowCaptions(false))
            .unwrap();
        assert!(
            !s.app
                .project_retention(s.project)
                .unwrap()
                .unwrap()
                .lined_up
        );
    }

    #[test]
    fn without_what_the_post_was_cut_from_the_drops_are_shares_of_the_video() {
        let s = posted();
        // A post of a file whose record is gone.
        s.h.db
            .save_publication(&Publication {
                render: bardo_domain::RenderId::new(),
                ..s.publication.clone()
            })
            .unwrap();
        let over = s.app.project_retention(s.project).unwrap().unwrap();
        assert_eq!(over.length, None);
        assert!(!over.lined_up);
        assert_eq!(over.drops.len(), 1);
        assert_eq!(over.drops[0].span, None);
        assert_eq!(over.drops[0].drop.from, Share::from_ten_thousandths(3_000));
    }

    #[test]
    fn only_a_youtube_post_with_a_curve_has_one() {
        let s = posted();
        s.h.db
            .save_retention(&[PostRetention {
                publication: s.publication.id,
                read_at: SystemTime::UNIX_EPOCH,
                curve: RetentionCurve::default(),
            }])
            .unwrap();
        assert_eq!(s.app.project_retention(s.project).unwrap(), None);
        s.h.db.remove_publication(s.publication.id).unwrap();
        assert_eq!(s.app.posted_retention(s.project).unwrap(), None);
        assert_eq!(s.app.posted_retention(VideoProjectId::new()).unwrap(), None);
    }
}
