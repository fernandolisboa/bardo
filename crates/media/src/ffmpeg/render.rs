//! Rendering a timeline description: one video track of clips back to
//! back, audio tracks of clips placed on the timeline, cropped or fitted to
//! the output frame, mixed, and normalized to a loudness target. Audio
//! clips fade in and out, and a track can duck under a level envelope (the
//! music under the narration). Video clips join with a cut or a transition
//! (`xfade`) centered on the cut. A still can move: a slow zoom or pan
//! over its framed picture (`zoompan`). Captions are burned in over the
//! joined picture (`captions`).
//!
//! The same filter graph feeds preview (raw frames on a pipe, from the
//! playhead on) and the final render (an MP4 file), so what the preview
//! shows is what the render writes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::captions::{CaptionFiles, CaptionTrack, escape_graph, escape_option, path_option};
use super::frames::{FrameSize, FrameStream};
use super::process::{self, Span};
use super::{
    Ffmpeg, MediaError, Monitor, VideoEncoder, partial_path, path_arg, remove_partial, seconds,
};
use crate::PcmStream;
use bardo_domain::TransitionKind;

/// Every audio stream is mixed at this rate and layout.
const SAMPLE_RATE: u32 = 48_000;

/// What to render. `media`'s own shape for now; the editor's timeline in
/// `domain` (#20) maps onto it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderPlan {
    /// Played back to back; the timeline is as long as these together.
    pub video: Vec<VideoClip>,
    pub audio: Vec<AudioTrack>,
    /// Burned in over the picture, when there are any.
    pub captions: Option<CaptionTrack>,
    /// How much picture the video makes before the plan's start, left out
    /// of what it shows: a plan starting inside a transition makes its
    /// video from where the transition starts, so the transition plays on
    /// from where it was.
    #[serde(default, skip_serializing_if = "Duration::is_zero")]
    pub skipped: Duration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoClip {
    pub source: ClipSource,
    /// Where in the source the clip starts (ignored for stills and black).
    pub start: Duration,
    /// Rounded to whole frames of the output rate. A source shorter than
    /// this holds its last frame to the end.
    pub duration: Duration,
    pub framing: Framing,
    /// How the picture goes into this clip from the one before; `None` for
    /// a cut (and on the first clip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// How the view moves over a still's framed picture; ignored on video
    /// and black.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<Motion>,
}

/// A view moving over a still's framed picture, from `from` to `to` over
/// `length` of the clip's own time: from the first frame the clip makes
/// (the start of the transition into it, when there is one) on. `elapsed`
/// is how much of the move came before that frame: a plan starting inside
/// the clip moves on from where it was. Before the move and after it the
/// view holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    pub from: View,
    pub to: View,
    pub length: Duration,
    #[serde(default, skip_serializing_if = "Duration::is_zero")]
    pub elapsed: Duration,
}

/// A window of the frame's shape, `zoom` times smaller than the framed
/// picture (1.0 and up), placed `x` across (0.0 at the left edge, 1.0 at
/// the right) and `y` down the room it leaves.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
}

impl View {
    fn holds_together(&self) -> bool {
        (1.0..=10.0).contains(&self.zoom)
            && (0.0..=1.0).contains(&self.x)
            && (0.0..=1.0).contains(&self.y)
    }
}

/// How many times larger than the output a moving still is drawn before
/// the view is taken from it. `zoompan` places and sizes its window in
/// whole pixels of what it is given (in 4:4:4, so not in pairs), so a slow
/// move steps by up to half an output pixel times the zoom instead of a
/// whole one: smooth to the eye, where 3× costs twice the time and half
/// again the memory of 2× for a step hardly anyone sees (ADR-0019).
const MOTION_OVERSAMPLE: u32 = 2;

/// A transition into a clip, around the cut: the clip starts `before`
/// ahead of the cut, and the clip before it plays on `after` past the cut.
/// Each holds its edge frame where its source has no picture there. Each
/// part rounds to whole frames of the output rate; together they are no
/// longer than either clip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    #[serde(with = "transition_code")]
    pub kind: TransitionKind,
    pub before: Duration,
    pub after: Duration,
}

impl Transition {
    pub fn duration(&self) -> Duration {
        self.before + self.after
    }
}

/// A transition kind by its stored code, so a plan can be written down.
mod transition_code {
    use bardo_domain::TransitionKind;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        kind: &TransitionKind,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(kind.code())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<TransitionKind, D::Error> {
        let code = String::deserialize(deserializer)?;
        code.parse().map_err(serde::de::Error::custom)
    }
}

/// The `xfade` transition that draws `kind`.
fn xfade_name(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::Crossfade => "fade",
        TransitionKind::FadeBlack => "fadeblack",
        TransitionKind::FadeWhite => "fadewhite",
        TransitionKind::WipeLeft => "wipeleft",
        TransitionKind::WipeRight => "wiperight",
        TransitionKind::SlideLeft => "slideleft",
        TransitionKind::SlideRight => "slideright",
        TransitionKind::SlideUp => "slideup",
        TransitionKind::SlideDown => "slidedown",
        TransitionKind::ZoomIn => "zoomin",
    }
}

/// What a video clip shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipSource {
    /// A video file.
    Video(PathBuf),
    /// An image file, shown for the clip's whole length.
    Still(PathBuf),
    /// Black, where the timeline has nothing to show.
    Black,
}

/// How a source picture fills an output frame of another shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Framing {
    /// The largest window of the output's shape, placed `x` across (0.0 at
    /// the left edge, 1.0 at the right) and `y` down the source: a 9:16
    /// slice of a 16:9 clip moves along `x` only.
    Crop { x: f32, y: f32 },
    /// The whole picture, black bars around it.
    Fit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    pub clips: Vec<AudioClip>,
    pub gain_db: f32,
    /// Lowers the whole track where its dips are.
    pub duck: Option<Duck>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub source: PathBuf,
    /// Where in the source the clip starts.
    pub start: Duration,
    pub duration: Duration,
    /// Where on the timeline it plays.
    pub at: Duration,
    pub gain_db: f32,
    /// Linear fades from and to silence, over the clip as placed.
    pub fade_in: Duration,
    pub fade_out: Duration,
    /// How much of the clip as placed comes before `start`: a plan
    /// starting inside a clip leaves its head out, and its fade-in plays on
    /// from where it was.
    pub skipped: Duration,
}

/// A level envelope: down by `depth_db` in each dip, linear in decibels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Duck {
    pub depth_db: f32,
    /// In order and apart.
    pub dips: Vec<Dip>,
}

/// One dip: down from `start` to the full depth at `full`, held until
/// `release`, back up by `end`. Seconds on the plan's timeline; a plan
/// starting inside a dip has it begin before zero.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dip {
    pub start: f64,
    pub full: f64,
    pub release: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Output {
    pub size: FrameSize,
    pub fps: (u32, u32),
    pub encoder: VideoEncoder,
    /// Bits per second.
    pub video_bitrate: u32,
    pub audio_bitrate: u32,
}

/// A loudness target, as networks publish them (YouTube plays back at
/// about -14 LUFS).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessTarget {
    /// Integrated loudness, LUFS.
    pub integrated: f32,
    /// Maximum true peak, dBTP.
    pub true_peak: f32,
}

/// The loudness range `loudnorm` is given when measuring; the
/// measurements do not depend on it.
const MEASURED_RANGE: f32 = 11.0;

/// Measurements do not depend on the target, but `loudnorm` needs one.
const REFERENCE_TARGET: LoudnessTarget = LoudnessTarget {
    integrated: -14.0,
    true_peak: -1.0,
};

/// Loudness as ffmpeg's `loudnorm` measured it (EBU R128). Silence
/// measures as minus infinity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    pub integrated: f32,
    pub true_peak: f32,
    pub range: f32,
    pub threshold: f32,
    pub target_offset: f32,
}

impl Loudness {
    /// Whether there is no sound to speak of (`bardo_domain::SILENCE`).
    pub fn is_silent(&self) -> bool {
        !self.integrated.is_finite() || f64::from(self.integrated) < bardo_domain::SILENCE
    }
}

impl RenderPlan {
    /// How long the plan plays: its video, less what it skips.
    pub fn duration(&self) -> Duration {
        let made: Duration = self.video.iter().map(|clip| clip.duration).sum();
        made.saturating_sub(self.skipped)
    }

    /// Where each transition plays on the plan's timeline (before
    /// `skipped`), by the clip it leads into: from its start to its end.
    fn transition_spans(&self) -> Vec<(usize, Duration, Duration)> {
        let mut spans = Vec::new();
        let mut cut = Duration::ZERO;
        for (index, clip) in self.video.iter().enumerate() {
            if let Some(transition) = clip.transition.filter(|_| index > 0) {
                spans.push((
                    index,
                    cut.saturating_sub(transition.before),
                    cut + transition.after,
                ));
            }
            cut += clip.duration;
        }
        spans
    }

    /// The plan from `from` on, as if the timeline started there: what a
    /// preview starting at the playhead plays. Starting inside a
    /// transition, the video starts where the transition does and skips
    /// up to `from`.
    pub fn starting_at(&self, from: Duration) -> RenderPlan {
        let made_from = self.skipped + from;
        let video_from = self
            .transition_spans()
            .into_iter()
            .find(|&(_, start, end)| start < made_from && made_from < end)
            .map_or(made_from, |(_, start, _)| start);
        let mut video = Vec::new();
        let mut clip_start = Duration::ZERO;
        for (index, clip) in self.video.iter().enumerate() {
            let clip_end = clip_start + clip.duration;
            if clip_end > video_from {
                let skip = video_from.saturating_sub(clip_start);
                // The clip before it is gone, and its transition with it.
                let first = video.is_empty();
                let transition = clip.transition.filter(|_| !first);
                // The clip makes its first frame later: by what it skips,
                // and by the lead-in of a transition it no longer plays.
                let dropped = match clip.transition.filter(|_| index > 0 && first) {
                    Some(dropped) => dropped.before,
                    None => Duration::ZERO,
                };
                video.push(VideoClip {
                    start: clip.start + skip,
                    duration: clip.duration - skip,
                    transition,
                    motion: clip.motion.map(|motion| Motion {
                        elapsed: motion.elapsed + dropped + skip,
                        ..motion
                    }),
                    ..clip.clone()
                });
            }
            clip_start = clip_end;
        }
        let from = made_from - self.skipped;
        let audio = self
            .audio
            .iter()
            .map(|track| AudioTrack {
                clips: track
                    .clips
                    .iter()
                    .filter(|clip| clip.at + clip.duration > from)
                    .map(|clip| {
                        let skip = from.saturating_sub(clip.at);
                        AudioClip {
                            start: clip.start + skip,
                            duration: clip.duration - skip,
                            at: clip.at.saturating_sub(from),
                            skipped: clip.skipped + skip,
                            ..clip.clone()
                        }
                    })
                    .collect(),
                gain_db: track.gain_db,
                duck: track.duck.as_ref().map(|duck| {
                    let shift = from.as_secs_f64();
                    Duck {
                        depth_db: duck.depth_db,
                        dips: duck
                            .dips
                            .iter()
                            .filter(|dip| dip.end > shift)
                            .map(|dip| Dip {
                                start: dip.start - shift,
                                full: dip.full - shift,
                                release: dip.release - shift,
                                end: dip.end - shift,
                            })
                            .collect(),
                    }
                }),
            })
            .collect();
        RenderPlan {
            video,
            audio,
            captions: self
                .captions
                .as_ref()
                .map(|captions| captions.starting_at(from)),
            skipped: made_from - video_from,
        }
    }

    fn check(&self) -> Result<(), MediaError> {
        if self.video.is_empty() {
            return Err(MediaError::InvalidPlan("no video clips"));
        }
        if self.video.iter().any(|clip| clip.duration.is_zero()) {
            return Err(MediaError::InvalidPlan("a video clip has no length"));
        }
        let bad_crop = self.video.iter().any(|clip| {
            matches!(clip.framing, Framing::Crop { x, y }
                if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y))
        });
        if bad_crop {
            return Err(MediaError::InvalidPlan("crop window outside the picture"));
        }
        let bad_motion = self
            .video
            .iter()
            .filter_map(|clip| clip.motion)
            .any(|motion| {
                motion.length.is_zero()
                    || !motion.from.holds_together()
                    || !motion.to.holds_together()
            });
        if bad_motion {
            return Err(MediaError::InvalidPlan("a motion goes outside the picture"));
        }
        // A clip's own time holds the end of the transition into it and
        // the start of the one out of it, and they must not overlap.
        let overlapping = self.video.iter().enumerate().any(|(index, clip)| {
            let into = clip
                .transition
                .filter(|_| index > 0)
                .map_or(Duration::ZERO, |t| t.after);
            let out = self
                .video
                .get(index + 1)
                .and_then(|next| next.transition)
                .map_or(Duration::ZERO, |t| t.before);
            into + out > clip.duration
        });
        if overlapping {
            return Err(MediaError::InvalidPlan("transitions overlap inside a clip"));
        }
        Ok(())
    }

    fn audio_clips(&self) -> impl Iterator<Item = (&AudioClip, f32, Option<&Duck>)> {
        self.audio.iter().flat_map(|track| {
            track
                .clips
                .iter()
                .filter(|clip| !clip.duration.is_zero())
                .map(move |clip| (clip, track.gain_db + clip.gain_db, track.duck.as_ref()))
        })
    }
}

/// One `-ss … -t … -i …` input.
#[derive(Debug, Clone, PartialEq)]
struct Input<'a> {
    source: InputSource<'a>,
    start: Duration,
    duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum InputSource<'a> {
    File(&'a Path),
    /// An image repeated at the rate given.
    Still(&'a Path, (u32, u32)),
    /// A black picture at the rate given.
    Black((u32, u32)),
}

/// Whole frames of `duration` at `fps`, rounded to the nearest.
fn frame_count(duration: Duration, (numerator, denominator): (u32, u32)) -> u64 {
    let numerator = u128::from(numerator);
    let denominator = u128::from(denominator.max(1));
    let nanos = duration.as_nanos();
    ((nanos * numerator + denominator * 500_000_000) / (denominator * 1_000_000_000)) as u64
}

/// `frames` frames at `fps`, in seconds as ffmpeg reads them.
fn frame_seconds(frames: u64, (numerator, denominator): (u32, u32)) -> String {
    let seconds = frames as f64 * f64::from(denominator) / f64::from(numerator.max(1));
    format!("{seconds:.6}")
}

/// The filters that make a picture of `width` × `height` from a source
/// of any shape, as `framing` says.
fn framing_filter(framing: Framing, width: u32, height: u32) -> String {
    match framing {
        Framing::Crop { x, y } => format!(
            "crop=w='min(iw,ih*{width}/{height})':h='min(ih,iw*{height}/{width})':\
             x='(iw-ow)*{x}':y='(ih-oh)*{y}',scale={width}:{height}:flags=bicubic"
        ),
        Framing::Fit => format!(
            "scale={width}:{height}:force_original_aspect_ratio=decrease:flags=bicubic,\
             pad={width}:{height}:(ow-iw)/2:(oh-ih)/2"
        ),
    }
}

/// The `zoompan` that makes `frames` frames of `size` from one framed
/// picture, the view following `motion`: its zoom changes by the same
/// ratio each frame and its place moves at an even pace (as
/// `bardo_domain::MotionView::toward` does). `on` counts the frames made.
fn zoompan(motion: &Motion, frames: u64, size: FrameSize, fps: (u32, u32)) -> String {
    let (numerator, denominator) = fps;
    let FrameSize { width, height } = size;
    let (from, to) = (motion.from, motion.to);
    let span = frame_count(motion.length, fps).saturating_sub(1);
    let progress = if span == 0 {
        "0".to_owned()
    } else {
        format!("clip((on+{})/{span},0,1)", frame_count(motion.elapsed, fps))
    };
    let ratio = to.zoom / from.zoom;
    let zoom = format!("{:.6}*pow({ratio:.6},{progress})", from.zoom);
    let place = |from: f64, to: f64| format!("{from:.6}+{:.6}*{progress}", to - from);
    format!(
        "zoompan=z='{zoom}':x='(iw-iw/zoom)*({})':y='(ih-ih/zoom)*({})':d={frames}:\
         s={width}x{height}:fps={numerator}/{denominator}",
        place(from.x, to.x),
        place(from.y, to.y),
    )
}

/// The `movie` source reading `path`, escaped for the filter graph.
fn movie(path: &Path) -> String {
    let arguments = format!("filename={}", escape_option(&path_option(path)));
    format!("movie={}", escape_graph(&arguments))
}

/// Inputs and the filter graph that turns them into `[vout]`: each clip
/// trimmed, framed and set to the output rate, then joined with a cut or
/// its transition, with the captions (`overlay`: a filter such as
/// `subtitles=…`) drawn over them.
///
/// A clip with a transition on either side runs longer than its place on
/// the timeline: on into the clip after it (`after`) and from before the
/// clip before it ends (`before`). `xfade` lays the two over each other
/// there, so the cut stays where it was and the timeline as long.
fn video_graph<'a>(
    plan: &'a RenderPlan,
    size: FrameSize,
    fps: (u32, u32),
    pixel_format: &str,
    overlay: Option<&str>,
) -> (Vec<Input<'a>>, String) {
    let FrameSize { width, height } = size;
    let (numerator, denominator) = fps;
    let mut inputs = Vec::new();
    let mut chains = Vec::new();
    for (index, clip) in plan.video.iter().enumerate() {
        let into = clip.transition.filter(|_| index > 0);
        let before = into.map_or(Duration::ZERO, |t| t.before);
        let after = plan
            .video
            .get(index + 1)
            .and_then(|next| next.transition)
            .map_or(Duration::ZERO, |t| t.after);
        // A source with less picture before its start than the transition
        // needs holds its first frame for the rest.
        let (start, held) = match clip.source {
            ClipSource::Video(_) => (
                clip.start.saturating_sub(before),
                frame_count(before.saturating_sub(clip.start), fps),
            ),
            ClipSource::Still(_) | ClipSource::Black => (clip.start, 0),
        };
        let frames = frame_count(clip.duration, fps).max(1)
            + frame_count(before, fps)
            + frame_count(after, fps);
        // A moving still: its picture once (the first frame of an animated
        // one), framed larger than the output, then the view taken from it
        // frame by frame. Read by `movie` rather than as an input, so it is
        // opened when its clip plays: an input's first frame goes through
        // the graph as ffmpeg starts, and every moving still would be
        // drawn large before the first frame came out.
        if let (ClipSource::Still(path), Some(motion)) = (&clip.source, clip.motion) {
            let large = framing_filter(
                clip.framing,
                width * MOTION_OVERSAMPLE,
                height * MOTION_OVERSAMPLE,
            );
            chains.push(format!(
                "{},trim=end_frame=1,{large},setsar=1,format=yuv444p,{},setsar=1,\
                 format=yuv420p[v{index}]",
                movie(path),
                zoompan(&motion, frames, size, fps),
            ));
            continue;
        }
        let input = inputs.len();
        inputs.push(Input {
            source: match &clip.source {
                ClipSource::Video(path) => InputSource::File(path),
                ClipSource::Still(path) => InputSource::Still(path, fps),
                ClipSource::Black => InputSource::Black(fps),
            },
            start,
            duration: before + clip.duration + after,
        });
        let framing = framing_filter(clip.framing, width, height);
        let held = if held > 0 {
            format!("start={held}:start_mode=clone:")
        } else {
            String::new()
        };
        // A source that ends early holds its last frame (`tpad`), so every
        // clip is exactly as many frames long as asked and the clips after
        // it stay in time with the audio.
        chains.push(format!(
            "[{input}:v:0]settb=AVTB,setpts=PTS-STARTPTS,fps={numerator}/{denominator},\
             tpad={held}stop=-1:stop_mode=clone,trim=end_frame={frames},{framing},setsar=1,format=yuv420p[v{index}]"
        ));
    }
    // Runs of clips joined by transitions, each one stream; cuts join the
    // runs.
    let mut runs: Vec<String> = Vec::new();
    let mut run_frames = 0;
    for (index, clip) in plan.video.iter().enumerate() {
        let frames = frame_count(clip.duration, fps).max(1);
        match clip.transition.filter(|_| index > 0) {
            Some(transition) => {
                let after = frame_count(transition.after, fps);
                let length = frame_count(transition.before, fps) + after;
                let joined = runs.pop().expect("a clip before the transition");
                // The clip comes in `length` before the run's end.
                chains.push(format!(
                    "{joined}[v{index}]xfade=transition={}:duration={}:offset={}[x{index}]",
                    xfade_name(transition.kind),
                    frame_seconds(length, fps),
                    frame_seconds(run_frames - length, fps),
                ));
                runs.push(format!("[x{index}]"));
                run_frames = run_frames - after + frames;
            }
            None => {
                runs.push(format!("[v{index}]"));
                run_frames = frames;
            }
        }
        // The run reaches on into the clip after when it has a transition.
        run_frames += plan
            .video
            .get(index + 1)
            .and_then(|next| next.transition)
            .map_or(0, |t| frame_count(t.after, fps));
    }
    let skip = frame_count(plan.skipped, fps);
    let skip = if skip > 0 {
        format!("trim=start_frame={skip},setpts=PTS-STARTPTS,")
    } else {
        String::new()
    };
    let overlay = overlay
        .map(|filter| format!("{filter},"))
        .unwrap_or_default();
    let labels = runs.concat();
    chains.push(if runs.len() == 1 {
        format!("{labels}{skip}{overlay}format={pixel_format}[vout]")
    } else {
        format!(
            "{labels}concat=n={}:v=1:a=0,{skip}{overlay}format={pixel_format}[vout]",
            runs.len()
        )
    });
    (inputs, chains.join(";"))
}

/// A clip's fades, in its own time: the fade-in on from where a plan
/// starting inside it left it, the fade-out from where it begins (or, when
/// the clip starts inside it, from the level it had reached).
fn fades(clip: &AudioClip) -> String {
    let mut filters = String::new();
    let fade_in = clip.fade_in.as_secs_f64();
    let done = clip.skipped.as_secs_f64();
    if fade_in > done {
        filters.push_str(&format!(
            ",afade=t=in:st=0:d={:.6}:silence={:.6}",
            fade_in - done,
            done / fade_in
        ));
    }
    let fade_out = clip.fade_out.as_secs_f64();
    let length = clip.duration.as_secs_f64();
    if fade_out > 0.0 {
        if fade_out <= length {
            filters.push_str(&format!(
                ",afade=t=out:st={:.6}:d={fade_out:.6}",
                length - fade_out
            ));
        } else {
            filters.push_str(&format!(
                ",afade=t=out:st=0:d={length:.6}:unity={:.6}",
                length / fade_out
            ));
        }
    }
    filters
}

/// The level envelope of `duck` over the timeline span `from`-`to` (in
/// seconds), as a `volume` expression of the timeline time `t`; `None`
/// where no dip reaches.
fn duck_expression(duck: &Duck, from: f64, to: f64) -> Option<String> {
    // Each dip adds how far down it is, from 0 to 1; dips never overlap.
    let ramp = |start: f64, end: f64| {
        if end > start {
            format!("clip((t{:+.4})/{:.4},0,1)", -start, end - start)
        } else {
            format!("gte(t,{start:.4})")
        }
    };
    let terms: Vec<String> = duck
        .dips
        .iter()
        .filter(|dip| dip.end > from && dip.start < to)
        .map(|dip| {
            format!(
                "{}-{}",
                ramp(dip.start, dip.full),
                ramp(dip.release, dip.end)
            )
        })
        .collect();
    (!terms.is_empty())
        .then(|| format!("pow(10,{:.4}*({}))", -duck.depth_db / 20.0, terms.join("+")))
}

/// The mix of every audio clip, as long as the video, labelled `[amix]`.
/// `first_input` is the index of the first audio input.
fn audio_graph(plan: &RenderPlan, first_input: usize) -> (Vec<Input<'_>>, String) {
    let total = seconds(plan.duration());
    let mut inputs = Vec::new();
    let mut chains = Vec::new();
    for (index, (clip, gain_db, duck)) in plan.audio_clips().enumerate() {
        inputs.push(Input {
            source: InputSource::File(&clip.source),
            start: clip.start,
            duration: clip.duration,
        });
        let delay = (clip.at.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as u64;
        // After the delay the clip's time is the timeline's. Ten
        // milliseconds a step keeps the envelope's ramps smooth.
        let ducked = duck
            .and_then(|duck| {
                let at = clip.at.as_secs_f64();
                duck_expression(duck, at, at + clip.duration.as_secs_f64())
            })
            .map(|volume| format!(",asetnsamples=n=480,volume=eval=frame:volume='{volume}'"))
            .unwrap_or_default();
        chains.push(format!(
            "[{}:a:0]asetpts=PTS-STARTPTS,\
             aformat=sample_fmts=fltp:sample_rates={SAMPLE_RATE}:channel_layouts=stereo,\
             volume={gain_db}dB{},adelay=delays={delay}S:all=1{ducked}[a{index}]",
            first_input + index,
            fades(clip)
        ));
    }
    let fit = format!("apad=whole_dur={total},atrim=duration={total}");
    chains.push(match inputs.len() {
        0 => format!("anullsrc=r={SAMPLE_RATE}:cl=stereo,atrim=duration={total}[amix]"),
        1 => format!("[a0]{fit}[amix]"),
        count => {
            let labels: String = (0..count).map(|index| format!("[a{index}]")).collect();
            format!(
                "{labels}amix=inputs={count}:duration=longest:dropout_transition=0:normalize=0,{fit}[amix]"
            )
        }
    });
    (inputs, chains.join(";"))
}

fn loudnorm(target: LoudnessTarget) -> String {
    format!(
        "loudnorm=I={}:TP={}:LRA={MEASURED_RANGE}",
        target.integrated, target.true_peak
    )
}

/// The rate the true-peak limiter runs at: 4× oversampling, so the peaks
/// it sees are the true peaks between samples.
const OVERSAMPLED: u32 = 192_000;

/// The second pass: one gain to the target for the whole mix (no
/// pumping), then a limiter at 4× oversampling for the peaks that gain
/// pushes over the ceiling. `loudnorm`'s own second pass switches to
/// dynamic compression when the mix's range is wide or the gain lifts its
/// peaks, which flattens the mix as edited and still let peaks over the
/// ceiling through AAC.
fn normalize_filter(target: LoudnessTarget, measured: Loudness) -> String {
    let gain = target.integrated - measured.integrated;
    let ceiling = 10_f32.powf(target.true_peak / 20.0);
    format!(
        "volume={gain:.2}dB,aresample={OVERSAMPLED},\
         alimiter=limit={ceiling:.4}:level=disabled:attack=1:release=50,aresample={SAMPLE_RATE}"
    )
}

fn push_inputs(command: &mut std::process::Command, inputs: &[Input<'_>]) {
    for input in inputs {
        let length = seconds(input.duration);
        match input.source {
            InputSource::File(path) => {
                command
                    .args(["-ss", &seconds(input.start), "-t", &length, "-i"])
                    .arg(path_arg(path));
            }
            InputSource::Still(path, (numerator, denominator)) => {
                command
                    .args(["-loop", "1", "-framerate"])
                    .arg(format!("{numerator}/{denominator}"))
                    .args(["-t", &length, "-i"])
                    .arg(path_arg(path));
            }
            InputSource::Black((numerator, denominator)) => {
                command
                    .args(["-f", "lavfi", "-t", &length, "-i"])
                    .arg(format!("color=c=black:s=64x36:r={numerator}/{denominator}"));
            }
        }
    }
}

impl Ffmpeg {
    /// Loudness of a file's audio, as a review screen shows it.
    pub fn measure_loudness(
        &self,
        path: &Path,
        monitor: &dyn Monitor,
    ) -> Result<Loudness, MediaError> {
        let total = self.probe(path)?.duration;
        let reference = REFERENCE_TARGET;
        let mut command = self.ffmpeg();
        command
            .args(["-loglevel", "info", "-i"])
            .arg(path_arg(path))
            .args([
                "-vn",
                "-af",
                &format!("{}:print_format=json", loudnorm(reference)),
            ])
            .args(["-f", "null", "-"]);
        let log = process::run(command, monitor, Span::whole(total))?;
        parse_loudness(&log)
    }

    /// Loudness of the plan's audio mix before normalization, as the render
    /// review shows it. Audio only, so it takes a fraction of a render.
    pub fn measure_mix_loudness(
        &self,
        plan: &RenderPlan,
        monitor: &dyn Monitor,
    ) -> Result<Loudness, MediaError> {
        plan.check()?;
        self.measure_mix(plan, monitor, Span::whole(plan.duration()))
    }

    /// Loudness of the plan's audio mix before normalization.
    fn measure_mix(
        &self,
        plan: &RenderPlan,
        monitor: &dyn Monitor,
        span: Span,
    ) -> Result<Loudness, MediaError> {
        let (inputs, mix) = audio_graph(plan, 0);
        let mut command = self.ffmpeg();
        command.args(["-loglevel", "info"]);
        push_inputs(&mut command, &inputs);
        command
            .args([
                "-filter_complex",
                &format!(
                    "{mix};[amix]{}:print_format=json[aout]",
                    loudnorm(REFERENCE_TARGET)
                ),
            ])
            .args(["-map", "[aout]", "-f", "null", "-"]);
        let log = process::run(command, monitor, span)?;
        parse_loudness(&log)
    }

    /// Renders `plan` to an MP4 at `destination`. With a loudness target it
    /// first measures the mix (audio only, fast), then renders with one
    /// gain. Writes next to `destination` and renames when done, so a
    /// cancelled or failed render never leaves a file that looks finished.
    pub fn render(
        &self,
        plan: &RenderPlan,
        output: &Output,
        loudness: Option<LoudnessTarget>,
        destination: &Path,
        monitor: &dyn Monitor,
    ) -> Result<(), MediaError> {
        plan.check()?;
        let total = plan.duration();
        let measured = match loudness {
            Some(target) => Some((
                target,
                self.measure_mix(plan, monitor, Span::part(total, 0.0, 0.1))?,
            )),
            None => None,
        };
        // Silence has no loudness to bring anywhere: it renders as it is.
        let normalize = measured.filter(|(_, measured)| !measured.is_silent());

        let captions = CaptionFiles::write(plan.captions.as_ref(), output.size)?;
        let overlay = captions.as_ref().map(CaptionFiles::filter);
        let (mut inputs, video) =
            video_graph(plan, output.size, output.fps, "yuv420p", overlay.as_deref());
        let (audio_inputs, mix) = audio_graph(plan, inputs.len());
        inputs.extend(audio_inputs);
        let audio_out = match normalize {
            Some((target, measured)) => {
                format!("[amix]{}[aout]", normalize_filter(target, measured))
            }
            None => "[amix]anull[aout]".to_string(),
        };
        let (numerator, denominator) = output.fps;
        let keyframes = (2 * numerator).div_ceil(denominator.max(1));

        let partial = partial_path(destination);
        let mut command = self.ffmpeg();
        command.arg("-y");
        push_inputs(&mut command, &inputs);
        command
            .args(["-filter_complex", &format!("{video};{mix};{audio_out}")])
            .args(["-map", "[vout]", "-map", "[aout]"])
            .args(output.encoder.args(output.video_bitrate, keyframes))
            .args(["-r", &format!("{numerator}/{denominator}")])
            .args(["-c:a", "aac", "-b:a", &output.audio_bitrate.to_string()])
            .args(["-ar", &SAMPLE_RATE.to_string(), "-ac", "2"])
            .args([
                "-t",
                &seconds(total),
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ])
            .arg(path_arg(&partial));
        let span = if measured.is_some() {
            Span::part(total, 0.1, 1.0)
        } else {
            Span::whole(total)
        };
        match process::run(command, monitor, span) {
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

    /// Preview frames of `plan` from `from` on, scaled to `size`: the
    /// render's own video graph, written as raw BGRA instead of encoded.
    pub fn preview(
        &self,
        plan: &RenderPlan,
        from: Duration,
        size: FrameSize,
        fps: (u32, u32),
    ) -> Result<FrameStream, MediaError> {
        let plan = plan.starting_at(from);
        plan.check()?;
        let captions = CaptionFiles::write(plan.captions.as_ref(), size)?;
        let overlay = captions.as_ref().map(CaptionFiles::filter);
        let (inputs, video) = video_graph(&plan, size, fps, "bgra", overlay.as_deref());
        let mut command = self.ffmpeg();
        push_inputs(&mut command, &inputs);
        command
            .args(["-filter_complex", &video, "-map", "[vout]"])
            .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"]);
        Ok(FrameStream::spawn(command, size, fps, from)?.keeping(captions))
    }

    /// The audio mix of `plan` from `from` on, as interleaved stereo f32
    /// samples at 48 kHz: what the preview plays along with its frames.
    pub fn preview_audio(
        &self,
        plan: &RenderPlan,
        from: Duration,
    ) -> Result<PcmStream, MediaError> {
        let plan = plan.starting_at(from);
        plan.check()?;
        let (inputs, mix) = audio_graph(&plan, 0);
        let mut command = self.ffmpeg();
        push_inputs(&mut command, &inputs);
        command
            .args(["-filter_complex", &mix, "-map", "[amix]"])
            .args(["-f", "f32le", "-ar", &SAMPLE_RATE.to_string(), "-ac", "2"])
            .arg("pipe:1");
        process::spawn_pcm(command, 2, SAMPLE_RATE)
    }
}

#[derive(Deserialize)]
struct LoudnormJson {
    input_i: String,
    input_tp: String,
    input_lra: String,
    input_thresh: String,
    target_offset: String,
}

/// The JSON block loudnorm prints at the end of its run.
fn parse_loudness(log: &str) -> Result<Loudness, MediaError> {
    let start = log
        .rfind('{')
        .ok_or_else(|| MediaError::Parse("no loudnorm summary".into()))?;
    let end = log[start..]
        .find('}')
        .ok_or_else(|| MediaError::Parse("no loudnorm summary".into()))?;
    let json: LoudnormJson = serde_json::from_str(&log[start..=start + end])
        .map_err(|error| MediaError::Parse(format!("loudnorm summary: {error}")))?;
    let number = |text: &str| {
        text.trim()
            .parse::<f32>()
            .map_err(|_| MediaError::Parse(format!("loudnorm value {text:?}")))
    };
    Ok(Loudness {
        integrated: number(&json.input_i)?,
        true_peak: number(&json.input_tp)?,
        range: number(&json.input_lra)?,
        threshold: number(&json.input_thresh)?,
        target_offset: number(&json.target_offset)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: f64) -> Duration {
        Duration::from_secs_f64(seconds)
    }

    fn video(name: &str, start: f64, duration: f64) -> VideoClip {
        VideoClip {
            source: ClipSource::Video(PathBuf::from(name)),
            start: secs(start),
            duration: secs(duration),
            framing: Framing::Crop { x: 0.5, y: 0.5 },
            transition: None,
            motion: None,
        }
    }

    fn audio(name: &str, start: f64, duration: f64, at: f64) -> AudioClip {
        AudioClip {
            source: PathBuf::from(name),
            start: secs(start),
            duration: secs(duration),
            at: secs(at),
            gain_db: 0.0,
            fade_in: Duration::ZERO,
            fade_out: Duration::ZERO,
            skipped: Duration::ZERO,
        }
    }

    fn plan() -> RenderPlan {
        RenderPlan {
            video: vec![video("a.mp4", 1.0, 2.0), video("b.mp4", 0.0, 3.0)],
            audio: vec![
                AudioTrack {
                    clips: vec![audio("voice.mp3", 0.0, 3.0, 0.5)],
                    gain_db: 0.0,
                    duck: None,
                },
                AudioTrack {
                    clips: vec![audio("music.mp3", 10.0, 5.0, 0.0)],
                    gain_db: -12.0,
                    duck: None,
                },
            ],
            captions: None,
            skipped: Duration::ZERO,
        }
    }

    #[test]
    fn timeline_is_as_long_as_its_video() {
        assert_eq!(plan().duration(), secs(5.0));
    }

    #[test]
    fn starting_inside_a_clip_trims_it_and_shifts_audio() {
        let later = plan().starting_at(secs(2.5));
        assert_eq!(later.video, vec![video("b.mp4", 0.5, 2.5)]);
        assert_eq!(later.duration(), secs(2.5));
        let skipped = |mut clip: AudioClip, by: f64| {
            clip.skipped = secs(by);
            clip
        };
        assert_eq!(
            later.audio[0].clips,
            vec![skipped(audio("voice.mp3", 2.0, 1.0, 0.0), 2.0)]
        );
        assert_eq!(
            later.audio[1].clips,
            vec![skipped(audio("music.mp3", 12.5, 2.5, 0.0), 2.5)]
        );
    }

    #[test]
    fn starting_before_a_clip_keeps_it_whole_and_moves_it_earlier() {
        let mut plan = plan();
        plan.audio[0].clips = vec![audio("voice.mp3", 0.0, 1.0, 4.0)];
        let later = plan.starting_at(secs(1.0));
        assert_eq!(later.video[0], video("a.mp4", 2.0, 1.0));
        assert_eq!(
            later.audio[0].clips,
            vec![audio("voice.mp3", 0.0, 1.0, 3.0)]
        );
    }

    #[test]
    fn clips_over_before_the_start_are_dropped() {
        let later = plan().starting_at(secs(4.0));
        assert_eq!(later.video.len(), 1);
        assert!(later.audio[0].clips.is_empty());
        assert!(plan().starting_at(secs(5.0)).video.is_empty());
    }

    #[test]
    fn video_graph_crops_each_clip_then_joins_them() {
        let plan = plan();
        let (inputs, graph) =
            video_graph(&plan, FrameSize::new(1080, 1920), (30, 1), "yuv420p", None);
        assert_eq!(inputs.len(), 2);
        assert_eq!(
            (inputs[0].start, inputs[0].duration),
            (secs(1.0), secs(2.0))
        );
        assert!(graph.starts_with(
            "[0:v:0]settb=AVTB,setpts=PTS-STARTPTS,fps=30/1,tpad=stop=-1:stop_mode=clone,trim=end_frame=60,\
             crop=w='min(iw,ih*1080/1920)':h='min(ih,iw*1920/1080)':x='(iw-ow)*0.5':y='(ih-oh)*0.5',\
             scale=1080:1920:flags=bicubic,setsar=1,format=yuv420p[v0];"
        ));
        assert!(graph.ends_with("[v0][v1]concat=n=2:v=1:a=0,format=yuv420p[vout]"));
    }

    fn transition(kind: TransitionKind, before: f64, after: f64) -> Option<Transition> {
        Some(Transition {
            kind,
            before: secs(before),
            after: secs(after),
        })
    }

    /// Three clips of 2, 3 and 2 s: a crossfade of 0.5 s into the second
    /// and a hard cut into the third.
    fn crossfaded() -> RenderPlan {
        let mut plan = plan();
        plan.video.push(video("c.mp4", 4.0, 2.0));
        plan.video[1].transition = transition(TransitionKind::Crossfade, 0.2, 0.3);
        plan
    }

    #[test]
    fn a_transition_overlaps_the_clips_around_their_cut() {
        let plan = crossfaded();
        assert_eq!(plan.duration(), secs(7.0), "the cuts stay where they were");
        let (inputs, graph) = video_graph(&plan, FrameSize::new(960, 540), (30, 1), "bgra", None);
        // The first clip plays on 0.3 s past its cut; the second starts
        // 0.2 s early, from further back in its file when it has picture
        // there, else holding its first frame.
        assert_eq!(
            (inputs[0].start, inputs[0].duration),
            (secs(1.0), secs(2.3))
        );
        assert_eq!(
            (inputs[1].start, inputs[1].duration),
            (secs(0.0), secs(3.2))
        );
        assert!(graph.contains("tpad=stop=-1:stop_mode=clone,trim=end_frame=69,"));
        assert!(
            graph.contains(
                "tpad=start=6:start_mode=clone:stop=-1:stop_mode=clone,trim=end_frame=96,"
            )
        );
        assert!(
            graph.contains("[v0][v1]xfade=transition=fade:duration=0.500000:offset=1.800000[x1]")
        );
        assert!(graph.ends_with("[x1][v2]concat=n=2:v=1:a=0,format=bgra[vout]"));
    }

    #[test]
    fn a_source_with_picture_before_its_start_plays_it_into_the_transition() {
        let mut plan = crossfaded();
        plan.video[2].start = secs(1.0);
        plan.video[2].transition = transition(TransitionKind::SlideUp, 0.5, 0.5);
        let (inputs, graph) = video_graph(&plan, FrameSize::new(960, 540), (30, 1), "bgra", None);
        assert_eq!(
            (inputs[2].start, inputs[2].duration),
            (secs(0.5), secs(2.5))
        );
        assert!(graph.contains(
            "[2:v:0]settb=AVTB,setpts=PTS-STARTPTS,fps=30/1,tpad=stop=-1:stop_mode=clone,\
             trim=end_frame=75,"
        ));
        // The third clip comes in half a second before its cut at 5 s.
        assert!(
            graph
                .contains("[x1][v2]xfade=transition=slideup:duration=1.000000:offset=4.500000[x2]")
        );
        assert!(graph.ends_with("[x2]format=bgra[vout]"));
    }

    #[test]
    fn starting_inside_a_transition_makes_it_from_its_start_and_skips_to_there() {
        let plan = crossfaded();
        // The crossfade plays 1.8-2.3 s.
        let inside = plan.starting_at(secs(2.1));
        assert_eq!(inside.skipped, secs(0.3));
        assert_eq!(inside.duration(), secs(4.9));
        assert_eq!(inside.video[0], video("a.mp4", 2.8, 0.2));
        assert_eq!(inside.video[1].transition, plan.video[1].transition);
        let (_, graph) = video_graph(&inside, FrameSize::new(960, 540), (30, 1), "bgra", None);
        assert!(
            graph.contains("[v0][v1]xfade=transition=fade:duration=0.500000:offset=0.000000[x1]")
        );
        assert!(graph.ends_with(
            "[x1][v2]concat=n=2:v=1:a=0,trim=start_frame=9,setpts=PTS-STARTPTS,format=bgra[vout]"
        ));
        // The audio and captions start at the playhead itself.
        assert_eq!(inside.audio[0].clips[0].start, secs(1.6));

        // At the transition's start it plays whole; past its end the
        // second clip starts the plan with a cut.
        let at_start = plan.starting_at(secs(1.8));
        assert_eq!(at_start.skipped, Duration::ZERO);
        assert!(at_start.video[1].transition.is_some());
        let after = plan.starting_at(secs(2.3));
        assert_eq!(after.skipped, Duration::ZERO);
        assert_eq!(after.video[0].transition, None);
        assert_eq!(after.video[0].start, secs(0.3));
    }

    #[test]
    fn plans_without_transitions_write_down_as_before() {
        // A render's fingerprint is its plan's JSON: renders made before
        // transitions stay current.
        let json = serde_json::to_value(plan()).unwrap();
        assert!(json.get("skipped").is_none());
        assert!(!json.to_string().contains("transition"));
        assert!(!json.to_string().contains("motion"));
        let written = serde_json::to_string(&crossfaded()).unwrap();
        assert!(written.contains("\"kind\":\"crossfade\""));
        let back: RenderPlan = serde_json::from_str(&written).unwrap();
        assert_eq!(back, crossfaded());
    }

    #[test]
    fn transitions_that_overlap_inside_a_clip_are_refused() {
        let mut plan = crossfaded();
        plan.video[2].transition = transition(TransitionKind::Crossfade, 2.8, 0.2);
        assert!(matches!(plan.check(), Err(MediaError::InvalidPlan(_))));
        plan.video[2].transition = transition(TransitionKind::Crossfade, 2.7, 0.2);
        assert!(plan.check().is_ok());
    }

    #[test]
    fn captions_are_drawn_over_the_joined_picture() {
        let (_, graph) = video_graph(
            &plan(),
            FrameSize::new(960, 540),
            (30, 1),
            "bgra",
            Some("subtitles=filename=c.ass"),
        );
        assert!(
            graph
                .ends_with("[v0][v1]concat=n=2:v=1:a=0,subtitles=filename=c.ass,format=bgra[vout]")
        );
        let mut single = plan();
        single.video.truncate(1);
        let (_, graph) = video_graph(
            &single,
            FrameSize::new(960, 540),
            (30, 1),
            "bgra",
            Some("subtitles=filename=c.ass"),
        );
        assert!(graph.ends_with("[v0]subtitles=filename=c.ass,format=bgra[vout]"));
    }

    #[test]
    fn starting_later_moves_the_captions_with_the_picture() {
        let mut plan = plan();
        plan.captions = Some(CaptionTrack {
            style: bardo_domain::CaptionStyle::Clean,
            highlight: bardo_domain::CaptionHighlight::Off,
            lines: vec![super::super::CaptionLine {
                text: "Hello".into(),
                at: secs(1.0),
                duration: secs(2.0),
                words: Vec::new(),
            }],
        });
        let later = plan.starting_at(secs(2.0));
        let lines = &later.captions.unwrap().lines;
        assert_eq!((lines[0].at, lines[0].duration), (secs(0.0), secs(1.0)));
    }

    #[test]
    fn stills_loop_and_black_comes_from_a_color_source() {
        let mut plan = plan();
        plan.video[0].source = ClipSource::Still(PathBuf::from("scene.png"));
        plan.video[1].source = ClipSource::Black;
        let (inputs, graph) = video_graph(&plan, FrameSize::new(960, 540), (30, 1), "bgra", None);
        let mut command = std::process::Command::new("ffmpeg");
        push_inputs(&mut command, &inputs);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-loop",
                "1",
                "-framerate",
                "30/1",
                "-t",
                "2.000000",
                "-i",
                "scene.png",
                "-f",
                "lavfi",
                "-t",
                "3.000000",
                "-i",
                "color=c=black:s=64x36:r=30/1",
            ]
        );
        assert!(graph.contains("trim=end_frame=90,"));
    }

    /// A pan to the right at 1.25×, over `length` seconds of the clip.
    fn pan(length: f64) -> Option<Motion> {
        Some(Motion {
            from: View {
                zoom: 1.25,
                x: 0.0,
                y: 0.5,
            },
            to: View {
                zoom: 1.25,
                x: 1.0,
                y: 0.5,
            },
            length: secs(length),
            elapsed: Duration::ZERO,
        })
    }

    #[test]
    fn a_moving_still_is_framed_large_once_and_its_view_taken_frame_by_frame() {
        let mut plan = plan();
        plan.video[0].source = ClipSource::Still(PathBuf::from("scene.png"));
        plan.video[0].motion = pan(2.0);
        let (inputs, graph) =
            video_graph(&plan, FrameSize::new(1080, 1920), (30, 1), "yuv420p", None);
        // Read by the graph when its clip plays, not as an input: the clip
        // after it is input 0.
        assert_eq!(inputs.len(), 1);
        assert!(graph.contains("[0:v:0]settb=AVTB"), "{graph}");
        assert!(
            graph.starts_with(
                "movie=filename=scene.png,trim=end_frame=1,\
                 crop=w='min(iw,ih*2160/3840)':h='min(ih,iw*3840/2160)':\
                 x='(iw-ow)*0.5':y='(ih-oh)*0.5',scale=2160:3840:flags=bicubic,\
                 setsar=1,format=yuv444p,\
                 zoompan=z='1.250000*pow(1.000000,clip((on+0)/59,0,1))':\
                 x='(iw-iw/zoom)*(0.000000+1.000000*clip((on+0)/59,0,1))':\
                 y='(ih-ih/zoom)*(0.500000+0.000000*clip((on+0)/59,0,1))':\
                 d=60:s=1080x1920:fps=30/1,setsar=1,format=yuv420p[v0];"
            ),
            "{graph}"
        );
        // A path as the graph reads it: both levels escaped.
        assert_eq!(
            movie(Path::new(r"C:\Media\it's [a],b;c.png")),
            r"movie=filename=C\\:/Media/it\\\'s \[a\]\,b\;c.png"
        );
    }

    #[test]
    fn a_moving_still_moves_through_its_transitions_and_holds_outside_its_move() {
        let mut plan = crossfaded();
        plan.video[1].source = ClipSource::Still(PathBuf::from("scene.png"));
        // The whole time it shows: 0.2 s in, 3 s, 0.3 s out (none here).
        plan.video[1].motion = pan(3.2);
        let (_, graph) = video_graph(&plan, FrameSize::new(960, 540), (30, 1), "bgra", None);
        assert!(graph.contains("clip((on+0)/95,0,1)"), "{graph}");
        assert!(graph.contains(":d=96:s=960x540:"), "{graph}");
        // A move of one frame holds its first view.
        plan.video[1].motion = pan(0.01);
        let (_, graph) = video_graph(&plan, FrameSize::new(960, 540), (30, 1), "bgra", None);
        assert!(
            graph.contains("zoompan=z='1.250000*pow(1.000000,0)'"),
            "{graph}"
        );
    }

    #[test]
    fn video_and_black_ignore_a_motion() {
        let mut plan = plan();
        plan.video[0].motion = pan(2.0);
        plan.video[1].source = ClipSource::Black;
        plan.video[1].motion = pan(3.0);
        let (_, graph) = video_graph(&plan, FrameSize::new(960, 540), (30, 1), "bgra", None);
        assert!(!graph.contains("zoompan"), "{graph}");
    }

    #[test]
    fn a_preview_starting_inside_a_moving_still_moves_on_from_there() {
        let mut plan = crossfaded();
        plan.video[1].source = ClipSource::Still(PathBuf::from("scene.png"));
        plan.video[1].motion = pan(3.2);
        // Inside the crossfade: the clip still starts with it, unmoved.
        let inside = plan.starting_at(secs(2.1));
        assert_eq!(inside.video[1].motion, plan.video[1].motion);
        // Past it: 1 s into the clip, plus the 0.2 s lead-in it no longer
        // makes.
        let later = plan.starting_at(secs(3.0));
        assert_eq!(later.video[0].motion.unwrap().elapsed, secs(1.2));
        let (_, graph) = video_graph(&later, FrameSize::new(960, 540), (30, 1), "bgra", None);
        assert!(graph.contains("clip((on+36)/95,0,1)"), "{graph}");
        // From a playhead already moved on, further on.
        let further = later.starting_at(secs(0.5));
        assert_eq!(further.video[0].motion.unwrap().elapsed, secs(1.7));
    }

    #[test]
    fn a_motion_outside_the_picture_is_refused() {
        let mut plan = plan();
        plan.video[0].source = ClipSource::Still(PathBuf::from("scene.png"));
        plan.video[0].motion = pan(2.0);
        assert!(plan.check().is_ok());
        let mut far = plan.clone();
        far.video[0].motion.as_mut().unwrap().to.x = 1.5;
        assert!(far.check().is_err());
        let mut wide = plan.clone();
        wide.video[0].motion.as_mut().unwrap().from.zoom = 0.5;
        assert!(wide.check().is_err());
        let mut still = plan;
        still.video[0].motion.as_mut().unwrap().length = Duration::ZERO;
        assert!(still.check().is_err());
    }

    #[test]
    fn frame_counts_round_to_the_nearest_frame() {
        assert_eq!(frame_count(secs(2.0), (30, 1)), 60);
        assert_eq!(frame_count(Duration::from_nanos(33_333_333), (30, 1)), 1);
        assert_eq!(frame_count(secs(1.01), (30, 1)), 30);
        assert_eq!(frame_count(secs(1.02), (30, 1)), 31);
        assert_eq!(frame_count(secs(1.0), (30_000, 1_001)), 30);
    }

    #[test]
    fn fit_pads_instead_of_cropping() {
        let mut plan = plan();
        plan.video.truncate(1);
        plan.video[0].framing = Framing::Fit;
        let (_, graph) = video_graph(&plan, FrameSize::new(1080, 1920), (30, 1), "bgra", None);
        assert!(graph.contains("force_original_aspect_ratio=decrease"));
        assert!(graph.contains("pad=1080:1920:(ow-iw)/2:(oh-ih)/2"));
        assert!(graph.ends_with("[v0]format=bgra[vout]"));
    }

    #[test]
    fn audio_graph_places_gains_and_mixes_to_the_video_length() {
        let plan = plan();
        let (inputs, graph) = audio_graph(&plan, 2);
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[1].start, secs(10.0));
        assert!(graph.contains("[2:a:0]asetpts=PTS-STARTPTS,"));
        assert!(graph.contains("volume=0dB,adelay=delays=24000S:all=1[a0]"));
        assert!(graph.contains("[3:a:0]"));
        assert!(graph.contains("volume=-12dB,adelay=delays=0S:all=1[a1]"));
        assert!(graph.ends_with(
            "[a0][a1]amix=inputs=2:duration=longest:dropout_transition=0:normalize=0,\
             apad=whole_dur=5.000000,atrim=duration=5.000000[amix]"
        ));
    }

    #[test]
    fn no_audio_is_silence_as_long_as_the_video() {
        let mut plan = plan();
        plan.audio.clear();
        let (inputs, graph) = audio_graph(&plan, 2);
        assert!(inputs.is_empty());
        assert_eq!(
            graph,
            "anullsrc=r=48000:cl=stereo,atrim=duration=5.000000[amix]"
        );
    }

    #[test]
    fn plans_without_video_or_with_bad_crops_are_refused() {
        let mut empty = plan();
        empty.video.clear();
        assert!(matches!(empty.check(), Err(MediaError::InvalidPlan(_))));
        let mut bad = plan();
        bad.video[0].framing = Framing::Crop { x: 1.5, y: 0.5 };
        assert!(matches!(bad.check(), Err(MediaError::InvalidPlan(_))));
        assert!(plan().check().is_ok());
    }

    #[test]
    fn reads_the_loudnorm_summary() {
        let log = r#"[Parsed_loudnorm_0 @ 0x1]
{
	"input_i" : "-23.54",
	"input_tp" : "-7.12",
	"input_lra" : "1.20",
	"input_thresh" : "-33.80",
	"output_i" : "-14.10",
	"output_tp" : "-1.00",
	"output_lra" : "1.10",
	"output_thresh" : "-24.30",
	"normalization_type" : "dynamic",
	"target_offset" : "0.10"
}
"#;
        let loudness = parse_loudness(log).unwrap();
        assert_eq!(loudness.integrated, -23.54);
        assert_eq!(loudness.true_peak, -7.12);
        assert_eq!(loudness.threshold, -33.8);
        assert_eq!(loudness.target_offset, 0.1);
        assert!(parse_loudness("no summary").is_err());
    }

    #[test]
    fn second_pass_applies_one_gain_and_limits_true_peaks() {
        let target = LoudnessTarget {
            integrated: -14.0,
            true_peak: -1.5,
        };
        let measured = Loudness {
            integrated: -23.5,
            true_peak: -7.1,
            range: 1.2,
            threshold: -33.8,
            target_offset: 0.1,
        };
        assert_eq!(
            normalize_filter(target, measured),
            "volume=9.50dB,aresample=192000,\
             alimiter=limit=0.8414:level=disabled:attack=1:release=50,aresample=48000"
        );
    }

    fn dip(start: f64, full: f64, release: f64, end: f64) -> Dip {
        Dip {
            start,
            full,
            release,
            end,
        }
    }

    #[test]
    fn clips_fade_in_and_out_over_their_length() {
        let mut clip = audio("voice.mp3", 0.0, 3.0, 0.5);
        assert_eq!(fades(&clip), "");
        clip.fade_in = secs(0.5);
        clip.fade_out = secs(1.0);
        assert_eq!(
            fades(&clip),
            ",afade=t=in:st=0:d=0.500000:silence=0.000000\
             ,afade=t=out:st=2.000000:d=1.000000"
        );
    }

    #[test]
    fn a_plan_starting_inside_a_fade_plays_it_on_from_its_level() {
        let mut plan = plan();
        let clip = &mut plan.audio[0].clips[0];
        clip.fade_in = secs(1.0);
        clip.fade_out = secs(2.0);
        // The voice plays 0.5-3.5 s; from 0.75 s a quarter of its fade-in
        // is behind it.
        let later = plan.starting_at(secs(0.75));
        assert_eq!(
            fades(&later.audio[0].clips[0]),
            ",afade=t=in:st=0:d=0.750000:silence=0.250000\
             ,afade=t=out:st=0.750000:d=2.000000"
        );
        // From 2 s it starts three quarters into its fade-out.
        let later = plan.starting_at(secs(3.0));
        assert_eq!(
            fades(&later.audio[0].clips[0]),
            ",afade=t=out:st=0:d=0.500000:unity=0.250000"
        );
    }

    #[test]
    fn a_ducked_track_follows_its_envelope_on_the_timeline() {
        let mut plan = plan();
        plan.audio[1].duck = Some(Duck {
            depth_db: 12.0,
            dips: vec![dip(0.35, 0.5, 3.5, 4.0), dip(8.0, 8.0, 9.0, 9.5)],
        });
        let (_, graph) = audio_graph(&plan, 0);
        assert!(graph.contains(
            "adelay=delays=0S:all=1,asetnsamples=n=480,volume=eval=frame:\
             volume='pow(10,-0.6000*(clip((t-0.3500)/0.1500,0,1)-clip((t-3.5000)/0.5000,0,1)))'[a1]"
        ));
        assert!(
            !graph.contains("t-8.0000"),
            "a dip past the clip is left out"
        );
        assert!(
            graph.contains("volume=0dB,adelay=delays=24000S:all=1[a0]"),
            "the voice is not ducked"
        );

        // From 3.75 s the dip is half way back up and began before zero.
        let later = plan.starting_at(secs(3.75));
        let duck = later.audio[1].duck.as_ref().unwrap();
        assert_eq!(
            duck_expression(duck, 0.0, 1.25).unwrap(),
            "pow(10,-0.6000*(clip((t+3.4000)/0.1500,0,1)-clip((t+0.2500)/0.5000,0,1)))"
        );
        let dropped = plan.starting_at(secs(4.5));
        assert_eq!(dropped.audio[1].duck.as_ref().unwrap().dips.len(), 1);
    }

    #[test]
    fn a_dip_without_a_ramp_steps() {
        let duck = Duck {
            depth_db: 6.0,
            dips: vec![dip(0.0, 0.0, 1.0, 1.5)],
        };
        assert_eq!(
            duck_expression(&duck, 0.0, 2.0).unwrap(),
            "pow(10,-0.3000*(gte(t,0.0000)-clip((t-1.0000)/0.5000,0,1)))"
        );
        assert_eq!(duck_expression(&duck, 1.5, 3.0), None);
    }
}
