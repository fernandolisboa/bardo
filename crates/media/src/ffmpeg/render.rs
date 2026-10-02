//! Rendering a timeline description: one video track of clips back to
//! back, audio tracks of clips placed on the timeline, cropped or fitted to
//! the output frame, mixed, and normalized to a loudness target.
//!
//! The same filter graph feeds preview (raw frames on a pipe, from the
//! playhead on) and the final render (an MP4 file), so what the preview
//! shows is what the render writes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::frames::{FrameSize, FrameStream};
use super::process::{self, Span};
use super::{Ffmpeg, MediaError, Monitor, VideoEncoder, partial_path, path_arg, seconds};

/// Every audio stream is mixed at this rate and layout.
const SAMPLE_RATE: u32 = 48_000;

/// What to render. `media`'s own shape for now; the editor's timeline in
/// `domain` (#20) maps onto it.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPlan {
    /// Played back to back; the timeline is as long as these together.
    pub video: Vec<VideoClip>,
    pub audio: Vec<AudioTrack>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoClip {
    pub source: PathBuf,
    /// Where in the source the clip starts.
    pub start: Duration,
    pub duration: Duration,
    pub framing: Framing,
}

/// How a source picture fills an output frame of another shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Framing {
    /// The largest window of the output's shape, placed `x` across (0.0 at
    /// the left edge, 1.0 at the right) and `y` down the source: a 9:16
    /// slice of a 16:9 clip moves along `x` only.
    Crop { x: f32, y: f32 },
    /// The whole picture, black bars around it.
    Fit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrack {
    pub clips: Vec<AudioClip>,
    pub gain_db: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    pub source: PathBuf,
    /// Where in the source the clip starts.
    pub start: Duration,
    pub duration: Duration,
    /// Where on the timeline it plays.
    pub at: Duration,
    pub gain_db: f32,
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
    /// Loudness range, LU.
    pub range: f32,
}

/// Loudness as ffmpeg's `loudnorm` measured it (EBU R128).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    pub integrated: f32,
    pub true_peak: f32,
    pub range: f32,
    pub threshold: f32,
    pub target_offset: f32,
}

impl RenderPlan {
    pub fn duration(&self) -> Duration {
        self.video.iter().map(|clip| clip.duration).sum()
    }

    /// The plan from `from` on, as if the timeline started there: what a
    /// preview starting at the playhead plays.
    pub fn starting_at(&self, from: Duration) -> RenderPlan {
        let mut video = Vec::new();
        let mut clip_start = Duration::ZERO;
        for clip in &self.video {
            let clip_end = clip_start + clip.duration;
            if clip_end > from {
                let skip = from.saturating_sub(clip_start);
                video.push(VideoClip {
                    start: clip.start + skip,
                    duration: clip.duration - skip,
                    ..clip.clone()
                });
            }
            clip_start = clip_end;
        }
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
                            ..clip.clone()
                        }
                    })
                    .collect(),
                gain_db: track.gain_db,
            })
            .collect();
        RenderPlan { video, audio }
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
        Ok(())
    }

    fn audio_clips(&self) -> impl Iterator<Item = (&AudioClip, f32)> {
        self.audio.iter().flat_map(|track| {
            track
                .clips
                .iter()
                .filter(|clip| !clip.duration.is_zero())
                .map(move |clip| (clip, track.gain_db + clip.gain_db))
        })
    }
}

/// One `-ss … -t … -i …` input.
#[derive(Debug, Clone, PartialEq)]
struct Input<'a> {
    source: &'a Path,
    start: Duration,
    duration: Duration,
}

/// Inputs and the filter graph that turns them into `[vout]`: each clip
/// trimmed, framed and set to the output rate, then joined.
fn video_graph<'a>(
    plan: &'a RenderPlan,
    size: FrameSize,
    fps: (u32, u32),
    pixel_format: &str,
) -> (Vec<Input<'a>>, String) {
    let FrameSize { width, height } = size;
    let (numerator, denominator) = fps;
    let mut inputs = Vec::new();
    let mut chains = Vec::new();
    for (index, clip) in plan.video.iter().enumerate() {
        inputs.push(Input {
            source: &clip.source,
            start: clip.start,
            duration: clip.duration,
        });
        let framing = match clip.framing {
            Framing::Crop { x, y } => format!(
                "crop=w='min(iw,ih*{width}/{height})':h='min(ih,iw*{height}/{width})':\
                 x='(iw-ow)*{x}':y='(ih-oh)*{y}',scale={width}:{height}:flags=bicubic"
            ),
            Framing::Fit => format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease:flags=bicubic,\
                 pad={width}:{height}:(ow-iw)/2:(oh-ih)/2"
            ),
        };
        chains.push(format!(
            "[{index}:v:0]settb=AVTB,setpts=PTS-STARTPTS,fps={numerator}/{denominator},\
             trim=duration={},{framing},setsar=1,format=yuv420p[v{index}]",
            seconds(clip.duration)
        ));
    }
    let labels: String = (0..plan.video.len())
        .map(|index| format!("[v{index}]"))
        .collect();
    chains.push(if plan.video.len() == 1 {
        format!("{labels}format={pixel_format}[vout]")
    } else {
        format!(
            "{labels}concat=n={}:v=1:a=0,format={pixel_format}[vout]",
            plan.video.len()
        )
    });
    (inputs, chains.join(";"))
}

/// The mix of every audio clip, as long as the video, labelled `[amix]`.
/// `first_input` is the index of the first audio input.
fn audio_graph(plan: &RenderPlan, first_input: usize) -> (Vec<Input<'_>>, String) {
    let total = seconds(plan.duration());
    let mut inputs = Vec::new();
    let mut chains = Vec::new();
    for (index, (clip, gain_db)) in plan.audio_clips().enumerate() {
        inputs.push(Input {
            source: &clip.source,
            start: clip.start,
            duration: clip.duration,
        });
        let delay = (clip.at.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as u64;
        chains.push(format!(
            "[{}:a:0]asetpts=PTS-STARTPTS,\
             aformat=sample_fmts=fltp:sample_rates={SAMPLE_RATE}:channel_layouts=stereo,\
             volume={gain_db}dB,adelay=delays={delay}S:all=1[a{index}]",
            first_input + index
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
        "loudnorm=I={}:TP={}:LRA={}",
        target.integrated, target.true_peak, target.range
    )
}

/// Second-pass loudnorm: with the first pass's measurements and
/// `linear=true` it applies one gain to the whole mix (no pumping), as long
/// as that gain keeps the true peak under the target.
fn loudnorm_apply(target: LoudnessTarget, measured: Loudness) -> String {
    format!(
        "{}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true,\
         aresample={SAMPLE_RATE}",
        loudnorm(target),
        measured.integrated,
        measured.true_peak,
        measured.range,
        measured.threshold,
        measured.target_offset
    )
}

fn push_inputs(command: &mut std::process::Command, inputs: &[Input<'_>]) {
    for input in inputs {
        command
            .args([
                "-ss",
                &seconds(input.start),
                "-t",
                &seconds(input.duration),
                "-i",
            ])
            .arg(path_arg(input.source));
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
        let reference = LoudnessTarget {
            integrated: -14.0,
            true_peak: -1.0,
            range: 11.0,
        };
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

    /// Loudness of the plan's audio mix before normalization.
    fn measure_mix(
        &self,
        plan: &RenderPlan,
        target: LoudnessTarget,
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
                &format!("{mix};[amix]{}:print_format=json[aout]", loudnorm(target)),
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
                self.measure_mix(plan, target, monitor, Span::part(total, 0.0, 0.1))?,
            )),
            None => None,
        };

        let (mut inputs, video) = video_graph(plan, output.size, output.fps, "yuv420p");
        let (audio_inputs, mix) = audio_graph(plan, inputs.len());
        inputs.extend(audio_inputs);
        let audio_out = match measured {
            Some((target, measured)) => format!("[amix]{}[aout]", loudnorm_apply(target, measured)),
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
                let _ = std::fs::remove_file(&partial);
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
        let (inputs, video) = video_graph(&plan, size, fps, "bgra");
        let mut command = self.ffmpeg();
        push_inputs(&mut command, &inputs);
        command
            .args(["-filter_complex", &video, "-map", "[vout]"])
            .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"]);
        FrameStream::spawn(command, size, fps, from)
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
            source: PathBuf::from(name),
            start: secs(start),
            duration: secs(duration),
            framing: Framing::Crop { x: 0.5, y: 0.5 },
        }
    }

    fn audio(name: &str, start: f64, duration: f64, at: f64) -> AudioClip {
        AudioClip {
            source: PathBuf::from(name),
            start: secs(start),
            duration: secs(duration),
            at: secs(at),
            gain_db: 0.0,
        }
    }

    fn plan() -> RenderPlan {
        RenderPlan {
            video: vec![video("a.mp4", 1.0, 2.0), video("b.mp4", 0.0, 3.0)],
            audio: vec![
                AudioTrack {
                    clips: vec![audio("voice.mp3", 0.0, 3.0, 0.5)],
                    gain_db: 0.0,
                },
                AudioTrack {
                    clips: vec![audio("music.mp3", 10.0, 5.0, 0.0)],
                    gain_db: -12.0,
                },
            ],
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
        assert_eq!(
            later.audio[0].clips,
            vec![audio("voice.mp3", 2.0, 1.0, 0.0)]
        );
        assert_eq!(
            later.audio[1].clips,
            vec![audio("music.mp3", 12.5, 2.5, 0.0)]
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
        let (inputs, graph) = video_graph(&plan, FrameSize::new(1080, 1920), (30, 1), "yuv420p");
        assert_eq!(inputs.len(), 2);
        assert_eq!(
            (inputs[0].start, inputs[0].duration),
            (secs(1.0), secs(2.0))
        );
        assert!(graph.starts_with(
            "[0:v:0]settb=AVTB,setpts=PTS-STARTPTS,fps=30/1,trim=duration=2.000000,\
             crop=w='min(iw,ih*1080/1920)':h='min(ih,iw*1920/1080)':x='(iw-ow)*0.5':y='(ih-oh)*0.5',\
             scale=1080:1920:flags=bicubic,setsar=1,format=yuv420p[v0];"
        ));
        assert!(graph.ends_with("[v0][v1]concat=n=2:v=1:a=0,format=yuv420p[vout]"));
    }

    #[test]
    fn fit_pads_instead_of_cropping() {
        let mut plan = plan();
        plan.video.truncate(1);
        plan.video[0].framing = Framing::Fit;
        let (_, graph) = video_graph(&plan, FrameSize::new(1080, 1920), (30, 1), "bgra");
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
    fn second_pass_applies_one_linear_gain() {
        let target = LoudnessTarget {
            integrated: -14.0,
            true_peak: -1.5,
            range: 11.0,
        };
        let measured = Loudness {
            integrated: -23.5,
            true_peak: -7.1,
            range: 1.2,
            threshold: -33.8,
            target_offset: 0.1,
        };
        assert_eq!(
            loudnorm_apply(target, measured),
            "loudnorm=I=-14:TP=-1.5:LRA=11:measured_I=-23.5:measured_TP=-7.1:measured_LRA=1.2:\
             measured_thresh=-33.8:offset=0.1:linear=true,aresample=48000"
        );
    }
}
