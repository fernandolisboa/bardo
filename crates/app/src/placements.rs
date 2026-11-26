//! The export for ads (#283, docs/spec/marketing-videos.md, "Placement
//! exports"; ADR-0046): an ad's cut rendered in the shapes ad managers ask
//! for, 4:5 (1080 × 1350), 1:1 (1080 × 1080) and the editor's other shape,
//! each in `Videos\Bardo\<project> (<id>)\Ads\<shape>\` with its video and
//! `ad.txt`, the texts to paste. Bardo never talks to an ad manager.
//!
//! The cut stays in its frame. For each shape the composed pictures (the
//! product shots and the end card) are composed again in that shape, so
//! the product and the text stay whole; every other picture and clip
//! fills the frame through a window, centered unless the user moves it
//! for that clip in that shape (in 16:9 from a vertical cut each shows
//! whole with bars, as the cut's own framing does); the captions are laid
//! out for the shape, and none shows over the end card.
//!
//! The review asks what an upload of the ad asks (#282): every item
//! acknowledged, and every *product in use* scene checked. The job keeps
//! the plans the user reviewed and renders one shape after another,
//! checkpointing each; a shape is out of date once its plan or the ad's
//! texts change.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    AdExport, AdExportRepository, AdRepository, AdTextProblem, AdTexts, AdWindow, AspectRatio,
    CommercialDisclosure, CropPosition, ExportFiles, FPS, FrameShape, Job, JobFailure,
    JobFailureKind, JobId, JobKind, Loudness, Picture, PictureSize, ProductId, Progress,
    ProjectFiles, RepositoryError, TRUE_PEAK_CEILING, VideoCodec, VideoProject, VideoProjectId,
    ad_clip_key, image_dimensions, package_folder, unacknowledged, video_file_name,
};
use bardo_media::MediaEngine;
use bardo_media::ffmpeg::{
    ClipSource, FrameSize, LoudnessTarget, MediaError, Monitor, Output, RenderPlan,
};
use serde::{Deserialize, Serialize};

use crate::ad_publishing::AdPublishing;
use crate::editor::{EditorView, PlanMedia, timeline_plan};
use crate::jobs::{JobContext, JobHandler};
use crate::render::fingerprint;
use crate::scenes::{SceneHandler, id, parse, to_json};
use crate::{Bardo, ExportError, Text};

/// The video bitrate of every shape: what a 1080-line H.264 upload takes.
const VIDEO_BITRATE: u32 = 12_000_000;

/// AAC at 48 kHz stereo, as the render.
const AUDIO_BITRATE: u32 = 192_000;

/// How far under the true-peak ceiling normalization aims, as the render.
const CODEC_HEADROOM: f64 = 0.5;

/// The loudness of a shape when the project has no render to take it
/// from.
const DEFAULT_LOUDNESS_TENTHS: i16 = -140;

/// The text file beside each shape's video.
pub const AD_FILE: &str = "ad.txt";

#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
    #[error("video project not found")]
    ProjectNotFound,
    /// Only an ad is exported for ads.
    #[error("the project is not an ad")]
    NotAnAd,
    #[error("the project has no cut to export")]
    NoCut,
    /// The ad's texts are not written yet, or one is empty or too long.
    #[error("the ad's texts are missing or do not fit")]
    Texts,
    #[error("no shape chosen")]
    NothingChosen,
    /// An item of the ad's review is not acknowledged.
    #[error("the ad's review is not acknowledged")]
    NotAcknowledged,
    /// Scenes of the product in use wait for the user's check (#279).
    #[error("the product in some scenes is not checked")]
    ProductUnchecked,
    /// A composed picture waits to be made again.
    #[error("a composed picture is out of date")]
    PicturesWait,
    /// The cut or the texts changed after the review: review it again.
    #[error("the cut changed since the review")]
    CutChanged,
    #[error("the project is already exporting for ads")]
    AlreadyExporting,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl PlacementError {
    /// What the Publishing stage says.
    pub fn message(&self) -> Text {
        match self {
            PlacementError::ProjectNotFound | PlacementError::NotAnAd => Text::ProjectNotFound,
            PlacementError::NoCut => Text::RenderNoCut,
            PlacementError::Texts => Text::PlacementTextsNotReady,
            PlacementError::NothingChosen => Text::PlacementNothingChosen,
            PlacementError::NotAcknowledged => Text::AdNotAcknowledged,
            PlacementError::ProductUnchecked => Text::RenderProductUnchecked,
            PlacementError::PicturesWait => Text::PlacementPicturesWait,
            PlacementError::CutChanged => Text::RenderCutChanged,
            PlacementError::AlreadyExporting => Text::PlacementAlreadyRunning,
            PlacementError::Repository(_) => Text::ExportNotLoaded,
        }
    }
}

impl From<ExportError> for PlacementError {
    fn from(error: ExportError) -> Self {
        match error {
            ExportError::Repository(error) => PlacementError::Repository(error),
            _ => PlacementError::ProjectNotFound,
        }
    }
}

impl From<crate::EditorError> for PlacementError {
    fn from(error: crate::EditorError) -> Self {
        match error {
            crate::EditorError::Repository(error) => PlacementError::Repository(error),
            _ => PlacementError::ProjectNotFound,
        }
    }
}

/// One clip of the cut as a shape shows it through a window.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementClip {
    /// The clip, by `bardo_domain::ad_clip_key`.
    pub key: String,
    /// Its place on the video track, from 0.
    pub index: usize,
    /// The scene it shows (from 0), when it shows one.
    pub scene: Option<usize>,
    /// A picture of it for the review, if there is one.
    pub thumbnail: Option<PathBuf>,
    /// The size of its picture, or a guess of 16:9 when it cannot be read.
    pub source: PictureSize,
    /// Where its window is.
    pub position: CropPosition,
    /// Whether the user moved it from the middle.
    pub moved: bool,
}

/// One shape of the export, as the review shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementShape {
    pub shape: FrameShape,
    /// Whether the clips fill the frame through windows: not in 16:9 from
    /// a vertical cut, where each shows whole.
    pub windows: bool,
    /// The clips that show through windows, in the cut's order.
    pub clips: Vec<PlacementClip>,
    /// The shape's last export.
    pub last: Option<AdExport>,
    /// Whether `last` was made from the cut and texts as they are now.
    pub current: bool,
    made_from: String,
    plan: RenderPlan,
    composed: Vec<ComposeOrder>,
}

/// What the export for ads shows on the Publishing stage of an ad.
#[derive(Debug, Clone)]
pub struct PlacementReview {
    pub project: VideoProjectId,
    /// The cut's own shape.
    pub cut: AspectRatio,
    /// 4:5, 1:1 and the editor's other shape, in that order.
    pub shapes: Vec<PlacementShape>,
    /// The texts for an ad manager; `None` until the post texts' job wrote
    /// them.
    pub texts: Option<AdTexts>,
    /// The scenes (from 0) of the product in use not checked yet (#279).
    pub unchecked_products: Vec<usize>,
    /// A composed picture waits to be made again in the cut's shape.
    pub pictures_wait: bool,
    /// What an upload of the ad acknowledges (#282), shared with it.
    pub publishing: AdPublishing,
    /// The project's latest export for ads.
    pub job: Option<Job>,
    package: String,
    loudness: Loudness,
}

impl PlacementReview {
    /// What keeps the ad's texts from going out; `None` while there are
    /// none.
    pub fn text_problems(&self) -> Option<Vec<AdTextProblem>> {
        self.texts.as_ref().map(AdTexts::problems)
    }

    /// Whether nothing but the user's choice and acknowledgements keeps
    /// the export from starting.
    pub fn ready(&self) -> bool {
        self.unchecked_products.is_empty()
            && !self.pictures_wait
            && self
                .text_problems()
                .is_some_and(|problems| problems.is_empty())
            && !self.job.as_ref().is_some_and(|job| job.state().is_active())
    }
}

/// A composed scene a shape composes again before it renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ComposeOrder {
    /// The scene, from 0.
    scene: usize,
    /// The picture the cut shows for it: the job refuses a scene that
    /// shows another since the review.
    source: String,
    /// The file the shape's picture is written to in the project folder.
    file: String,
}

/// The file a shape's picture of the composed picture `source` is kept
/// in: named after both, so it is made once for each picture and shape.
fn composed_file(shape: FrameShape, source: &str) -> String {
    format!("ad-{}-{source}", shape.folder())
}

/// The file a shape renders to in the project folder, before it is copied
/// to the export folder.
fn rendered_file(shape: FrameShape) -> String {
    format!("ad-export-{}.mp4", shape.folder())
}

/// One shape of a job: the reviewed plan, what it composes first, and
/// what it writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ShapeOrder {
    shape: String,
    made_from: String,
    plan: RenderPlan,
    composed: Vec<ComposeOrder>,
    video_file: String,
    /// The text of `ad.txt`, in the language of the screen it was
    /// exported from.
    ad_text: String,
    /// The video of the shape's last export, removed when the new one has
    /// another name.
    previous: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PlacementPayload {
    project: String,
    package: String,
    product: String,
    loudness_tenths: i16,
    orders: Vec<ShapeOrder>,
}

/// The shapes a job has written, by code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct PlacementCheckpoint {
    done: Vec<String>,
}

/// How many shapes an export for ads has written, of how many.
pub fn placement_job_shapes(job: &Job) -> (usize, usize) {
    let total = serde_json::from_str::<PlacementPayload>(job.payload())
        .map_or(0, |payload| payload.orders.len());
    let done = job
        .checkpoint()
        .and_then(|checkpoint| serde_json::from_str::<PlacementCheckpoint>(checkpoint).ok())
        .map_or(0, |checkpoint| checkpoint.done.len());
    (done.min(total), total)
}

/// FNV-1a over `parts`, each ended by a separator.
fn hash(parts: &[&str]) -> String {
    let hash = parts
        .iter()
        .flat_map(|part| part.bytes().chain(std::iter::once(0x1f)))
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{hash:016x}")
}

/// `plan` without captions over the end card, which shows from `card`
/// on: a line that starts there is dropped, one that runs into it ends
/// where it starts.
fn without_captions_from(plan: &mut RenderPlan, card: Duration) {
    if let Some(captions) = &mut plan.captions {
        captions.lines.retain(|line| line.at < card);
        for line in &mut captions.lines {
            if line.at + line.duration > card {
                line.duration = card - line.at;
                line.words.retain(|word| *word < card);
            }
        }
    }
}

impl Bardo {
    /// The project's latest export for ads.
    pub fn placement_job(&self, project: VideoProjectId) -> Option<Job> {
        self.latest_job_of(JobKind::PlacementExport, project)
    }

    /// Where a shape's export for ads is on disk.
    pub fn placement_folder(&self, review: &PlacementReview, shape: FrameShape) -> PathBuf {
        self.export_files.ad_folder(&review.package, shape)
    }

    /// The export for ads of `project`; `None` for a project that is no
    /// ad, or an ad whose brief or product is gone.
    pub fn placement_review(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<PlacementReview>, PlacementError> {
        let project = self.export_project(project)?;
        if !project.is_ad() {
            return Ok(None);
        }
        let Some(publishing) = self.ad_publishing(&project)? else {
            return Ok(None);
        };
        let view = self.editor_view(project.id)?;
        let timeline = view
            .timeline
            .as_ref()
            .filter(|timeline| !timeline.is_empty())
            .ok_or(PlacementError::NoCut)?;
        let cut = timeline.aspect();
        let texts = self.ads.ad_texts(project.id)?;
        let windows = self.ad_exports.ad_windows(project.id)?;
        let exports = self.ad_exports.ad_exports(project.id)?;
        let plan = self.scene_plans.scene_plan(project.id)?;
        let unchecked_products = plan
            .as_ref()
            .map(|plan| plan.unchecked_products())
            .unwrap_or_default();
        let format = self.video_format(&project)?.aspect();
        let pictures_wait = match &plan {
            Some(plan) => !self.missing_shots(&project, plan, format)?.is_empty(),
            None => false,
        };
        let loudness = self.placement_loudness(project.id, cut)?;
        let sizes = self.footage_sizes(project.id)?;
        let mut shapes = Vec::new();
        for shape in FrameShape::for_ads(cut) {
            let Some(built) = self.shape_plan(
                &view,
                plan.as_ref(),
                shape,
                &windows,
                &sizes,
                texts.as_ref(),
                loudness,
            ) else {
                return Err(PlacementError::NoCut);
            };
            let last = exports.iter().find(|export| export.shape == shape).cloned();
            let current = last
                .as_ref()
                .is_some_and(|last| last.made_from == built.made_from);
            shapes.push(PlacementShape {
                last,
                current,
                ..built
            });
        }
        Ok(Some(PlacementReview {
            project: project.id,
            cut,
            shapes,
            texts,
            unchecked_products,
            pictures_wait,
            publishing,
            job: self.placement_job(project.id),
            package: package_folder(&project.title, project.id),
            loudness,
        }))
    }

    /// The loudness of the project's own render in its cut's shape, else
    /// of any of its renders, else −14 LUFS.
    fn placement_loudness(
        &self,
        project: VideoProjectId,
        cut: AspectRatio,
    ) -> Result<Loudness, RepositoryError> {
        let renders = self.renders.renders(project)?;
        let loudness = renders
            .iter()
            .find(|render| render.preset.aspect == cut)
            .or(renders.first())
            .map(|render| render.preset.loudness);
        Ok(loudness.unwrap_or_else(|| {
            Loudness::from_tenths(DEFAULT_LOUDNESS_TENTHS).expect("−14 LUFS is in range")
        }))
    }

    /// The picture size of each imported video, by file.
    fn footage_sizes(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<(String, PictureSize)>, RepositoryError> {
        Ok(self
            .media_assets
            .media_assets(project)?
            .into_iter()
            .filter_map(|asset| asset.picture.map(|size| (asset.file, size)))
            .collect())
    }

    /// The cut of `view` as `shape` renders it, with what it composes
    /// first and the clips that show through windows; `None` without a
    /// cut.
    #[allow(clippy::too_many_arguments)]
    fn shape_plan(
        &self,
        view: &EditorView,
        plan: Option<&bardo_domain::ScenePlan>,
        shape: FrameShape,
        windows: &[AdWindow],
        sizes: &[(String, PictureSize)],
        texts: Option<&AdTexts>,
        loudness: Loudness,
    ) -> Option<PlacementShape> {
        let timeline = view.timeline.as_ref()?;
        let project = view.project.id;
        // The editor's shapes frame as a cut in them does; 4:5 and 1:1
        // start from the cut's own plan and take windows below.
        let base = shape.aspect().unwrap_or(timeline.aspect());
        let mut render = timeline_plan(view, &*self.files, base, PlanMedia::Originals)?;
        // In 16:9 every picture shows whole: the cut's framing for it.
        let windowed = shape != FrameShape::Landscape;
        let mut clips = Vec::new();
        let mut composed = Vec::new();
        let mut card_at = None;
        for (index, item) in timeline.video().iter().enumerate() {
            let scene = match item.picture {
                Picture::Scene(scene) => plan.and_then(|plan| plan.scene(scene).ok()),
                Picture::Footage(_) => None,
            };
            if scene.is_some_and(|scene| scene.is_end_card()) && card_at.is_none() {
                card_at = Some(item.at);
            }
            let Some(clip) = render.video.get_mut(index) else {
                continue;
            };
            if let Some(scene) = scene.filter(|scene| scene.is_composed()) {
                // Composed again in the shape: the product and text whole.
                if let (Some(source), Picture::Scene(at)) = (scene.picture(), &item.picture) {
                    let file = composed_file(shape, source);
                    clip.source = ClipSource::Still(self.files.path(project, &file));
                    clip.framing = bardo_media::ffmpeg::Framing::Fit;
                    if !composed
                        .iter()
                        .any(|order: &ComposeOrder| order.file == file)
                    {
                        composed.push(ComposeOrder {
                            scene: *at,
                            source: source.to_owned(),
                            file,
                        });
                    }
                }
                continue;
            }
            if !windowed || matches!(clip.source, ClipSource::Black) {
                continue;
            }
            let key = ad_clip_key(&item.picture, item.source_start());
            let saved = windows
                .iter()
                .find(|window| window.shape == shape && window.clip == key);
            let position = saved.map_or(CropPosition::CENTER, |window| window.position);
            let (x, y) = position.fractions();
            clip.framing = bardo_media::ffmpeg::Framing::Crop { x, y };
            let shown = view.clips.get(index);
            let thumbnail = shown.and_then(|shown| shown.thumbnail.clone());
            let source = match &item.picture {
                Picture::Footage(file) => sizes
                    .iter()
                    .find(|(name, _)| name == file)
                    .map(|(_, size)| *size),
                Picture::Scene(_) => None,
            }
            .or_else(|| {
                let path = shown.and_then(|shown| shown.thumbnail.as_ref())?;
                let bytes = std::fs::read(path).ok()?;
                image_dimensions(&bytes).map(|(width, height)| PictureSize::new(width, height))
            })
            .unwrap_or(PictureSize::new(1920, 1080));
            clips.push(PlacementClip {
                key,
                index,
                scene: match item.picture {
                    Picture::Scene(scene) => Some(scene),
                    Picture::Footage(_) => None,
                },
                thumbnail,
                source,
                position,
                moved: saved.is_some(),
            });
        }
        if let Some(card) = card_at {
            without_captions_from(&mut render, card);
        }
        let (primary, headline) =
            texts.map_or(("", ""), |texts| (texts.primary(), texts.headline()));
        let made_from = hash(&[
            shape.code(),
            &fingerprint(&render),
            primary,
            headline,
            &loudness.tenths().to_string(),
        ]);
        Some(PlacementShape {
            shape,
            windows: windowed,
            clips,
            last: None,
            current: false,
            made_from,
            plan: render,
            composed,
        })
    }

    /// Moves a clip's window in one shape of the export for ads; the
    /// middle forgets it.
    pub fn set_placement_window(
        &self,
        project: VideoProjectId,
        shape: FrameShape,
        clip: &str,
        position: CropPosition,
    ) -> Result<(), PlacementError> {
        let project = self.export_project(project)?;
        if !project.is_ad() {
            return Err(PlacementError::NotAnAd);
        }
        if position == CropPosition::CENTER {
            self.ad_exports.drop_ad_window(project.id, shape, clip)?;
        } else {
            self.ad_exports.save_ad_window(
                project.id,
                &AdWindow {
                    shape,
                    clip: clip.to_owned(),
                    position,
                },
            )?;
        }
        Ok(())
    }

    /// Replaces the ad's texts for an ad manager with the user's. Text
    /// over a limit is kept, and keeps the export from starting until it
    /// fits.
    pub fn set_ad_texts(
        &self,
        project: VideoProjectId,
        primary: &str,
        headline: &str,
    ) -> Result<AdTexts, PlacementError> {
        let project = self.export_project(project)?;
        if !project.is_ad() {
            return Err(PlacementError::NotAnAd);
        }
        let texts = AdTexts::new(primary, headline);
        if self.ads.ad_texts(project.id)?.as_ref() != Some(&texts) {
            self.ads.save_ad_texts(project.id, &texts)?;
        }
        Ok(texts)
    }

    /// Queues the export for ads of the `chosen` shapes of `review`, with
    /// the keys of the items the user `acknowledged`. Refuses an item left
    /// out, a *product in use* scene not checked, a composed picture that
    /// waits, texts that do not fit, and a cut or texts that changed since
    /// the review.
    pub fn start_placement_export(
        &self,
        review: &PlacementReview,
        chosen: &[FrameShape],
        acknowledged: &[String],
    ) -> Result<JobId, PlacementError> {
        if self
            .placement_job(review.project)
            .is_some_and(|job| self.at_work(&job))
        {
            return Err(PlacementError::AlreadyExporting);
        }
        // What is stored now, not what the screen last read.
        let now = self
            .placement_review(review.project)?
            .ok_or(PlacementError::NotAnAd)?;
        if !now.unchecked_products.is_empty() {
            return Err(PlacementError::ProductUnchecked);
        }
        if now.pictures_wait {
            return Err(PlacementError::PicturesWait);
        }
        let texts = match &now.texts {
            Some(texts) if texts.problems().is_empty() => texts.clone(),
            _ => return Err(PlacementError::Texts),
        };
        if !unacknowledged(&now.publishing.items, acknowledged).is_empty() {
            return Err(PlacementError::NotAcknowledged);
        }
        let shapes: Vec<&PlacementShape> = now
            .shapes
            .iter()
            .filter(|shape| chosen.contains(&shape.shape))
            .collect();
        if shapes.is_empty() {
            return Err(PlacementError::NothingChosen);
        }
        let changed = shapes.iter().any(|shape| {
            review
                .shapes
                .iter()
                .find(|seen| seen.shape == shape.shape)
                .is_none_or(|seen| seen.made_from != shape.made_from)
        });
        if changed {
            return Err(PlacementError::CutChanged);
        }
        let project = self.export_project(review.project)?;
        let product = project.product.ok_or(PlacementError::NotAnAd)?;
        let brief = self
            .script_brief(&project)?
            .ok_or(PlacementError::NotAnAd)?;
        let video_file = video_file_name(&project.title);
        let orders = shapes
            .iter()
            .map(|shape| ShapeOrder {
                shape: shape.shape.code().to_owned(),
                made_from: shape.made_from.clone(),
                plan: shape.plan.clone(),
                composed: shape.composed.clone(),
                ad_text: self.ad_file(
                    &project,
                    shape.shape,
                    &video_file,
                    &texts,
                    brief.brief.text(),
                    &now.publishing,
                ),
                previous: shape
                    .last
                    .as_ref()
                    .map(|last| last.file.clone())
                    .filter(|file| *file != video_file),
                video_file: video_file.clone(),
            })
            .collect();
        let payload = PlacementPayload {
            project: project.id.to_string(),
            package: now.package.clone(),
            product: product.to_string(),
            loudness_tenths: now.loudness.tenths(),
            orders,
        };
        let job = Job::new(self.profile.id, JobKind::PlacementExport, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// The `ad.txt` of one shape, in the interface language: the video,
    /// the texts to paste, the call to action and link, and what to keep
    /// in mind when posting it.
    fn ad_file(
        &self,
        project: &VideoProject,
        shape: FrameShape,
        video_file: &str,
        texts: &AdTexts,
        brief: &bardo_domain::BriefText,
        publishing: &AdPublishing,
    ) -> String {
        let (width, height) = shape.size();
        let field = |label: Text, value: &str| format!("{}\n{value}", self.text(label));
        let mut blocks = vec![
            self.text_with(
                Text::PlacementFileVideo,
                &[
                    ("file", video_file),
                    ("size", &format!("{width}×{height}")),
                    ("shape", shape.code()),
                ],
            ),
            field(Text::PlacementPrimary, texts.primary()),
            field(Text::PlacementHeadline, texts.headline()),
        ];
        if !brief.call_to_action().is_empty() {
            blocks.push(field(Text::PlacementCall, brief.call_to_action()));
        }
        if !brief.link().is_empty() {
            blocks.push(field(Text::PlacementLink, brief.link()));
        }
        if publishing.disclosure != CommercialDisclosure::None {
            blocks.push(self.text_with(
                Text::PlacementCommercialReminder,
                &[(
                    "disclosure",
                    &self.text(Text::CommercialName(publishing.disclosure)),
                )],
            ));
        }
        if self.needs_disclosure(project.id).unwrap_or(true) {
            blocks.push(self.text(Text::PlacementSyntheticReminder).into_owned());
        }
        if !publishing.items.is_empty() {
            let lines: Vec<String> = publishing
                .items
                .iter()
                .map(|item| format!("- {}", self.ad_item_text(item)))
                .collect();
            blocks.push(format!(
                "{}\n{}",
                self.text(Text::AdItemsHeading),
                lines.join("\n")
            ));
        }
        let mut text = blocks.join("\n\n");
        text.push('\n');
        // Notepad and every ad manager take Windows line ends.
        text.replace('\n', "\r\n")
    }

    /// A shape as the review names it: `4:5 (1080 × 1350)`.
    pub fn placement_shape_text(&self, shape: FrameShape) -> String {
        let (width, height) = shape.size();
        self.text_with(
            Text::PlacementShapeName,
            &[
                ("shape", shape.code()),
                ("size", &format!("{width} × {height}")),
            ],
        )
    }

    /// What keeps the ad's texts from going out, as one sentence.
    pub fn ad_text_problem_text(&self, problem: AdTextProblem) -> String {
        let (text, max) = match problem {
            AdTextProblem::PrimaryEmpty => (Text::PlacementPrimaryEmpty, 0),
            AdTextProblem::PrimaryTooLong => {
                (Text::PlacementPrimaryTooLong, bardo_domain::AD_PRIMARY_MAX)
            }
            AdTextProblem::HeadlineEmpty => (Text::PlacementHeadlineEmpty, 0),
            AdTextProblem::HeadlineTooLong => (
                Text::PlacementHeadlineTooLong,
                bardo_domain::AD_HEADLINE_MAX,
            ),
        };
        self.text_with(text, &[("max", &max.to_string())])
    }
}

/// Renders an ad's export for ads, one shape after another.
pub(crate) struct PlacementHandler {
    /// Composes the shapes' pictures as the scenes compose the cut's.
    pub(crate) scenes: SceneHandler,
    pub(crate) ad_exports: Arc<dyn AdExportRepository>,
    pub(crate) export_files: Arc<dyn ExportFiles>,
}

/// One shape's share of the job's progress, and its cancel.
struct ShapeMonitor<'a> {
    cx: &'a JobContext,
    done: usize,
    total: usize,
}

impl Monitor for ShapeMonitor<'_> {
    fn should_stop(&self) -> bool {
        self.cx.should_stop()
    }

    fn progress(&self, fraction: f32) {
        let share = (self.done as f32 + fraction.clamp(0.0, 1.0)) / self.total.max(1) as f32;
        self.cx
            .report_progress(Progress::from_permille((share * 1000.0) as u16));
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

fn media_failure(shape: &str, error: &MediaError) -> JobFailure {
    JobFailure::new(JobFailureKind::Media, format!("{shape}: {error}"))
}

impl PlacementHandler {
    fn files(&self) -> &dyn ProjectFiles {
        self.scenes.files.as_ref()
    }

    fn media(&self) -> &dyn MediaEngine {
        self.scenes.media.as_ref()
    }

    /// Composes each picture `order` needs in `shape` that is not made
    /// yet, from the scene as it is now. A scene that shows another
    /// picture than the reviewed one fails the shape: review it again.
    fn compose(
        &self,
        project: VideoProjectId,
        product: ProductId,
        shape: FrameShape,
        orders: &[ComposeOrder],
    ) -> Result<(), JobFailure> {
        if orders
            .iter()
            .all(|order| self.files().exists(project, &order.file))
        {
            return Ok(());
        }
        let plan = self
            .scenes
            .plans
            .scene_plan(project)
            .map_err(unexpected)?
            .ok_or_else(|| JobFailure::unexpected("the project has no scenes"))?;
        for order in orders {
            if self.files().exists(project, &order.file) {
                continue;
            }
            let scene = plan
                .scene(order.scene)
                .ok()
                .filter(|scene| scene.picture() == Some(order.source.as_str()))
                .ok_or_else(|| {
                    JobFailure::new(
                        JobFailureKind::Files,
                        format!(
                            "scene {} changed since the review: review the export again",
                            order.scene + 1
                        ),
                    )
                })?;
            let png = if scene.is_end_card() {
                let inputs = crate::end_cards::CardInputs::of(
                    self.scenes.ads.as_ref(),
                    self.scenes.end_cards.as_ref(),
                    self.scenes.products.as_ref(),
                    project,
                    Some(product),
                )
                .map_err(unexpected)?
                .ok_or_else(|| JobFailure::unexpected("the ad's brief is gone"))?;
                self.scenes
                    .compose_card_picture(
                        project,
                        order.scene,
                        &inputs,
                        scene.product_image(),
                        scene.look(),
                        product,
                        shape,
                    )?
                    .0
            } else {
                let image = scene
                    .product_image()
                    .ok_or_else(|| JobFailure::unexpected("a product shot shows no image"))?;
                self.scenes
                    .compose_product_shot(
                        project,
                        order.scene,
                        image,
                        scene.look(),
                        product,
                        shape,
                    )?
                    .png
            };
            self.files()
                .write(project, &order.file, &png)
                .map_err(unexpected)?;
        }
        Ok(())
    }

    /// Renders `plan` in `shape` with the best H.264 encoder, then with
    /// the software one if a hardware one fails.
    fn render(
        &self,
        plan: &RenderPlan,
        shape: FrameShape,
        loudness: Loudness,
        destination: &std::path::Path,
        monitor: &ShapeMonitor<'_>,
    ) -> Result<(), MediaError> {
        let (width, height) = shape.size();
        let output = |encoder| Output {
            size: FrameSize::new(width, height),
            fps: (FPS, 1),
            encoder,
            video_bitrate: VIDEO_BITRATE,
            audio_bitrate: AUDIO_BITRATE,
        };
        let target = LoudnessTarget {
            integrated: loudness.lufs() as f32,
            true_peak: (TRUE_PEAK_CEILING - CODEC_HEADROOM) as f32,
        };
        let encoders = self.media().encoders()?;
        let best = encoders.best(VideoCodec::H264)?;
        match self
            .media()
            .render(plan, &output(best), Some(target), destination, monitor)
        {
            Err(MediaError::Failed { .. }) if best.is_hardware() => {
                let software = encoders.software(VideoCodec::H264)?;
                self.media()
                    .render(plan, &output(software), Some(target), destination, monitor)
            }
            result => result,
        }
    }
}

impl JobHandler for PlacementHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: PlacementPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let product: ProductId = id(&payload.product)?;
        let loudness = Loudness::from_tenths(payload.loudness_tenths)
            .map_err(|_| JobFailure::unexpected("invalid loudness"))?;
        let mut checkpoint: PlacementCheckpoint = match cx.checkpoint() {
            Some(text) => parse(text)?,
            None => PlacementCheckpoint::default(),
        };
        let total = payload.orders.len();
        for order in &payload.orders {
            if checkpoint.done.contains(&order.shape) {
                continue;
            }
            if cx.should_stop() {
                return Ok(());
            }
            let shape: FrameShape = order
                .shape
                .parse()
                .map_err(|_| JobFailure::unexpected("unknown shape"))?;
            self.compose(project, product, shape, &order.composed)?;
            let rendered = rendered_file(shape);
            let destination = self.files().path(project, &rendered);
            let monitor = ShapeMonitor {
                cx,
                done: checkpoint.done.len(),
                total,
            };
            match self.render(&order.plan, shape, loudness, &destination, &monitor) {
                Ok(()) => {}
                Err(MediaError::Cancelled) => return Ok(()),
                Err(error) => return Err(media_failure(&order.shape, &error)),
            }
            let mut source = self.files().open(project, &rendered).map_err(unexpected)?;
            self.export_files
                .copy_ad_from(&payload.package, shape, &order.video_file, &mut source)
                .map_err(unexpected)?;
            self.export_files
                .write_ad(&payload.package, shape, AD_FILE, order.ad_text.as_bytes())
                .map_err(unexpected)?;
            if let Some(previous) = &order.previous {
                self.export_files
                    .remove_ad(&payload.package, shape, previous)
                    .map_err(unexpected)?;
            }
            let size_bytes = std::fs::metadata(&destination)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            // The project folder keeps no second copy.
            let _ = self.files().remove(project, &rendered);
            self.ad_exports
                .save_ad_export(&AdExport {
                    project,
                    shape,
                    made_from: order.made_from.clone(),
                    file: order.video_file.clone(),
                    size_bytes,
                    exported_at: SystemTime::now(),
                })
                .map_err(unexpected)?;
            checkpoint.done.push(order.shape.clone());
            let progress = Progress::of(checkpoint.done.len() as u64, total as u64);
            cx.save_checkpoint(to_json(&checkpoint), progress)
                .map_err(unexpected)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use bardo_domain::{
        AdReviewItem, CaptionStyle, CheckedScript, JobState, Network, NetworkAccountDraft,
        ProjectFileError,
    };
    use bardo_media::ffmpeg::{CaptionLine, CaptionTrack};
    use bardo_storage::MemoryExportFiles;

    use super::*;
    use crate::BudgetConsent;
    use crate::ad_angles::testing::TYPESAFE_KEY;
    use crate::ad_scenes::ad_tests::{A_SCREEN_AND_A_PHOTO, answer, narrated_ad, plan_with};
    use crate::export::tests::add_account;
    use crate::scenes::tests::{Harness, done, wait_done};

    /// Export files in memory that note each video copied, by shape, and
    /// fail the first copy of a 1:1 video while `fail_square` is set.
    #[derive(Default)]
    struct NotedExports {
        inner: MemoryExportFiles,
        copied: Mutex<Vec<FrameShape>>,
        fail_square: AtomicBool,
    }

    impl ExportFiles for NotedExports {
        fn write(
            &self,
            package: &str,
            network: Network,
            name: &str,
            bytes: &[u8],
        ) -> Result<(), ProjectFileError> {
            self.inner.write(package, network, name, bytes)
        }

        fn copy_from(
            &self,
            package: &str,
            network: Network,
            name: &str,
            source: &mut dyn std::io::Read,
        ) -> Result<(), ProjectFileError> {
            self.inner.copy_from(package, network, name, source)
        }

        fn remove(
            &self,
            package: &str,
            network: Network,
            name: &str,
        ) -> Result<(), ProjectFileError> {
            self.inner.remove(package, network, name)
        }

        fn exists(&self, package: &str, network: Network, name: &str) -> bool {
            self.inner.exists(package, network, name)
        }

        fn folder(&self, package: &str, network: Network) -> PathBuf {
            self.inner.folder(package, network)
        }

        fn copy_ad_from(
            &self,
            package: &str,
            shape: FrameShape,
            name: &str,
            source: &mut dyn std::io::Read,
        ) -> Result<(), ProjectFileError> {
            if shape == FrameShape::Square && self.fail_square.swap(false, Ordering::SeqCst) {
                return Err(ProjectFileError {
                    name: name.to_owned(),
                    source: std::io::Error::other("the disk is full"),
                });
            }
            self.copied.lock().unwrap().push(shape);
            self.inner.copy_ad_from(package, shape, name, source)
        }

        fn write_ad(
            &self,
            package: &str,
            shape: FrameShape,
            name: &str,
            bytes: &[u8],
        ) -> Result<(), ProjectFileError> {
            self.inner.write_ad(package, shape, name, bytes)
        }

        fn remove_ad(
            &self,
            package: &str,
            shape: FrameShape,
            name: &str,
        ) -> Result<(), ProjectFileError> {
            self.inner.remove_ad(package, shape, name)
        }

        fn ad_folder(&self, package: &str, shape: FrameShape) -> PathBuf {
            self.inner.ad_folder(package, shape)
        }
    }

    /// The posts and the texts for an ad manager, as the ad metadata job
    /// answers.
    fn written_posts() -> String {
        serde_json::json!({
            "posts": [{
                "network": "youtube",
                "title": "The notebook that refills",
                "description": "This one refills. Get yours at example.com/notebook.",
                "tags": ["notebook"]
            }],
            "ad": {
                "primary_text": "Tired of buying notebooks? This one refills.",
                "headline": "The notebook that refills"
            }
        })
        .to_string()
    }

    /// A vertical ad cut from a drawn hook, a product shot, a drawn scene
    /// and the end card, with its post texts and ad texts written.
    fn ad(h: &Harness, exports: &Arc<NotedExports>) -> (Bardo, VideoProject) {
        let mut app = h.start_with_exports(Arc::clone(exports) as _);
        app.save_provider_key(bardo_domain::Provider::TypeSafe, TYPESAFE_KEY)
            .unwrap();
        let (_, project, _) = narrated_ad(h, &app, &A_SCREEN_AND_A_PHOTO);
        add_account(
            &app,
            &project,
            Network::YouTube,
            NetworkAccountDraft::default(),
        );
        plan_with(
            h,
            &app,
            &project,
            answer([
                ("drawn", -1),
                ("product_shot", 1),
                ("drawn", -1),
                ("end_card", 0),
            ]),
        );
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        (app, project)
    }

    fn write_texts(h: &Harness, app: &Bardo, project: &VideoProject) {
        h.answer(written_posts());
        // A sensitive claim used: an item to acknowledge.
        h.text
            .check_with(&[(2, "This one refills", 1), (6, "Lasts 3 school years", 3)]);
        done(
            app,
            app.generate_metadata(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    fn review(app: &Bardo, project: &VideoProject) -> PlacementReview {
        app.placement_review(project.id).unwrap().unwrap()
    }

    fn shape(review: &PlacementReview, shape: FrameShape) -> &PlacementShape {
        review.shapes.iter().find(|s| s.shape == shape).unwrap()
    }

    fn size_of(h: &Harness, project: &VideoProject, file: &str) -> (u32, u32) {
        let bytes = h.files.read(project.id, file).unwrap();
        image_dimensions(&bytes).unwrap()
    }

    #[test]
    fn the_ad_metadata_job_writes_the_ad_texts_and_the_check_reads_them() {
        let h = Harness::new();
        let exports: Arc<NotedExports> = Arc::default();
        let (app, project) = ad(&h, &exports);
        let before = review(&app, &project);
        assert_eq!(before.texts, None);
        assert!(!before.ready());

        write_texts(&h, &app, &project);
        let request = h
            .text
            .requests()
            .into_iter()
            .rev()
            .find(|request| request.prompt.contains("The networks, one per line"))
            .unwrap();
        assert!(request.instructions.ends_with(crate::ads::AD_TEXTS_RULES));
        let bardo_domain::TextFormat::Json { schema } = &request.format else {
            panic!("metadata is asked for as JSON");
        };
        assert!(schema.contains("primary_text"), "{schema}");
        let check = h.text.check_requests().pop().unwrap();
        assert!(
            check.prompt.contains(
                "4. Ad manager: The notebook that refills.\n5. Tired of buying notebooks?"
            ),
            "{}",
            check.prompt
        );

        let after = review(&app, &project);
        let texts = after.texts.clone().unwrap();
        assert_eq!(
            texts.primary(),
            "Tired of buying notebooks? This one refills."
        );
        assert_eq!(after.text_problems(), Some(Vec::new()));
        // An edit is kept past its limit and puts the check out of date.
        let long = "x".repeat(41);
        app.set_ad_texts(project.id, texts.primary(), &long)
            .unwrap();
        let edited = review(&app, &project);
        assert_eq!(
            edited.text_problems(),
            Some(vec![AdTextProblem::HeadlineTooLong])
        );
        assert!(
            edited
                .publishing
                .items
                .contains(&AdReviewItem::NotChecked(CheckedScript::Posts))
        );
        assert!(matches!(
            app.start_placement_export(&edited, &[FrameShape::Portrait], &edited.publishing.keys()),
            Err(PlacementError::Texts)
        ));
    }

    #[test]
    fn a_vertical_cut_exports_to_4_5_1_1_and_16_9_with_its_composed_pictures_made_again() {
        let h = Harness::new();
        let exports: Arc<NotedExports> = Arc::default();
        let (app, project) = ad(&h, &exports);
        write_texts(&h, &app, &project);
        let review = review(&app, &project);
        assert_eq!(review.cut, AspectRatio::Vertical);
        assert_eq!(
            review.shapes.iter().map(|s| s.shape).collect::<Vec<_>>(),
            [
                FrameShape::Portrait,
                FrameShape::Square,
                FrameShape::Landscape
            ]
        );
        let portrait = shape(&review, FrameShape::Portrait);
        assert!(portrait.windows);
        // The drawn scenes show through windows; the product shot and the
        // end card are composed again.
        let scenes: Vec<_> = portrait.clips.iter().map(|clip| clip.scene).collect();
        assert_eq!(scenes, [Some(0), Some(2)]);
        assert!(portrait.clips.iter().all(|clip| !clip.moved));
        assert_eq!(portrait.composed.len(), 2);
        let wide = shape(&review, FrameShape::Landscape);
        assert!(!wide.windows && wide.clips.is_empty());
        assert!(review.shapes.iter().all(|s| s.last.is_none() && !s.current));

        // Nothing starts with an item left unacknowledged.
        assert!(!review.publishing.items.is_empty());
        assert!(matches!(
            app.start_placement_export(&review, &[FrameShape::Portrait], &[]),
            Err(PlacementError::NotAcknowledged)
        ));
        assert!(matches!(
            app.start_placement_export(&review, &[], &review.publishing.keys()),
            Err(PlacementError::NothingChosen)
        ));
        let all = FrameShape::for_ads(review.cut);
        let job = app
            .start_placement_export(&review, &all, &review.publishing.keys())
            .unwrap();
        done(&app, job);

        for order in &shape(&review, FrameShape::Portrait).composed {
            assert_eq!(size_of(&h, &project, &order.file), (1080, 1350));
        }
        for order in &shape(&review, FrameShape::Square).composed {
            assert_eq!(size_of(&h, &project, &order.file), (1080, 1080));
        }
        for order in &shape(&review, FrameShape::Landscape).composed {
            assert_eq!(size_of(&h, &project, &order.file), (1920, 1080));
        }
        let video = video_file_name(&project.title);
        for shape in all {
            let names = exports.inner.ad_names(&review.package, shape);
            assert_eq!(names, [video.clone(), AD_FILE.to_owned()], "{shape}");
            assert!(
                exports
                    .inner
                    .read_ad(&review.package, shape, &video)
                    .is_some()
            );
        }
        let text = String::from_utf8(
            exports
                .inner
                .read_ad(&review.package, FrameShape::Square, AD_FILE)
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("1080×1080"), "{text}");
        assert!(text.contains("Primary text\r\nTired of buying notebooks? This one refills."));
        assert!(text.contains("Headline\r\nThe notebook that refills"));
        assert!(text.contains("Get yours"), "the call to action: {text}");
        assert!(text.contains("example.com/notebook"), "the link: {text}");
        assert!(text.contains("Commercial disclosure"), "{text}");
        let calls = h.media.renders();
        assert_eq!(calls.len(), 3);
        for (call, shape) in calls.iter().zip(all) {
            let (width, height) = shape.size();
            assert_eq!(call.output.size, FrameSize::new(width, height));
            assert!(call.destination.ends_with(rendered_file(shape)));
            // The composed scenes show the shape's own pictures.
            for order in &self::shape(&review, shape).composed {
                assert!(call.plan.video.iter().any(|clip| matches!(
                    &clip.source,
                    ClipSource::Still(path) if path.ends_with(&order.file)
                )));
            }
        }
        // The project folder keeps no second copy.
        assert!(
            !h.files
                .exists(project.id, &rendered_file(FrameShape::Square))
        );

        let after = self::review(&app, &project);
        assert!(after.shapes.iter().all(|s| s.current), "{:?}", after.shapes);
        // New texts put every shape out of date.
        app.set_ad_texts(project.id, "Refill it, keep writing.", "Refill it")
            .unwrap();
        let edited = self::review(&app, &project);
        assert!(edited.shapes.iter().all(|s| s.last.is_some() && !s.current));
    }

    #[test]
    fn a_window_moved_for_one_clip_in_one_shape_moves_only_that_one_and_is_kept() {
        let h = Harness::new();
        let exports: Arc<NotedExports> = Arc::default();
        let (app, project) = ad(&h, &exports);
        write_texts(&h, &app, &project);
        let before = review(&app, &project);
        let clip = shape(&before, FrameShape::Portrait).clips[0].clone();
        let left = CropPosition::new(0, 500);
        app.set_placement_window(project.id, FrameShape::Portrait, &clip.key, left)
            .unwrap();

        let after = review(&app, &project);
        let portrait = shape(&after, FrameShape::Portrait);
        assert_eq!(portrait.clips[0].position, left);
        assert!(portrait.clips[0].moved);
        assert_eq!(portrait.clips[1].position, CropPosition::CENTER);
        assert_eq!(
            portrait.plan.video[clip.index].framing,
            bardo_media::ffmpeg::Framing::Crop { x: 0.0, y: 0.5 }
        );
        assert_ne!(
            portrait.made_from,
            shape(&before, FrameShape::Portrait).made_from
        );
        let square = shape(&after, FrameShape::Square);
        assert!(square.clips.iter().all(|clip| !clip.moved));
        assert_eq!(
            square.made_from,
            shape(&before, FrameShape::Square).made_from
        );
        // A review from before the move is refused for that shape.
        assert!(matches!(
            app.start_placement_export(&before, &[FrameShape::Portrait], &before.publishing.keys()),
            Err(PlacementError::CutChanged)
        ));
        // Back in the middle, the window is forgotten.
        app.set_placement_window(
            project.id,
            FrameShape::Portrait,
            &clip.key,
            CropPosition::CENTER,
        )
        .unwrap();
        assert!(!shape(&review(&app, &project), FrameShape::Portrait).clips[0].moved);
    }

    #[test]
    fn a_resumed_export_does_not_render_the_shapes_it_finished() {
        let h = Harness::new();
        let exports: Arc<NotedExports> = Arc::default();
        let (app, project) = ad(&h, &exports);
        write_texts(&h, &app, &project);
        let review = review(&app, &project);
        exports.fail_square.store(true, Ordering::SeqCst);
        let job = app
            .start_placement_export(
                &review,
                &[FrameShape::Portrait, FrameShape::Square],
                &review.publishing.keys(),
            )
            .unwrap();
        let failed = wait_done(&app, job);
        assert_eq!(failed.state(), JobState::Failed);
        assert_eq!(placement_job_shapes(&failed), (1, 2));
        assert_eq!(*exports.copied.lock().unwrap(), [FrameShape::Portrait]);
        let between = self::review(&app, &project);
        assert!(shape(&between, FrameShape::Portrait).current);
        assert!(shape(&between, FrameShape::Square).last.is_none());

        app.retry_job(job).unwrap();
        done(&app, job);
        assert_eq!(
            *exports.copied.lock().unwrap(),
            [FrameShape::Portrait, FrameShape::Square]
        );
    }

    #[test]
    fn the_export_waits_for_the_product_in_use_check() {
        let h = Harness::new();
        let exports: Arc<NotedExports> = Arc::default();
        let mut app = h.start_with_exports(Arc::clone(&exports) as _);
        app.save_provider_key(bardo_domain::Provider::TypeSafe, TYPESAFE_KEY)
            .unwrap();
        let (_, project, _) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        plan_with(
            &h,
            &app,
            &project,
            answer([
                ("drawn", -1),
                ("product_shot", 1),
                ("product_in_use", -1),
                ("end_card", 0),
            ]),
        );
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        app.set_ad_texts(project.id, "Refill it.", "Refill it")
            .unwrap();
        let review = review(&app, &project);
        assert_eq!(review.unchecked_products, [2]);
        assert!(!review.ready());
        assert!(matches!(
            app.start_placement_export(&review, &[FrameShape::Square], &review.publishing.keys()),
            Err(PlacementError::ProductUnchecked)
        ));
    }

    #[test]
    fn a_project_that_is_no_ad_has_no_export_for_ads() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        assert!(app.placement_review(project.id).unwrap().is_none());
        assert!(matches!(
            app.set_ad_texts(project.id, "a", "b"),
            Err(PlacementError::NotAnAd)
        ));
    }

    #[test]
    fn no_caption_shows_over_the_end_card() {
        let line = |at: u64, duration: u64, words: &[u64]| CaptionLine {
            text: "a line".into(),
            at: Duration::from_millis(at),
            duration: Duration::from_millis(duration),
            words: words.iter().map(|w| Duration::from_millis(*w)).collect(),
        };
        let mut plan = RenderPlan {
            video: Vec::new(),
            audio: Vec::new(),
            captions: Some(CaptionTrack {
                style: CaptionStyle::Clean,
                highlight: bardo_domain::CaptionHighlight::Off,
                lines: vec![
                    line(0, 1_000, &[]),
                    line(1_000, 1_500, &[1_000, 2_200]),
                    line(2_500, 500, &[]),
                ],
            }),
            skipped: Duration::ZERO,
        };
        without_captions_from(&mut plan, Duration::from_millis(2_000));
        let lines = plan.captions.unwrap().lines;
        assert_eq!(lines, [line(0, 1_000, &[]), line(1_000, 1_000, &[1_000])]);
    }
}
