//! The final render (PRD stories 71-75): a video project's cut made into
//! one file per network account, in that account's render preset. Render
//! costs time and cannot be taken back, so it runs only after a review
//! whose quality gates say what would go wrong (CONTEXT.md).
//!
//! Quality gates are measured rules: the cut's length against the preset's
//! limit, the mix's loudness against its target, captions, framing, media
//! and encoders. They are checked here, with no provider call, so the
//! review costs nothing and works offline. A blocking gate keeps that
//! output from rendering; a warning lets the user go on.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    AspectRatio, MaxDuration, Network, NetworkAccountId, ProfileId, RenderPreset, RepositoryError,
    VideoCodec, VideoProjectId,
};

uuid_id!(
    /// Identifies one rendered file.
    RenderId
);

/// The true peak a render's loudness normalization keeps under, in dBTP.
/// Networks transcode uploads, and a peak under −1 dBTP survives that.
pub const TRUE_PEAK_CEILING: f64 = -1.0;

/// How far from its target a render's integrated loudness may land, in LU.
pub const LOUDNESS_TOLERANCE: f64 = 1.0;

/// A mix this far from its target (in dB either way) is flagged: the render
/// would raise noise with it, or squash it, to reach the target.
pub const LOUDNESS_GAIN_WARNING: f64 = 6.0;

/// Below this integrated loudness (LUFS) a mix counts as silent; loudness
/// normalization leaves it as it is.
pub const SILENCE: f64 = -70.0;

/// Loudness as measured (EBU R128).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredLoudness {
    /// Integrated loudness, LUFS.
    pub integrated: f64,
    /// Maximum true peak, dBTP.
    pub true_peak: f64,
}

impl MeasuredLoudness {
    pub fn is_silent(&self) -> bool {
        !self.integrated.is_finite() || self.integrated < SILENCE
    }

    /// The gain, in dB, that brings this to `target` (LUFS).
    pub fn gain_to(&self, target: f64) -> f64 {
        target - self.integrated
    }

    /// Whether this landed within [`LOUDNESS_TOLERANCE`] of `target`.
    pub fn is_on_target(&self, target: f64) -> bool {
        !self.is_silent() && self.gain_to(target).abs() <= LOUDNESS_TOLERANCE
    }
}

/// One thing the review flags about a render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gate {
    /// The cut is longer than the network takes. Blocks that output.
    TooLong { limit: MaxDuration },
    /// No encoder for the preset's codec works on this machine. Blocks
    /// that output.
    NoEncoder(VideoCodec),
    /// The cut was made in one frame shape and this output is the other:
    /// each clip fills it through its crop window, centered unless moved,
    /// which the user may not have looked at.
    Reframed {
        cut: AspectRatio,
        output: AspectRatio,
    },
    /// Captions are turned off in the cut.
    CaptionsOff,
    /// Clips whose media is missing render black.
    MissingMedia(usize),
    /// The mix has no sound to speak of; it renders as it is.
    Silent,
    /// The mix is far from the target: the render changes it by `gain`
    /// decibels (positive raises it).
    LoudnessFar { gain: f64 },
    /// Reaching the target with one gain would push the peaks over the
    /// ceiling, so normalization compresses them instead.
    PeaksLimited,
}

/// How much a gate holds a render back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GateLevel {
    /// The user may go on.
    Warning,
    /// That output cannot be rendered.
    Blocking,
}

impl Gate {
    pub fn level(&self) -> GateLevel {
        match self {
            Gate::TooLong { .. } | Gate::NoEncoder(_) => GateLevel::Blocking,
            _ => GateLevel::Warning,
        }
    }

    pub fn blocks(&self) -> bool {
        self.level() == GateLevel::Blocking
    }
}

/// What the gates read about the cut, whatever the output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutFacts {
    pub duration: Duration,
    pub aspect: AspectRatio,
    pub captions_shown: bool,
    /// Video clips whose file is not in the project folder.
    pub missing_media: usize,
    /// The mix's loudness before normalization, once measured.
    pub mix: Option<MeasuredLoudness>,
}

/// The gates about the cut itself, shown once for every output.
pub fn cut_gates(cut: &CutFacts) -> Vec<Gate> {
    let mut gates = Vec::new();
    if !cut.captions_shown {
        gates.push(Gate::CaptionsOff);
    }
    if cut.missing_media > 0 {
        gates.push(Gate::MissingMedia(cut.missing_media));
    }
    if cut.mix.is_some_and(|mix| mix.is_silent()) {
        gates.push(Gate::Silent);
    }
    gates
}

/// The gates of one output: its preset against the cut. `encoder_works` is
/// `None` until the machine's encoders are known.
pub fn output_gates(
    cut: &CutFacts,
    preset: &RenderPreset,
    encoder_works: Option<bool>,
) -> Vec<Gate> {
    let mut gates = Vec::new();
    let limit = preset.max_duration;
    if cut.duration > Duration::from_secs(u64::from(limit.seconds())) {
        gates.push(Gate::TooLong { limit });
    }
    if encoder_works == Some(false) {
        gates.push(Gate::NoEncoder(preset.codec));
    }
    if preset.aspect != cut.aspect {
        gates.push(Gate::Reframed {
            cut: cut.aspect,
            output: preset.aspect,
        });
    }
    if let Some(mix) = cut.mix.filter(|mix| !mix.is_silent()) {
        let gain = mix.gain_to(preset.loudness.lufs());
        if gain.abs() > LOUDNESS_GAIN_WARNING {
            gates.push(Gate::LoudnessFar { gain });
        }
        if mix.true_peak + gain > TRUE_PEAK_CEILING {
            gates.push(Gate::PeaksLimited);
        }
    }
    gates
}

/// A file rendered for one network account of a video project: the
/// latest one, which a new render for the same account replaces.
#[derive(Debug, Clone, PartialEq)]
pub struct Render {
    pub id: RenderId,
    pub owner: ProfileId,
    pub project: VideoProjectId,
    pub account: NetworkAccountId,
    pub network: Network,
    /// The preset it was rendered in.
    pub preset: RenderPreset,
    /// In the project folder.
    pub file: String,
    /// The encoder that made it, as ffmpeg names it.
    pub encoder: String,
    pub duration: Duration,
    pub size_bytes: u64,
    /// The file's loudness as measured after rendering; `None` for silence.
    pub loudness: Option<MeasuredLoudness>,
    /// What the cut looked like when rendered (a fingerprint of the render
    /// plan), to tell when the cut has changed since.
    pub cut: String,
    pub rendered_at: SystemTime,
}

impl Render {
    /// Whether the file's loudness landed within the tolerance of its
    /// preset's target. A silent render has nothing to land.
    pub fn loudness_on_target(&self) -> Option<bool> {
        self.loudness
            .filter(|loudness| !loudness.is_silent())
            .map(|loudness| loudness.is_on_target(self.preset.loudness.lufs()))
    }
}

/// Persistence port for renders.
pub trait RenderRepository: Send + Sync {
    /// The project's renders, in `Network::ALL` order.
    fn renders(&self, project: VideoProjectId) -> Result<Vec<Render>, RepositoryError>;

    /// Saves a render, replacing the project's earlier one for the same
    /// account.
    fn save_render(&self, render: &Render) -> Result<(), RepositoryError>;
}

impl<T: RenderRepository + ?Sized> RenderRepository for Arc<T> {
    fn renders(&self, project: VideoProjectId) -> Result<Vec<Render>, RepositoryError> {
        (**self).renders(project)
    }

    fn save_render(&self, render: &Render) -> Result<(), RepositoryError> {
        (**self).save_render(render)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut() -> CutFacts {
        CutFacts {
            duration: Duration::from_secs(58),
            aspect: AspectRatio::Vertical,
            captions_shown: true,
            missing_media: 0,
            mix: Some(MeasuredLoudness {
                integrated: -16.0,
                true_peak: -6.0,
            }),
        }
    }

    #[test]
    fn a_short_that_fits_every_preset_passes_clean() {
        let cut = cut();
        assert!(cut_gates(&cut).is_empty());
        for network in [Network::YouTube, Network::TikTok, Network::InstagramReels] {
            assert_eq!(
                output_gates(&cut, &network.render_preset(), Some(true)),
                vec![],
                "{network}"
            );
        }
    }

    #[test]
    fn a_cut_over_the_networks_limit_blocks_that_output_only() {
        let cut = CutFacts {
            duration: Duration::from_secs(150),
            ..cut()
        };
        let x = output_gates(&cut, &Network::X.render_preset(), Some(true));
        assert_eq!(
            x,
            vec![Gate::TooLong {
                limit: MaxDuration::from_seconds(140).unwrap()
            }]
        );
        assert!(x[0].blocks());
        assert!(output_gates(&cut, &Network::YouTube.render_preset(), Some(true)).is_empty());
    }

    #[test]
    fn exactly_the_limit_is_allowed() {
        let cut = CutFacts {
            duration: Duration::from_secs(180),
            ..cut()
        };
        assert!(output_gates(&cut, &Network::YouTube.render_preset(), Some(true)).is_empty());
    }

    #[test]
    fn a_codec_no_encoder_handles_blocks_once_encoders_are_known() {
        let preset = Network::YouTube.render_preset();
        assert_eq!(
            output_gates(&cut(), &preset, Some(false)),
            vec![Gate::NoEncoder(VideoCodec::H264)]
        );
        assert!(output_gates(&cut(), &preset, None).is_empty());
    }

    #[test]
    fn another_frame_shape_is_a_warning() {
        let gates = output_gates(&cut(), &Network::Kick.render_preset(), Some(true));
        assert_eq!(
            gates,
            vec![Gate::Reframed {
                cut: AspectRatio::Vertical,
                output: AspectRatio::Landscape
            }]
        );
        assert_eq!(gates[0].level(), GateLevel::Warning);
    }

    #[test]
    fn captions_off_and_missing_media_flag_the_cut() {
        let cut = CutFacts {
            captions_shown: false,
            missing_media: 2,
            ..cut()
        };
        assert_eq!(
            cut_gates(&cut),
            vec![Gate::CaptionsOff, Gate::MissingMedia(2)]
        );
        assert!(cut_gates(&cut).iter().all(|gate| !gate.blocks()));
    }

    #[test]
    fn a_quiet_mix_far_from_the_target_is_flagged_with_its_gain() {
        let cut = CutFacts {
            mix: Some(MeasuredLoudness {
                integrated: -24.5,
                true_peak: -14.0,
            }),
            ..cut()
        };
        let gates = output_gates(&cut, &Network::YouTube.render_preset(), Some(true));
        assert_eq!(gates, vec![Gate::LoudnessFar { gain: 10.5 }]);
    }

    #[test]
    fn a_gain_pushing_peaks_over_the_ceiling_says_they_get_limited() {
        let cut = CutFacts {
            mix: Some(MeasuredLoudness {
                integrated: -18.0,
                true_peak: -2.0,
            }),
            ..cut()
        };
        // +4 dB takes the peak from −2 to +2 dBTP.
        assert_eq!(
            output_gates(&cut, &Network::YouTube.render_preset(), Some(true)),
            vec![Gate::PeaksLimited]
        );
    }

    #[test]
    fn a_silent_mix_is_flagged_once_and_never_normalized() {
        let cut = CutFacts {
            mix: Some(MeasuredLoudness {
                integrated: f64::NEG_INFINITY,
                true_peak: f64::NEG_INFINITY,
            }),
            ..cut()
        };
        assert_eq!(cut_gates(&cut), vec![Gate::Silent]);
        assert!(output_gates(&cut, &Network::YouTube.render_preset(), Some(true)).is_empty());
    }

    #[test]
    fn a_render_lands_on_target_within_the_tolerance() {
        let on = MeasuredLoudness {
            integrated: -14.8,
            true_peak: -1.2,
        };
        assert!(on.is_on_target(-14.0));
        let off = MeasuredLoudness {
            integrated: -15.2,
            ..on
        };
        assert!(!off.is_on_target(-14.0));
        let silent = MeasuredLoudness {
            integrated: -80.0,
            true_peak: -90.0,
        };
        assert!(silent.is_silent() && !silent.is_on_target(-14.0));
    }
}
