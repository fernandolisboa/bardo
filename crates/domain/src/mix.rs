//! The audio mix (PRD stories 61-63): a level, mute and solo for each audio
//! lane, and the music ducking under the narration. The mix is part of the
//! timeline, so preview and render play the same thing and every change to
//! it is an edit that can be undone (`crate::Edit`).
//!
//! Ducking lowers the music while the narration speaks, by an amount the
//! user picks. Where the narration speaks comes from its word timings: the
//! music starts going down a little before the first word of a phrase, so
//! the voice comes in over a quieter bed, and comes back up after the last
//! word. Pauses shorter than [`DUCK_HOLD`] keep the music down, so it does
//! not pump between words.

use std::time::Duration;

/// A level in tenths of a decibel: exact, so mixes compare and save as
/// they were set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Decibels(i16);

impl Decibels {
    pub const ZERO: Decibels = Decibels(0);

    pub const fn from_tenths(tenths: i16) -> Self {
        Decibels(tenths)
    }

    pub fn tenths(self) -> i16 {
        self.0
    }

    pub fn db(self) -> f32 {
        f32::from(self.0) / 10.0
    }

    /// This level moved by `by`, within `range`.
    pub fn nudged(self, by: Decibels, range: (Decibels, Decibels)) -> Decibels {
        Decibels(self.0.saturating_add(by.0)).clamp(range.0, range.1)
    }
}

/// How far a lane's level goes down and up.
pub const GAIN_RANGE: (Decibels, Decibels) = (Decibels(-300), Decibels(120));
/// How deep the music can duck.
pub const DUCK_RANGE: (Decibels, Decibels) = (Decibels(10), Decibels(300));
/// How deep the music ducks until the user says otherwise.
pub const DEFAULT_DUCK: Decibels = Decibels(120);

/// How long the music takes to go down, ahead of the first word.
pub const DUCK_ATTACK: Duration = Duration::from_millis(150);
/// How long it takes to come back after the last word.
pub const DUCK_RELEASE: Duration = Duration::from_millis(500);
/// A pause shorter than this keeps the music down.
pub const DUCK_HOLD: Duration = Duration::from_millis(1_000);

/// An audio lane of the timeline: A1 to A3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioLane {
    Narration,
    Music,
    Sfx,
}

impl AudioLane {
    pub const ALL: [AudioLane; 3] = [AudioLane::Narration, AudioLane::Music, AudioLane::Sfx];

    fn index(self) -> usize {
        match self {
            AudioLane::Narration => 0,
            AudioLane::Music => 1,
            AudioLane::Sfx => 2,
        }
    }
}

/// One lane's place in the mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LaneMix {
    pub gain: Decibels,
    pub muted: bool,
    pub solo: bool,
}

/// Whether and how deep the music ducks under the narration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ducking {
    pub on: bool,
    /// How far down the music goes, as a positive level.
    pub depth: Decibels,
}

impl Default for Ducking {
    fn default() -> Self {
        Ducking {
            on: true,
            depth: DEFAULT_DUCK,
        }
    }
}

/// The mix of a timeline: every lane at 0 dB, nothing muted or soloed, the
/// music ducking 12 dB, until the user changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Mix {
    lanes: [LaneMix; 3],
    pub ducking: Ducking,
}

impl Mix {
    pub fn lane(&self, lane: AudioLane) -> LaneMix {
        self.lanes[lane.index()]
    }

    pub(crate) fn lane_mut(&mut self, lane: AudioLane) -> &mut LaneMix {
        &mut self.lanes[lane.index()]
    }

    /// Sets a lane's mix as it is, as when reading a saved cut back; the
    /// editor changes it through `crate::Edit::SetLane`.
    pub fn set_lane(&mut self, lane: AudioLane, mix: LaneMix) {
        self.lanes[lane.index()] = mix;
    }

    /// Whether `lane` is heard: not muted, and soloed when any lane is.
    pub fn is_audible(&self, lane: AudioLane) -> bool {
        let mix = self.lane(lane);
        let soloing = self.lanes.iter().any(|lane| lane.solo);
        !mix.muted && (!soloing || mix.solo)
    }

    /// Every level within its range.
    pub fn holds_together(&self) -> bool {
        let in_range =
            |level: Decibels, (low, high): (Decibels, Decibels)| (low..=high).contains(&level);
        self.lanes
            .iter()
            .all(|lane| in_range(lane.gain, GAIN_RANGE))
            && in_range(self.ducking.depth, DUCK_RANGE)
    }
}

/// One dip of the ducked music: down from `start` to full depth at `full`,
/// held until `release`, back up by `end`. Times are on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dip {
    pub start: Duration,
    pub full: Duration,
    pub release: Duration,
    pub end: Duration,
}

/// How the music's level moves under the narration: its dips, in order and
/// apart, each `depth` deep.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DuckEnvelope {
    pub depth: Decibels,
    pub dips: Vec<Dip>,
}

impl DuckEnvelope {
    /// The envelope that ducks `depth` under `speech`, the stretches of the
    /// timeline where the narration speaks (in any order, overlapping or
    /// not). Stretches closer than [`DUCK_HOLD`] make one dip; each dip
    /// starts going down [`DUCK_ATTACK`] ahead (from the timeline's start
    /// at the earliest) and comes back over [`DUCK_RELEASE`].
    pub fn under(speech: &[(Duration, Duration)], depth: Decibels) -> DuckEnvelope {
        let mut spans: Vec<(Duration, Duration)> = speech
            .iter()
            .copied()
            .filter(|(start, end)| start < end)
            .collect();
        spans.sort();
        let mut merged: Vec<(Duration, Duration)> = Vec::new();
        for (start, end) in spans {
            match merged.last_mut() {
                Some(last) if start < last.1 + DUCK_HOLD => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        let dips = merged
            .into_iter()
            .map(|(full, release)| Dip {
                start: full.saturating_sub(DUCK_ATTACK),
                full,
                release,
                end: release + DUCK_RELEASE,
            })
            .collect();
        DuckEnvelope { depth, dips }
    }

    /// How much the music is lowered at `time`, from 0 (not at all) to 1
    /// (the full depth).
    pub fn amount_at(&self, time: Duration) -> f32 {
        let ramp = |from: Duration, to: Duration| {
            if time >= to {
                1.0
            } else if time <= from {
                0.0
            } else {
                (time - from).as_secs_f32() / (to - from).as_secs_f32()
            }
        };
        self.dips
            .iter()
            .find(|dip| dip.start <= time && time < dip.end)
            .map_or(0.0, |dip| {
                ramp(dip.start, dip.full) - ramp(dip.release, dip.end)
            })
    }

    /// The music's level change at `time`, in decibels (zero or below).
    pub fn gain_at(&self, time: Duration) -> f32 {
        -self.depth.db() * self.amount_at(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn decibels_are_tenths_and_stay_in_range() {
        let gain = Decibels::from_tenths(-60);
        assert_eq!(gain.db(), -6.0);
        assert_eq!(
            gain.nudged(Decibels::from_tenths(5), GAIN_RANGE),
            Decibels::from_tenths(-55)
        );
        assert_eq!(
            Decibels::from_tenths(110).nudged(Decibels::from_tenths(50), GAIN_RANGE),
            GAIN_RANGE.1
        );
        assert_eq!(
            Decibels::from_tenths(i16::MIN).nudged(Decibels::from_tenths(-10), GAIN_RANGE),
            GAIN_RANGE.0
        );
    }

    #[test]
    fn a_new_mix_plays_everything_and_ducks_the_music_twelve_db() {
        let mix = Mix::default();
        for lane in AudioLane::ALL {
            assert_eq!(mix.lane(lane), LaneMix::default());
            assert!(mix.is_audible(lane));
        }
        assert_eq!(
            mix.ducking,
            Ducking {
                on: true,
                depth: Decibels::from_tenths(120)
            }
        );
        assert!(mix.holds_together());
    }

    #[test]
    fn muting_silences_a_lane_and_solo_silences_the_others() {
        let mut mix = Mix::default();
        mix.lane_mut(AudioLane::Music).muted = true;
        assert!(!mix.is_audible(AudioLane::Music));
        assert!(mix.is_audible(AudioLane::Narration));

        mix.lane_mut(AudioLane::Narration).solo = true;
        assert!(mix.is_audible(AudioLane::Narration));
        assert!(!mix.is_audible(AudioLane::Sfx), "not soloed");

        // A soloed lane that is also muted stays silent.
        mix.lane_mut(AudioLane::Music).solo = true;
        assert!(!mix.is_audible(AudioLane::Music));
    }

    #[test]
    fn a_level_out_of_range_does_not_hold_together() {
        let mut mix = Mix::default();
        mix.lane_mut(AudioLane::Sfx).gain = Decibels::from_tenths(130);
        assert!(!mix.holds_together());
        let mut mix = Mix::default();
        mix.ducking.depth = Decibels::ZERO;
        assert!(!mix.holds_together());
    }

    #[test]
    fn music_dips_ahead_of_speech_and_comes_back_after_it() {
        let depth = Decibels::from_tenths(120);
        let envelope = DuckEnvelope::under(&[(ms(2_000), ms(3_000))], depth);
        assert_eq!(
            envelope.dips,
            vec![Dip {
                start: ms(1_850),
                full: ms(2_000),
                release: ms(3_000),
                end: ms(3_500),
            }]
        );
        assert_eq!(envelope.gain_at(ms(1_000)), 0.0);
        assert_eq!(envelope.gain_at(ms(1_850)), 0.0);
        assert!((envelope.gain_at(ms(1_925)) + 6.0).abs() < 1e-4, "half way");
        assert_eq!(envelope.gain_at(ms(2_000)), -12.0, "down by the first word");
        assert_eq!(envelope.gain_at(ms(2_999)), -12.0);
        assert!((envelope.gain_at(ms(3_250)) + 6.0).abs() < 1e-4);
        assert_eq!(envelope.gain_at(ms(3_500)), 0.0);
    }

    #[test]
    fn the_envelope_moves_smoothly() {
        let envelope = DuckEnvelope::under(
            &[(ms(500), ms(900)), (ms(3_000), ms(3_200))],
            Decibels::from_tenths(200),
        );
        // No step larger than a ramp allows in 10 ms.
        let most = 20.0 * 10.0 / DUCK_ATTACK.as_millis() as f32 + 1e-3;
        let mut last = envelope.gain_at(Duration::ZERO);
        for step in 1..500 {
            let gain = envelope.gain_at(ms(step * 10));
            assert!((gain - last).abs() <= most, "{} ms", step * 10);
            last = gain;
        }
    }

    #[test]
    fn short_pauses_keep_the_music_down() {
        let envelope = DuckEnvelope::under(
            &[
                (ms(1_000), ms(1_400)),
                (ms(1_500), ms(2_000)),
                (ms(2_900), ms(3_100)),
                (ms(5_000), ms(5_500)),
            ],
            DEFAULT_DUCK,
        );
        let spans: Vec<(Duration, Duration)> = envelope
            .dips
            .iter()
            .map(|dip| (dip.full, dip.release))
            .collect();
        assert_eq!(
            spans,
            vec![(ms(1_000), ms(3_100)), (ms(5_000), ms(5_500))],
            "pauses under a second are held"
        );
        assert_eq!(envelope.amount_at(ms(2_500)), 1.0);
        assert_eq!(envelope.amount_at(ms(4_000)), 0.0);
    }

    #[test]
    fn speech_given_out_of_order_or_overlapping_makes_the_same_dips() {
        let envelope = DuckEnvelope::under(
            &[
                (ms(3_000), ms(3_200)),
                (ms(1_000), ms(2_000)),
                (ms(1_500), ms(1_800)),
                (ms(4_000), ms(4_000)),
            ],
            DEFAULT_DUCK,
        );
        assert_eq!(envelope.dips.len(), 2);
        assert_eq!(envelope.dips[0].release, ms(2_000));
        assert!(DuckEnvelope::under(&[], DEFAULT_DUCK).dips.is_empty());
    }

    #[test]
    fn speech_at_the_very_start_is_ducked_from_the_first_frame() {
        let envelope = DuckEnvelope::under(&[(ms(50), ms(1_000))], DEFAULT_DUCK);
        assert_eq!(envelope.dips[0].start, Duration::ZERO);
        assert!(envelope.amount_at(Duration::ZERO) == 0.0);
        assert_eq!(envelope.amount_at(ms(50)), 1.0);
        let at_zero = DuckEnvelope::under(&[(ms(0), ms(1_000))], DEFAULT_DUCK);
        assert_eq!(at_zero.amount_at(Duration::ZERO), 1.0);
    }
}
