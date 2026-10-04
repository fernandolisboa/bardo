//! TikTok draft upload rules (PRD stories 84, 85, 87 and 89; ADR-0008):
//! what a file must be for TikTok to take it, how it is cut into chunks
//! for the Content Posting API's file upload, and how many drafts may wait
//! in the creator's inbox.
//!
//! The specs and the chunk rules come from the media transfer guide, the
//! cap from the upload reference (both checked 2026-10-04):
//! <https://developers.tiktok.com/doc/content-posting-api-media-transfer-guide>,
//! <https://developers.tiktok.com/doc/content-posting-api-reference-upload-video>.
//! Where a size could be read in decimal or binary megabytes, the limit
//! here is the stricter of the two, so a file that passes here passes
//! however TikTok counts them.

use std::io::{self, Read};
use std::time::{Duration, SystemTime};

use crate::{Container, mp4_layout};

/// The longest video TikTok takes from apps: 10 minutes.
pub const TIKTOK_MAX_DURATION: Duration = Duration::from_secs(10 * 60);
/// The largest file: 4 GB.
pub const TIKTOK_MAX_BYTES: u64 = 4 * 1000 * 1000 * 1000;
/// Frame rates TikTok takes, in frames per second (inclusive).
pub const TIKTOK_FPS: (f64, f64) = (23.0, 60.0);
/// The shortest and longest side of the picture, in pixels (inclusive).
pub const TIKTOK_SIDE: (u32, u32) = (360, 4096);

/// The smallest chunk but the only one of a small file: 5 MB.
pub const TIKTOK_MIN_CHUNK: u64 = 5 * 1024 * 1024;
/// The largest chunk but the last: 64 MB.
pub const TIKTOK_MAX_CHUNK: u64 = 64 * 1000 * 1000;
/// The largest last chunk, which takes the rest of the file: 128 MB.
pub const TIKTOK_MAX_LAST_CHUNK: u64 = 128 * 1000 * 1000;
/// The most chunks a file goes in.
pub const TIKTOK_MAX_CHUNKS: u64 = 1000;
/// How much one request sends: 10 MiB, small enough to hold in memory and
/// to lose little when a request fails, and within TikTok's range.
pub const TIKTOK_CHUNK: u64 = 10 * 1024 * 1024;

/// How many shares may wait in a creator's inbox for them to post, in any
/// 24 hours (`spam_risk_too_many_pending_share`).
pub const TIKTOK_PENDING_DRAFTS: u32 = 5;
/// The moving window the pending drafts are counted over.
pub const TIKTOK_DRAFT_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

/// A file's container as far as TikTok cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoContainer {
    Mp4,
    Mov,
    WebM,
}

/// The EBML magic every Matroska and WebM file starts with.
const EBML: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];

/// Reads which container `file` is: an MP4 or MOV by its `ftyp` box, or a
/// WebM by its EBML header naming the `webm` document type (another
/// Matroska file is not one). `None` for anything else.
pub fn video_container(file: &mut dyn Read) -> io::Result<Option<VideoContainer>> {
    let mut head = [0u8; 64];
    let mut filled = 0;
    while filled < head.len() {
        match file.read(&mut head[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    let head = &head[..filled];
    if head.starts_with(&EBML) {
        let webm = head.windows(4).any(|window| window == b"webm");
        return Ok(webm.then_some(VideoContainer::WebM));
    }
    let layout = mp4_layout(&mut io::Cursor::new(head).chain(file))?;
    Ok(layout.container.map(|container| match container {
        Container::Mp4 => VideoContainer::Mp4,
        Container::Mov => VideoContainer::Mov,
    }))
}

/// What the TikTok checks read of a rendered file.
#[derive(Debug, Clone, PartialEq)]
pub struct TikTokFile {
    pub container: Option<VideoContainer>,
    /// The first video stream's codec as ffprobe names it (`h264`, `hevc`,
    /// `vp8`, `vp9`); `None` without a video stream.
    pub codec: Option<String>,
    pub width: u32,
    pub height: u32,
    /// Frames per second of the video stream.
    pub fps: f64,
    pub duration: Duration,
    pub size: u64,
}

/// One way a file falls short of what TikTok takes. Each one blocks the
/// upload: TikTok would refuse the file after it arrived.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TikTokSpecProblem {
    /// Bardo could not read the file's streams.
    Unreadable,
    /// Not an MP4, MOV or WebM file.
    Container,
    /// The file has no video stream.
    NoVideo,
    /// The video is not H.264, H.265, VP8 or VP9: the codec as ffprobe
    /// names it.
    Codec(String),
    /// Outside 23 to 60 frames per second: the rate in hundredths.
    FrameRate(u32),
    /// A side under 360 or over 4096 pixels: the width and height.
    PictureSize(u32, u32),
    /// Longer than 10 minutes.
    TooLong(Duration),
    /// Over 4 GB: the size in bytes.
    TooBig(u64),
}

impl TikTokSpecProblem {
    /// Every `code`, in the order `check_tiktok` lists them.
    pub const CODES: [&'static str; 8] = [
        "unreadable",
        "container",
        "no_video",
        "codec",
        "frame_rate",
        "picture_size",
        "too_long",
        "too_big",
    ];

    /// Stable name, for texts.
    pub fn code(&self) -> &'static str {
        match self {
            TikTokSpecProblem::Unreadable => "unreadable",
            TikTokSpecProblem::Container => "container",
            TikTokSpecProblem::NoVideo => "no_video",
            TikTokSpecProblem::Codec(_) => "codec",
            TikTokSpecProblem::FrameRate(_) => "frame_rate",
            TikTokSpecProblem::PictureSize(..) => "picture_size",
            TikTokSpecProblem::TooLong(_) => "too_long",
            TikTokSpecProblem::TooBig(_) => "too_big",
        }
    }
}

/// Everything about `file` TikTok would refuse, in the order the review
/// lists them.
pub fn check_tiktok(file: &TikTokFile) -> Vec<TikTokSpecProblem> {
    let mut problems = Vec::new();
    if file.container.is_none() {
        problems.push(TikTokSpecProblem::Container);
    }
    match file.codec.as_deref() {
        Some("h264" | "hevc" | "vp8" | "vp9") => {}
        Some(other) => problems.push(TikTokSpecProblem::Codec(other.to_owned())),
        None => problems.push(TikTokSpecProblem::NoVideo),
    }
    if file.codec.is_some() {
        if !(TIKTOK_FPS.0..=TIKTOK_FPS.1).contains(&file.fps) {
            let hundredths = (file.fps.max(0.0) * 100.0).round().min(f64::from(u32::MAX));
            problems.push(TikTokSpecProblem::FrameRate(hundredths as u32));
        }
        let side = TIKTOK_SIDE.0..=TIKTOK_SIDE.1;
        if !side.contains(&file.width) || !side.contains(&file.height) {
            problems.push(TikTokSpecProblem::PictureSize(file.width, file.height));
        }
    }
    if file.duration > TIKTOK_MAX_DURATION {
        problems.push(TikTokSpecProblem::TooLong(file.duration));
    }
    if file.size > TIKTOK_MAX_BYTES {
        problems.push(TikTokSpecProblem::TooBig(file.size));
    }
    problems
}

/// Why a file cannot be cut into chunks TikTok takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChunkPlanError {
    #[error("the file is empty")]
    Empty,
    #[error("a chunk of {0} bytes is outside TikTok's range")]
    ChunkSize(u64),
    #[error("the file would go in more chunks, or a larger last one, than TikTok takes")]
    TooBig,
}

/// How a file goes to TikTok: `total_chunk_count` chunks of `chunk_size`
/// bytes, sent in order, the last one taking the rest of the file. What
/// the upload's init declares, and what each `PUT` sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPlan {
    pub video_size: u64,
    pub chunk_size: u64,
    pub total_chunk_count: u64,
}

impl ChunkPlan {
    /// The plan for a file of `video_size` bytes in chunks of `chunk`
    /// bytes. A file no larger than one chunk goes whole. Otherwise the
    /// count is `video_size / chunk`, rounded down, as TikTok counts it, and
    /// the last chunk takes the rest.
    pub fn new(video_size: u64, chunk: u64) -> Result<Self, ChunkPlanError> {
        if video_size == 0 {
            return Err(ChunkPlanError::Empty);
        }
        if !(TIKTOK_MIN_CHUNK..=TIKTOK_MAX_CHUNK).contains(&chunk) {
            return Err(ChunkPlanError::ChunkSize(chunk));
        }
        let plan = if video_size <= chunk {
            Self {
                video_size,
                chunk_size: video_size,
                total_chunk_count: 1,
            }
        } else {
            Self {
                video_size,
                chunk_size: chunk,
                total_chunk_count: video_size / chunk,
            }
        };
        if plan.total_chunk_count > TIKTOK_MAX_CHUNKS || plan.last_len() > TIKTOK_MAX_LAST_CHUNK {
            return Err(ChunkPlanError::TooBig);
        }
        Ok(plan)
    }

    /// The plan Bardo uploads with: chunks of [`TIKTOK_CHUNK`].
    pub fn for_video(video_size: u64) -> Result<Self, ChunkPlanError> {
        Self::new(video_size, TIKTOK_CHUNK)
    }

    fn last_len(&self) -> u64 {
        self.video_size - (self.total_chunk_count - 1) * self.chunk_size
    }

    /// Where chunk `index` (from 0) starts, and how many bytes it has.
    /// `None` past the last one.
    pub fn chunk(&self, index: u64) -> Option<(u64, u64)> {
        if index >= self.total_chunk_count {
            return None;
        }
        let first = index * self.chunk_size;
        let len = if index + 1 == self.total_chunk_count {
            self.last_len()
        } else {
            self.chunk_size
        };
        Some((first, len))
    }

    /// The chunk that starts at `offset`; `None` when no chunk starts
    /// there (the whole file included).
    pub fn chunk_at(&self, offset: u64) -> Option<u64> {
        offset
            .is_multiple_of(self.chunk_size)
            .then_some(offset / self.chunk_size)
            .filter(|index| *index < self.total_chunk_count)
    }
}

/// Whether one more draft may go to the creator's inbox now, given when
/// Bardo's earlier drafts of the account started (any order): TikTok keeps
/// at most [`TIKTOK_PENDING_DRAFTS`] waiting in any 24 hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftRoom {
    /// `left` more may go now.
    Room { left: u32 },
    /// Full: the next place frees `at`, when the window passes the draft
    /// that frees it.
    Full { at: SystemTime },
}

/// The room left in the inbox at `now` for drafts Bardo started at `sent`.
/// A draft the creator already posted or discarded still counts until its
/// 24 hours pass: Bardo cannot tell, and TikTok counts within the window.
pub fn draft_room(sent: &[SystemTime], now: SystemTime) -> DraftRoom {
    let start = now
        .checked_sub(TIKTOK_DRAFT_WINDOW)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut counted: Vec<SystemTime> = sent.iter().copied().filter(|at| *at > start).collect();
    let cap = TIKTOK_PENDING_DRAFTS as usize;
    if counted.len() < cap {
        return DraftRoom::Room {
            left: (cap - counted.len()) as u32,
        };
    }
    counted.sort();
    // With `cap` or more in the window, a place frees once all but
    // `cap - 1` have left it.
    let frees = counted[counted.len() - cap];
    DraftRoom::Full {
        at: frees
            .checked_add(TIKTOK_DRAFT_WINDOW)
            .unwrap_or(now)
            .max(now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn file() -> TikTokFile {
        TikTokFile {
            container: Some(VideoContainer::Mp4),
            codec: Some("h264".into()),
            width: 1080,
            height: 1920,
            fps: 30.0,
            duration: Duration::from_secs(45),
            size: 40 * MIB,
        }
    }

    #[test]
    fn a_bardo_render_meets_tiktoks_specs() {
        assert_eq!(check_tiktok(&file()), vec![]);
        for codec in ["hevc", "vp8", "vp9"] {
            let f = TikTokFile {
                codec: Some(codec.into()),
                ..file()
            };
            assert_eq!(check_tiktok(&f), vec![], "{codec}");
        }
        for container in [VideoContainer::Mov, VideoContainer::WebM] {
            let f = TikTokFile {
                container: Some(container),
                ..file()
            };
            assert_eq!(check_tiktok(&f), vec![], "{container:?}");
        }
    }

    #[test]
    fn each_spec_tiktok_refuses_is_listed_in_order() {
        let f = TikTokFile {
            container: None,
            codec: Some("prores".into()),
            width: 320,
            height: 5000,
            fps: 120.0,
            duration: Duration::from_secs(10 * 60 + 1),
            size: TIKTOK_MAX_BYTES + 1,
        };
        assert_eq!(
            check_tiktok(&f),
            vec![
                TikTokSpecProblem::Container,
                TikTokSpecProblem::Codec("prores".into()),
                TikTokSpecProblem::FrameRate(12_000),
                TikTokSpecProblem::PictureSize(320, 5000),
                TikTokSpecProblem::TooLong(Duration::from_secs(601)),
                TikTokSpecProblem::TooBig(TIKTOK_MAX_BYTES + 1),
            ]
        );
        let codes: Vec<&str> = check_tiktok(&f).iter().map(|p| p.code()).collect();
        let ordered: Vec<&str> = TikTokSpecProblem::CODES
            .iter()
            .copied()
            .filter(|code| codes.contains(code))
            .collect();
        assert_eq!(codes, ordered, "CODES lists them in the same order");
    }

    #[test]
    fn the_limits_themselves_pass() {
        let edge = TikTokFile {
            width: 360,
            height: 4096,
            fps: 23.0,
            duration: TIKTOK_MAX_DURATION,
            size: TIKTOK_MAX_BYTES,
            ..file()
        };
        assert_eq!(check_tiktok(&edge), vec![]);
        let fast = TikTokFile { fps: 60.0, ..edge };
        assert_eq!(check_tiktok(&fast), vec![]);
        let slow = TikTokFile {
            fps: 29.97 - 7.0,
            ..file()
        };
        assert_eq!(
            check_tiktok(&slow),
            vec![TikTokSpecProblem::FrameRate(2297)]
        );
    }

    #[test]
    fn a_file_without_video_says_so_once() {
        let audio = TikTokFile {
            codec: None,
            width: 0,
            height: 0,
            fps: 0.0,
            ..file()
        };
        assert_eq!(check_tiktok(&audio), vec![TikTokSpecProblem::NoVideo]);
    }

    fn mp4(brand: &[u8; 4]) -> Vec<u8> {
        let mut bytes = 16u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(brand);
        bytes.extend_from_slice(&[0, 0, 2, 0]);
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(b"moov");
        bytes
    }

    #[test]
    fn containers_are_read_from_the_files_first_bytes() {
        let read = |bytes: &[u8]| video_container(&mut io::Cursor::new(bytes.to_vec())).unwrap();
        assert_eq!(read(&mp4(b"isom")), Some(VideoContainer::Mp4));
        assert_eq!(read(&mp4(b"qt  ")), Some(VideoContainer::Mov));
        let mut webm = EBML.to_vec();
        webm.extend_from_slice(&[0x9f, 0x42, 0x86, 0x81, 0x01, 0x42, 0x82, 0x84]);
        webm.extend_from_slice(b"webm");
        assert_eq!(read(&webm), Some(VideoContainer::WebM));
        let mut mkv = EBML.to_vec();
        mkv.extend_from_slice(&[0x42, 0x82, 0x88]);
        mkv.extend_from_slice(b"matroska");
        assert_eq!(read(&mkv), None, "Matroska is not WebM");
        assert_eq!(read(b"RIFF\0\0\0\0AVI "), None);
        assert_eq!(read(b""), None);
    }

    #[test]
    fn a_small_file_goes_in_one_chunk_of_its_own_size() {
        for size in [1, 3 * MIB, TIKTOK_CHUNK] {
            let plan = ChunkPlan::for_video(size).unwrap();
            assert_eq!(
                plan,
                ChunkPlan {
                    video_size: size,
                    chunk_size: size,
                    total_chunk_count: 1
                }
            );
            assert_eq!(plan.chunk(0), Some((0, size)));
            assert_eq!(plan.chunk(1), None);
        }
    }

    #[test]
    fn the_count_is_rounded_down_and_the_last_chunk_takes_the_rest() {
        // TikTok's own example: 50,000,123 bytes in chunks of 10,000,000.
        let plan = ChunkPlan::new(50_000_123, 10_000_000).unwrap();
        assert_eq!(plan.total_chunk_count, 5);
        assert_eq!(plan.chunk(3), Some((30_000_000, 10_000_000)));
        assert_eq!(plan.chunk(4), Some((40_000_000, 10_000_123)));
        assert_eq!(plan.chunk(5), None);

        // Between one and two chunks: one chunk with the whole file.
        let plan = ChunkPlan::for_video(TIKTOK_CHUNK + 1).unwrap();
        assert_eq!(plan.total_chunk_count, 1);
        assert_eq!(plan.chunk_size, TIKTOK_CHUNK);
        assert_eq!(plan.chunk(0), Some((0, TIKTOK_CHUNK + 1)));

        let exact = ChunkPlan::for_video(3 * TIKTOK_CHUNK).unwrap();
        assert_eq!(exact.total_chunk_count, 3);
        assert_eq!(exact.chunk(2), Some((2 * TIKTOK_CHUNK, TIKTOK_CHUNK)));
    }

    #[test]
    fn the_chunks_cover_the_file_once_within_tiktoks_sizes() {
        for size in [7 * MIB, 25 * MIB + 3, 999 * MIB + 17, TIKTOK_MAX_BYTES] {
            let plan = ChunkPlan::for_video(size).unwrap();
            let mut next = 0;
            for index in 0..plan.total_chunk_count {
                let (first, len) = plan.chunk(index).unwrap();
                assert_eq!(first, next, "{size}: in order, no gap");
                assert_eq!(plan.chunk_at(first), Some(index));
                let last = index + 1 == plan.total_chunk_count;
                if plan.total_chunk_count > 1 && !last {
                    assert!((TIKTOK_MIN_CHUNK..=TIKTOK_MAX_CHUNK).contains(&len));
                }
                assert!(len <= TIKTOK_MAX_LAST_CHUNK);
                next += len;
            }
            assert_eq!(next, size);
            assert!(plan.total_chunk_count <= TIKTOK_MAX_CHUNKS);
            assert_eq!(plan.chunk_at(size), None, "nothing starts at the end");
        }
    }

    #[test]
    fn only_chunk_starts_are_chunks() {
        let plan = ChunkPlan::for_video(3 * TIKTOK_CHUNK + 5).unwrap();
        assert_eq!(plan.chunk_at(0), Some(0));
        assert_eq!(plan.chunk_at(TIKTOK_CHUNK), Some(1));
        assert_eq!(plan.chunk_at(TIKTOK_CHUNK + 1), None);
        assert_eq!(plan.chunk_at(3 * TIKTOK_CHUNK), None, "inside the last one");
    }

    #[test]
    fn plans_outside_tiktoks_rules_are_refused() {
        assert_eq!(ChunkPlan::for_video(0), Err(ChunkPlanError::Empty));
        assert_eq!(
            ChunkPlan::new(100 * MIB, MIB),
            Err(ChunkPlanError::ChunkSize(MIB))
        );
        assert_eq!(
            ChunkPlan::new(100 * MIB, 65 * MIB),
            Err(ChunkPlanError::ChunkSize(65 * MIB))
        );
        assert_eq!(
            ChunkPlan::new(1001 * TIKTOK_MIN_CHUNK, TIKTOK_MIN_CHUNK),
            Err(ChunkPlanError::TooBig)
        );
        assert!(ChunkPlan::new(1000 * TIKTOK_MIN_CHUNK, TIKTOK_MIN_CHUNK).is_ok());
    }

    fn at(hours: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + hours * 3600)
    }

    #[test]
    fn five_drafts_in_24_hours_fill_the_inbox() {
        let now = at(30);
        assert_eq!(draft_room(&[], now), DraftRoom::Room { left: 5 });
        let four = [at(29), at(10), at(20), at(25)];
        assert_eq!(draft_room(&four, now), DraftRoom::Room { left: 1 });
        let five = [at(29), at(10), at(20), at(25), at(28)];
        assert_eq!(
            draft_room(&five, now),
            DraftRoom::Full { at: at(34) },
            "the oldest leaves the window 24 hours after it went"
        );
    }

    #[test]
    fn drafts_older_than_the_window_do_not_count() {
        let now = at(30);
        let sent = [at(6), at(2), at(10), at(20), at(25), at(28)];
        assert_eq!(draft_room(&sent, now), DraftRoom::Room { left: 1 });
        // Exactly 24 hours old has left the window.
        let sent = [at(6), at(10), at(20), at(25), at(28)];
        assert_eq!(draft_room(&sent, now), DraftRoom::Room { left: 1 });
    }

    #[test]
    fn with_more_than_five_a_place_frees_once_enough_have_left() {
        let now = at(30);
        let sent = [at(7), at(8), at(9), at(20), at(25), at(28), at(29)];
        // Seven counted: three must leave, the third at hour 9.
        assert_eq!(draft_room(&sent, now), DraftRoom::Full { at: at(33) });
    }
}
