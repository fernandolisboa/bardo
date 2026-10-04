//! Instagram Reels rules (PRD stories 84, 85 and 89; ADR-0008): what a file
//! must be for Instagram to take it as a Reel, the frame its cover comes
//! from, and the publishing limit Instagram reports.
//!
//! The specs are checked on the rendered file before the upload review
//! lets it go, like a quality gate: a file Instagram would refuse after it
//! arrived costs the whole upload. They come from the Reels specifications
//! of the IG user media reference (checked 2026-10-04, Graph API v25.0).
//!
//! The publishing limit is never a number written here: the guide says
//! 100 posts in a moving 24 hours and the reference still says 50, so the
//! upload reads `content_publishing_limit` and goes by what it says.

use std::io::{self, Read};
use std::time::{Duration, SystemTime};

/// The shortest Reel Instagram takes.
pub const REEL_MIN_DURATION: Duration = Duration::from_secs(3);
/// The longest Reel Instagram takes: 15 minutes.
pub const REEL_MAX_DURATION: Duration = Duration::from_secs(15 * 60);
/// The largest file: 300 MB, counted in decimal megabytes so a file that
/// passes here passes however Instagram counts them.
pub const REEL_MAX_BYTES: u64 = 300 * 1000 * 1000;
/// The widest picture, in pixels.
pub const REEL_MAX_WIDTH: u32 = 1920;
/// Frame rates Instagram takes, in frames per second (inclusive).
pub const REEL_FPS: (f64, f64) = (23.0, 60.0);

/// How long a publishing limit that is used up is read again, when Bardo
/// cannot tell when a place frees: the posts that fill it were not
/// published by Bardo.
pub const LIMIT_RECHECK: Duration = Duration::from_secs(60 * 60);

/// The file's container, as its `ftyp` box names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Container {
    /// MPEG-4 Part 14 (`isom`, `mp41`, `mp42`, `avc1`, …).
    Mp4,
    /// QuickTime (`qt  `).
    Mov,
}

/// How a file's top-level boxes are laid out, as far as Instagram cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mp4Layout {
    /// `None` when the first box is no `ftyp` of MP4 or QuickTime.
    pub container: Option<Container>,
    /// The `moov` box (the index) comes before the `mdat` box (the media),
    /// so the file plays before it has fully arrived ("fast start").
    pub moov_first: bool,
}

/// Top-level boxes read at most before giving up on finding `moov` or
/// `mdat`: a fast-start MP4 has them within the first few.
const MAX_BOXES: usize = 64;

/// Reads the top-level boxes of an MP4 or MOV file until `moov` or `mdat`.
/// Boxes in between (`free`, `wide`, `uuid`) are skipped by reading, so a
/// file that puts a large one first is slow but still answered.
pub fn mp4_layout(file: &mut dyn Read) -> io::Result<Mp4Layout> {
    let mut container = None;
    for index in 0..MAX_BOXES {
        let mut header = [0u8; 8];
        if !read_full(file, &mut header)? {
            break;
        }
        let size = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let kind = [header[4], header[5], header[6], header[7]];
        let (size, header_len) = match size {
            // The size follows as 64 bits.
            1 => {
                let mut large = [0u8; 8];
                if !read_full(file, &mut large)? {
                    break;
                }
                (u64::from_be_bytes(large), 16)
            }
            // Runs to the end of the file: nothing after it.
            0 => (u64::MAX, 8),
            size => (size, 8),
        };
        if size < header_len {
            break;
        }
        match &kind {
            b"moov" => {
                return Ok(Mp4Layout {
                    container,
                    moov_first: true,
                });
            }
            b"mdat" => break,
            b"ftyp" if index == 0 => {
                let mut brand = [0u8; 4];
                if size < header_len + 4 || !read_full(file, &mut brand)? {
                    break;
                }
                container = Some(match &brand {
                    b"qt  " => Container::Mov,
                    _ => Container::Mp4,
                });
                skip(file, size - header_len - 4)?;
            }
            _ if index == 0 => break,
            _ => skip(file, size - header_len)?,
        }
    }
    Ok(Mp4Layout {
        container,
        moov_first: false,
    })
}

/// Fills `buffer`; false when the file ended first.
fn read_full(file: &mut dyn Read, buffer: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => return Ok(false),
            read => filled += read,
        }
    }
    Ok(true)
}

fn skip(file: &mut dyn Read, bytes: u64) -> io::Result<()> {
    io::copy(&mut file.take(bytes), &mut io::sink()).map(drop)
}

/// What the Reel checks read of a rendered file.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelFile {
    pub layout: Mp4Layout,
    /// The first video stream's codec as ffprobe names it (`h264`, `hevc`);
    /// `None` without a video stream.
    pub codec: Option<String>,
    pub width: u32,
    /// Frames per second of the video stream.
    pub fps: f64,
    pub duration: Duration,
    pub size: u64,
}

/// One way a file falls short of what Instagram takes as a Reel. Each one
/// blocks the upload: Instagram would refuse the file after it arrived.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReelSpecProblem {
    /// Bardo could not read the file's streams.
    Unreadable,
    /// Not an MP4 or MOV file.
    Container,
    /// The index comes after the media: the file is not fast start.
    MoovNotFirst,
    /// The file has no video stream.
    NoVideo,
    /// The video is not H.264 or HEVC: the codec as ffprobe names it.
    Codec(String),
    /// Outside 23 to 60 frames per second: the rate in hundredths.
    FrameRate(u32),
    /// Wider than 1920 pixels: the width.
    TooWide(u32),
    /// Shorter than 3 seconds.
    TooShort(Duration),
    /// Longer than 15 minutes.
    TooLong(Duration),
    /// Over 300 MB: the size in bytes.
    TooBig(u64),
}

impl ReelSpecProblem {
    /// Every `code`, in the order `check_reel` lists them.
    pub const CODES: [&'static str; 10] = [
        "unreadable",
        "container",
        "moov_not_first",
        "no_video",
        "codec",
        "frame_rate",
        "too_wide",
        "too_short",
        "too_long",
        "too_big",
    ];

    /// Stable name, for texts.
    pub fn code(&self) -> &'static str {
        match self {
            ReelSpecProblem::Unreadable => "unreadable",
            ReelSpecProblem::Container => "container",
            ReelSpecProblem::MoovNotFirst => "moov_not_first",
            ReelSpecProblem::NoVideo => "no_video",
            ReelSpecProblem::Codec(_) => "codec",
            ReelSpecProblem::FrameRate(_) => "frame_rate",
            ReelSpecProblem::TooWide(_) => "too_wide",
            ReelSpecProblem::TooShort(_) => "too_short",
            ReelSpecProblem::TooLong(_) => "too_long",
            ReelSpecProblem::TooBig(_) => "too_big",
        }
    }
}

/// Everything about `file` Instagram would refuse in a Reel, in the order
/// the review lists them.
pub fn check_reel(file: &ReelFile) -> Vec<ReelSpecProblem> {
    let mut problems = Vec::new();
    if file.layout.container.is_none() {
        problems.push(ReelSpecProblem::Container);
    } else if !file.layout.moov_first {
        problems.push(ReelSpecProblem::MoovNotFirst);
    }
    match file.codec.as_deref() {
        Some("h264" | "hevc") => {}
        Some(other) => problems.push(ReelSpecProblem::Codec(other.to_owned())),
        None => problems.push(ReelSpecProblem::NoVideo),
    }
    if file.codec.is_some() && !(REEL_FPS.0..=REEL_FPS.1).contains(&file.fps) {
        let hundredths = (file.fps.max(0.0) * 100.0).round().min(f64::from(u32::MAX));
        problems.push(ReelSpecProblem::FrameRate(hundredths as u32));
    }
    if file.width > REEL_MAX_WIDTH {
        problems.push(ReelSpecProblem::TooWide(file.width));
    }
    if file.duration < REEL_MIN_DURATION {
        problems.push(ReelSpecProblem::TooShort(file.duration));
    } else if file.duration > REEL_MAX_DURATION {
        problems.push(ReelSpecProblem::TooLong(file.duration));
    }
    if file.size > REEL_MAX_BYTES {
        problems.push(ReelSpecProblem::TooBig(file.size));
    }
    problems
}

/// Reads a cover time typed by the user: seconds (`3`, `2.5`, `2,5`) or
/// minutes and seconds (`1:05`, `1:05.5`). `None` for anything else.
pub fn parse_cover_time(text: &str) -> Option<Duration> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return None;
    }
    let seconds = |part: &str| -> Option<f64> {
        let valid = !part.is_empty()
            && part.chars().all(|c| c.is_ascii_digit() || c == '.')
            && part.matches('.').count() <= 1
            && !part.starts_with('.')
            && !part.ends_with('.');
        valid.then(|| part.parse::<f64>().ok()).flatten()
    };
    let total = match text.split_once(':') {
        Some((minutes, rest)) => {
            let minutes = minutes.parse::<u32>().ok()?;
            let rest = seconds(rest)?;
            if rest >= 60.0 || !rest.is_finite() {
                return None;
            }
            f64::from(minutes) * 60.0 + rest
        }
        None => seconds(&text)?,
    };
    (total.is_finite() && total < 24.0 * 60.0 * 60.0)
        .then(|| Duration::from_millis((total * 1000.0).round() as u64))
}

/// A cover time as the review shows it: `m:ss`, with tenths when there
/// are any (`0:03`, `1:05.5`).
pub fn format_cover_time(at: Duration) -> String {
    let tenths = (at.as_millis() + 50) / 100;
    let (seconds, tenth) = (tenths / 10, tenths % 10);
    let (minutes, seconds) = (seconds / 60, seconds % 60);
    if tenth == 0 {
        format!("{minutes}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}.{tenth}")
    }
}

/// How much of Instagram's publishing limit an account used, as
/// `content_publishing_limit` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublishingLimit {
    /// Posts published through the API within the window.
    pub used: u32,
    /// Posts the window allows.
    pub total: u32,
    /// The moving window the limit counts over (24 hours today).
    pub window: Duration,
}

impl PublishingLimit {
    /// Whether one more post may go now.
    pub fn has_room(&self) -> bool {
        self.used < self.total
    }

    /// When to read the limit again, given the times Bardo published this
    /// account's posts (any order). Within the window, the oldest of them
    /// frees its place once the window passes it. When the limit also
    /// counts posts Bardo did not publish (another app did), one of those
    /// may free a place sooner, so it reads again after [`LIMIT_RECHECK`]
    /// at the latest. Never sooner than a minute from `now`, nor later than
    /// the window.
    pub fn next_try(&self, now: SystemTime, published: &[SystemTime]) -> NextTry {
        let start = now
            .checked_sub(self.window)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let counted: Vec<SystemTime> = published
            .iter()
            .copied()
            .filter(|at| *at > start && *at <= now)
            .collect();
        let recheck = now + LIMIT_RECHECK;
        let frees = counted
            .iter()
            .min()
            .and_then(|oldest| oldest.checked_add(self.window));
        let all_bardos = counted.len() as u64 >= u64::from(self.used);
        let (at, frees) = match frees {
            Some(at) if all_bardos || at <= recheck => (at, true),
            _ => (recheck, false),
        };
        let soonest = now + Duration::from_secs(60);
        let latest = now
            .checked_add(self.window.max(Duration::from_secs(60)))
            .unwrap_or(recheck)
            .max(soonest);
        NextTry {
            at: at.clamp(soonest, latest),
            frees,
        }
    }
}

/// When a used-up publishing limit is read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextTry {
    pub at: SystemTime,
    /// Whether a place frees by then, so the post goes; otherwise Bardo
    /// only reads the limit again.
    pub frees: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(body);
        bytes
    }

    fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut body = brand.to_vec();
        body.extend_from_slice(&[0, 0, 2, 0]);
        body.extend_from_slice(b"isomiso2avc1mp41");
        boxed(b"ftyp", &body)
    }

    fn layout(parts: &[Vec<u8>]) -> Mp4Layout {
        let bytes = parts.concat();
        mp4_layout(&mut bytes.as_slice()).unwrap()
    }

    #[test]
    fn a_fast_start_mp4_has_its_index_first() {
        let file = layout(&[
            ftyp(b"isom"),
            boxed(b"free", &[0; 8]),
            boxed(b"moov", &[1; 40]),
            boxed(b"mdat", &[2; 400]),
        ]);
        assert_eq!(
            file,
            Mp4Layout {
                container: Some(Container::Mp4),
                moov_first: true
            }
        );
    }

    #[test]
    fn an_index_after_the_media_is_not_fast_start() {
        let file = layout(&[
            ftyp(b"mp42"),
            boxed(b"mdat", &[2; 400]),
            boxed(b"moov", &[1; 40]),
        ]);
        assert_eq!(file.container, Some(Container::Mp4));
        assert!(!file.moov_first);
    }

    #[test]
    fn a_quicktime_file_is_a_mov() {
        let file = layout(&[ftyp(b"qt  "), boxed(b"moov", &[1; 40])]);
        assert_eq!(file.container, Some(Container::Mov));
        assert!(file.moov_first);
    }

    #[test]
    fn a_large_size_box_is_skipped_by_its_64_bit_size() {
        let mut wide = 1u32.to_be_bytes().to_vec();
        wide.extend_from_slice(b"free");
        wide.extend_from_slice(&(16u64 + 10).to_be_bytes());
        wide.extend_from_slice(&[0; 10]);
        let file = layout(&[ftyp(b"isom"), wide, boxed(b"moov", &[])]);
        assert!(file.moov_first);
    }

    #[test]
    fn something_else_is_no_mp4() {
        for bytes in [
            b"RIFF\0\0\0\0WEBPVP8 ".to_vec(),
            boxed(b"moov", &[]),
            Vec::new(),
            vec![0, 0, 0, 3, b'f', b't', b'y', b'p'],
        ] {
            let file = mp4_layout(&mut bytes.as_slice()).unwrap();
            assert_eq!(file.container, None, "{bytes:?}");
        }
        let cut = &ftyp(b"isom")[..10];
        assert_eq!(mp4_layout(&mut &cut[..]).unwrap().container, None);
    }

    fn good() -> ReelFile {
        ReelFile {
            layout: Mp4Layout {
                container: Some(Container::Mp4),
                moov_first: true,
            },
            codec: Some("h264".into()),
            width: 1080,
            fps: 30.0,
            duration: Duration::from_secs(58),
            size: 70_000_000,
        }
    }

    #[test]
    fn a_rendered_short_passes_every_reel_check() {
        assert_eq!(check_reel(&good()), []);
        let hevc = ReelFile {
            codec: Some("hevc".into()),
            width: 1920,
            fps: 60.0,
            duration: REEL_MAX_DURATION,
            size: REEL_MAX_BYTES,
            ..good()
        };
        assert_eq!(check_reel(&hevc), [], "the limits themselves pass");
        let edge = ReelFile {
            fps: 23.0,
            duration: REEL_MIN_DURATION,
            ..good()
        };
        assert_eq!(check_reel(&edge), []);
    }

    #[test]
    fn each_spec_a_file_misses_is_its_own_problem() {
        let file = ReelFile {
            layout: Mp4Layout {
                container: Some(Container::Mov),
                moov_first: false,
            },
            codec: Some("vp9".into()),
            width: 2160,
            fps: 120.0,
            duration: Duration::from_secs(16 * 60),
            size: REEL_MAX_BYTES + 1,
        };
        assert_eq!(
            check_reel(&file),
            [
                ReelSpecProblem::MoovNotFirst,
                ReelSpecProblem::Codec("vp9".into()),
                ReelSpecProblem::FrameRate(12_000),
                ReelSpecProblem::TooWide(2160),
                ReelSpecProblem::TooLong(Duration::from_secs(16 * 60)),
                ReelSpecProblem::TooBig(REEL_MAX_BYTES + 1),
            ]
        );
        let short = ReelFile {
            fps: 15.0,
            duration: Duration::from_millis(2_900),
            ..good()
        };
        assert_eq!(
            check_reel(&short),
            [
                ReelSpecProblem::FrameRate(1500),
                ReelSpecProblem::TooShort(Duration::from_millis(2_900)),
            ]
        );
    }

    #[test]
    fn a_file_without_video_or_container_is_refused_once_for_each() {
        let file = ReelFile {
            layout: Mp4Layout {
                container: None,
                moov_first: false,
            },
            codec: None,
            fps: 0.0,
            ..good()
        };
        assert_eq!(
            check_reel(&file),
            [ReelSpecProblem::Container, ReelSpecProblem::NoVideo],
            "no frame rate problem without a video stream"
        );
    }

    #[test]
    fn a_cover_time_reads_seconds_or_minutes_and_seconds() {
        let ms = Duration::from_millis;
        assert_eq!(parse_cover_time("3"), Some(ms(3_000)));
        assert_eq!(parse_cover_time(" 2.5 "), Some(ms(2_500)));
        assert_eq!(parse_cover_time("2,5"), Some(ms(2_500)), "pt-BR decimals");
        assert_eq!(parse_cover_time("1:05"), Some(ms(65_000)));
        assert_eq!(parse_cover_time("0:03.25"), Some(ms(3_250)));
        assert_eq!(parse_cover_time("0"), Some(Duration::ZERO));
        for bad in [
            "", "abc", "-1", "1:60", "1:", ":5", ".5", "5.", "1.2.3", "1:2:3",
        ] {
            assert_eq!(parse_cover_time(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_cover_time_shows_as_minutes_and_seconds() {
        let ms = Duration::from_millis;
        assert_eq!(format_cover_time(Duration::ZERO), "0:00");
        assert_eq!(format_cover_time(ms(3_000)), "0:03");
        assert_eq!(format_cover_time(ms(65_500)), "1:05.5");
        assert_eq!(format_cover_time(ms(59_960)), "1:00", "rounds to tenths");
        for at in [ms(0), ms(3_000), ms(65_500), ms(754_300)] {
            assert_eq!(parse_cover_time(&format_cover_time(at)), Some(at));
        }
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[test]
    fn a_limit_has_room_until_it_is_used_up() {
        let limit = |used| PublishingLimit {
            used,
            total: 100,
            window: DAY,
        };
        assert!(limit(0).has_room());
        assert!(limit(99).has_room());
        assert!(!limit(100).has_room());
        assert!(!limit(101).has_room());
        let none = PublishingLimit {
            used: 0,
            total: 0,
            window: DAY,
        };
        assert!(!none.has_room(), "a limit of zero lets nothing through");
    }

    #[test]
    fn a_limit_filled_by_bardo_frees_when_its_oldest_post_leaves_the_window() {
        let limit = PublishingLimit {
            used: 2,
            total: 2,
            window: DAY,
        };
        let now = at(DAY.as_secs() + 10_000);
        let published = [
            at(DAY.as_secs() + 9_000),
            // Left the window already: it no longer counts.
            at(5_000),
            at(20_000),
        ];
        assert_eq!(
            limit.next_try(now, &published),
            NextTry {
                at: at(20_000) + DAY,
                frees: true,
            }
        );
    }

    #[test]
    fn a_limit_shared_with_other_apps_is_read_again_within_the_hour() {
        let limit = PublishingLimit {
            used: 50,
            total: 50,
            window: DAY,
        };
        let now = at(DAY.as_secs() + 10_000);
        // Bardo's oldest frees a place in under three hours, but one of the
        // other 48 may free one sooner.
        let published = [at(DAY.as_secs() + 9_000), at(20_000)];
        assert_eq!(
            limit.next_try(now, &published),
            NextTry {
                at: now + LIMIT_RECHECK,
                frees: false,
            }
        );
        // One that frees within the hour is when the post goes.
        let published = [at(DAY.as_secs() + 9_000), at(12_000)];
        assert_eq!(
            limit.next_try(now, &published),
            NextTry {
                at: at(12_000) + DAY,
                frees: true,
            }
        );
    }

    #[test]
    fn a_limit_filled_by_other_apps_is_read_again_in_an_hour() {
        let limit = PublishingLimit {
            used: 100,
            total: 100,
            window: DAY,
        };
        let now = at(0);
        let again = NextTry {
            at: now + LIMIT_RECHECK,
            frees: false,
        };
        assert_eq!(limit.next_try(now, &[]), again);
        // A post that just left the window frees nothing in the future.
        let left = now - DAY;
        assert_eq!(limit.next_try(now, &[left]), again);
    }

    #[test]
    fn the_next_try_is_never_now_nor_past_the_window() {
        let limit = PublishingLimit {
            used: 1,
            total: 1,
            window: Duration::from_secs(30),
        };
        let now = at(0);
        assert_eq!(
            limit.next_try(now, &[now - Duration::from_secs(29)]).at,
            now + Duration::from_secs(60),
            "at least a minute"
        );
        let long = PublishingLimit {
            window: Duration::from_secs(600),
            ..limit
        };
        assert_eq!(
            long.next_try(now, &[]).at,
            now + Duration::from_secs(600),
            "never later than the window"
        );
        let endless = PublishingLimit {
            window: Duration::MAX,
            ..limit
        };
        assert_eq!(
            endless.next_try(now, &[now]).at,
            now + LIMIT_RECHECK,
            "a window past the clock reads again in an hour"
        );
    }
}
