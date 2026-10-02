//! The ffmpeg sidecar against the short clips in `tests/fixtures`. Needs
//! the pinned build: `cargo xtask fetch-ffmpeg` (CI does it).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bardo_media::ffmpeg::{
    AudioClip, AudioTrack, ClipSource, Dip, Duck, Ffmpeg, FrameSize, Framing, LoudnessTarget,
    MIN_VERSION, MediaError, Monitor, Output, ProxyCodec, ProxySettings, RenderPlan, VideoClip,
    VideoEncoder,
};

fn ffmpeg() -> Ffmpeg {
    Ffmpeg::locate().expect("ffmpeg not found: run `cargo xtask fetch-ffmpeg`")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn secs(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bardo-media-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn close(actual: Duration, expected: f64, tolerance: f64) -> bool {
    (actual.as_secs_f64() - expected).abs() <= tolerance
}

/// Records progress and stops once asked.
#[derive(Default)]
struct Recorder {
    progress: RefCell<Vec<f32>>,
    stop: Cell<bool>,
}

impl Monitor for Recorder {
    fn should_stop(&self) -> bool {
        self.stop.get()
    }

    fn progress(&self, fraction: f32) {
        self.progress.borrow_mut().push(fraction);
    }
}

/// Two clips with sound, a voice and a quieter music bed: the shape of a
/// short with narration, in 3.5 seconds.
fn short_plan() -> RenderPlan {
    RenderPlan {
        video: vec![
            VideoClip {
                source: ClipSource::Video(fixture("clip-a.mp4")),
                start: secs(0.5),
                duration: secs(1.5),
                framing: Framing::Crop { x: 0.5, y: 0.5 },
            },
            VideoClip {
                source: ClipSource::Video(fixture("clip-b.mp4")),
                start: Duration::ZERO,
                duration: secs(2.0),
                framing: Framing::Crop { x: 0.0, y: 0.5 },
            },
        ],
        audio: vec![
            AudioTrack {
                clips: vec![AudioClip {
                    source: fixture("voice-3s.mp3"),
                    start: Duration::ZERO,
                    duration: secs(3.0),
                    at: secs(0.25),
                    gain_db: 0.0,
                    fade_in: Duration::ZERO,
                    fade_out: Duration::ZERO,
                    skipped: Duration::ZERO,
                }],
                gain_db: 0.0,
                duck: None,
            },
            AudioTrack {
                clips: vec![AudioClip {
                    source: fixture("music-4s.mp3"),
                    start: Duration::ZERO,
                    duration: secs(4.0),
                    at: Duration::ZERO,
                    gain_db: 0.0,
                    fade_in: Duration::ZERO,
                    fade_out: Duration::ZERO,
                    skipped: Duration::ZERO,
                }],
                gain_db: -12.0,
                duck: None,
            },
        ],
    }
}

fn vertical(encoder: VideoEncoder) -> Output {
    Output {
        size: FrameSize::new(360, 640),
        fps: (30, 1),
        encoder,
        video_bitrate: 2_000_000,
        audio_bitrate: 128_000,
    }
}

const TARGET: LoudnessTarget = LoudnessTarget {
    integrated: -14.0,
    true_peak: -1.5,
    range: 11.0,
};

#[test]
fn finds_a_recent_enough_build() {
    assert!(ffmpeg().version() >= MIN_VERSION);
}

#[test]
fn probes_streams_and_duration() {
    let info = ffmpeg().probe(&fixture("clip-a.mp4")).unwrap();
    assert!(close(info.duration, 2.0, 0.05), "{:?}", info.duration);
    let video = info.video.unwrap();
    assert_eq!(
        (video.codec.as_str(), video.width, video.height),
        ("h264", 320, 180)
    );
    assert_eq!(video.frame_rate, (30, 1));
    assert_eq!(info.audio.unwrap().codec, "aac");

    let voice = ffmpeg().probe(&fixture("voice-3s.mp3")).unwrap();
    assert!(voice.video.is_none());
    assert!(close(voice.duration, 3.0, 0.1));
}

#[test]
fn probing_a_missing_file_fails_with_ffprobes_message() {
    let error = ffmpeg().probe(&fixture("missing.mp4")).unwrap_err();
    assert!(matches!(error, MediaError::Failed { .. }), "{error}");
}

#[test]
fn grabs_one_frame_scaled() {
    let size = FrameSize::new(160, 90);
    let frame = ffmpeg()
        .frame_at(&fixture("clip-a.mp4"), secs(1.0), size)
        .unwrap();
    assert_eq!(frame.bgra.len(), size.bgra_len());
    assert!(frame.bgra.chunks(4).any(|pixel| pixel[..3] != [0, 0, 0]));
    assert!(frame.bgra.chunks(4).all(|pixel| pixel[3] == 255));
}

#[test]
fn streams_frames_from_a_point_at_a_steady_rate() {
    let stream = ffmpeg()
        .frames(
            &fixture("clip-a.mp4"),
            secs(0.5),
            FrameSize::new(160, 90),
            (10, 1),
        )
        .unwrap();
    let frames: Vec<_> = stream.collect();
    assert!((14..=16).contains(&frames.len()), "{} frames", frames.len());
    assert_eq!(frames[0].at, secs(0.5));
    assert_eq!(frames[1].at, secs(0.6));
}

#[test]
fn waveform_follows_the_audio() {
    let waveform = ffmpeg().waveform(&fixture("voice-3s.mp3"), 10).unwrap();
    assert_eq!(waveform.peaks_per_second, 10);
    assert!(
        (29..=31).contains(&waveform.peaks.len()),
        "{}",
        waveform.peaks.len()
    );
    // The fixture is ffmpeg's test tone (1/8 of full scale) at half volume.
    let middle = waveform.peaks[15];
    assert!((0.05..=0.07).contains(&middle), "{middle}");
}

#[test]
fn builds_proxies_in_both_codecs() {
    let ffmpeg = ffmpeg();
    let dir = scratch("proxy");
    for (codec, name, expected) in [
        (ProxyCodec::Mjpeg, "a-mjpeg.mkv", "mjpeg"),
        (
            ProxyCodec::H264(VideoEncoder::OpenH264),
            "a-h264.mkv",
            "h264",
        ),
    ] {
        let destination = dir.join(name);
        let recorder = Recorder::default();
        ffmpeg
            .build_proxy(
                &fixture("clip-a.mp4"),
                &destination,
                ProxySettings { height: 90, codec },
                &recorder,
            )
            .unwrap();
        let info = ffmpeg.probe(&destination).unwrap();
        let video = info.video.unwrap();
        assert_eq!(
            (video.codec.as_str(), video.width, video.height),
            (expected, 160, 90)
        );
        assert!(info.audio.is_some());
        assert!(close(info.duration, 2.0, 0.1));
        assert!(!dir.join(name.replace(".mkv", ".partial.mkv")).exists());
    }
}

#[test]
fn software_encoding_always_works() {
    let encoders = ffmpeg().detect_encoders();
    assert!(
        encoders.working.contains(&VideoEncoder::OpenH264),
        "{encoders:?}"
    );
    assert!(!encoders.software().unwrap().is_hardware());
}

#[test]
fn renders_a_vertical_short_at_the_loudness_target() {
    let ffmpeg = ffmpeg();
    let dir = scratch("render");
    let destination = dir.join("short.mp4");
    let recorder = Recorder::default();
    ffmpeg
        .render(
            &short_plan(),
            &vertical(VideoEncoder::OpenH264),
            Some(TARGET),
            &destination,
            &recorder,
        )
        .unwrap();

    let info = ffmpeg.probe(&destination).unwrap();
    assert!(close(info.duration, 3.5, 0.1), "{:?}", info.duration);
    let video = info.video.unwrap();
    assert_eq!(
        (video.codec.as_str(), video.width, video.height),
        ("h264", 360, 640)
    );
    assert_eq!(video.frame_rate, (30, 1));
    let audio = info.audio.unwrap();
    assert_eq!(
        (audio.codec.as_str(), audio.sample_rate, audio.channels),
        ("aac", 48_000, 2)
    );

    let loudness = ffmpeg.measure_loudness(&destination, &()).unwrap();
    assert!(
        (loudness.integrated - TARGET.integrated).abs() <= 1.0,
        "integrated {} LUFS",
        loudness.integrated
    );
    assert!(
        loudness.true_peak <= TARGET.true_peak + 0.5,
        "true peak {}",
        loudness.true_peak
    );

    let progress = recorder.progress.borrow();
    assert!(
        progress.windows(2).all(|pair| pair[0] <= pair[1]),
        "{progress:?}"
    );
    assert!(
        progress.last().is_some_and(|last| *last > 0.9),
        "{progress:?}"
    );
}

#[test]
fn renders_with_every_working_encoder() {
    // NVENC, AMF, QSV and Media Foundation join on machines that have them;
    // OpenH264 everywhere.
    let ffmpeg = ffmpeg();
    let dir = scratch("encoders");
    for encoder in ffmpeg.detect_encoders().working {
        let destination = dir.join(format!("{}.mp4", encoder.name()));
        ffmpeg
            .render(&short_plan(), &vertical(encoder), None, &destination, &())
            .unwrap_or_else(|error| panic!("{}: {error}", encoder.name()));
        let video = ffmpeg.probe(&destination).unwrap().video.unwrap();
        assert_eq!(
            (video.codec.as_str(), video.height),
            ("h264", 640),
            "{}",
            encoder.name()
        );
    }
}

/// 4 s of music over black: what the mix tests render.
fn music_plan() -> RenderPlan {
    RenderPlan {
        video: vec![VideoClip {
            source: ClipSource::Black,
            start: Duration::ZERO,
            duration: secs(4.0),
            framing: Framing::Fit,
        }],
        audio: vec![AudioTrack {
            clips: vec![AudioClip {
                source: fixture("music-4s.mp3"),
                start: Duration::ZERO,
                duration: secs(4.0),
                at: Duration::ZERO,
                gain_db: 0.0,
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
                skipped: Duration::ZERO,
            }],
            gain_db: 0.0,
            duck: None,
        }],
    }
}

/// The level of a rendered file's sound from `from` for `length` seconds,
/// in dBFS (RMS of the mono downmix).
fn level(ffmpeg: &Ffmpeg, path: &Path, from: f64, length: f64) -> f64 {
    let output = std::process::Command::new(ffmpeg.ffmpeg_path())
        .args([
            "-v",
            "error",
            "-ss",
            &format!("{from}"),
            "-t",
            &format!("{length}"),
        ])
        .arg("-i")
        .arg(path)
        .args(["-vn", "-f", "f32le", "-ac", "1", "-ar", "48000", "pipe:1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert!(!samples.is_empty());
    let power = samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64;
    10.0 * power.max(1e-12).log10()
}

fn render_audio(ffmpeg: &Ffmpeg, plan: &RenderPlan, destination: &Path) {
    ffmpeg
        .render(
            plan,
            &vertical(VideoEncoder::OpenH264),
            None,
            destination,
            &(),
        )
        .unwrap();
}

#[test]
fn a_ducked_render_is_quieter_by_the_depth_only_where_it_dips() {
    let ffmpeg = ffmpeg();
    let dir = scratch("duck");
    let plain = dir.join("plain.mp4");
    let ducked = dir.join("ducked.mp4");
    render_audio(&ffmpeg, &music_plan(), &plain);
    let mut plan = music_plan();
    plan.audio[0].duck = Some(Duck {
        depth_db: 12.0,
        dips: vec![Dip {
            start: 1.35,
            full: 1.5,
            release: 2.5,
            end: 3.0,
        }],
    });
    render_audio(&ffmpeg, &plan, &ducked);

    let difference = |from: f64, length: f64| {
        level(&ffmpeg, &plain, from, length) - level(&ffmpeg, &ducked, from, length)
    };
    let before = difference(0.2, 1.0);
    let under = difference(1.6, 0.8);
    let after = difference(3.1, 0.8);
    assert!(
        before.abs() < 0.5,
        "untouched before the dip: {before:.2} dB"
    );
    assert!(
        (under - 12.0).abs() < 1.0,
        "down by the depth: {under:.2} dB"
    );
    assert!(after.abs() < 0.5, "back up after it: {after:.2} dB");
    // Ramps, not steps: half way down the release it is about half as low.
    let ramp = difference(2.7, 0.1);
    assert!(ramp > 3.0 && ramp < 9.0, "on the way back up: {ramp:.2} dB");
}

#[test]
fn rendered_fades_rise_from_and_fall_to_silence() {
    let ffmpeg = ffmpeg();
    let dir = scratch("fades");
    let plain = dir.join("plain.mp4");
    let faded = dir.join("faded.mp4");
    render_audio(&ffmpeg, &music_plan(), &plain);
    let mut plan = music_plan();
    let clip = &mut plan.audio[0].clips[0];
    clip.fade_in = secs(1.0);
    clip.fade_out = secs(1.0);
    render_audio(&ffmpeg, &plan, &faded);

    let difference = |from: f64, length: f64| {
        level(&ffmpeg, &plain, from, length) - level(&ffmpeg, &faded, from, length)
    };
    assert!(difference(0.0, 0.1) > 15.0, "starts from silence");
    assert!(difference(1.5, 1.0).abs() < 0.5, "full level between");
    assert!(difference(3.9, 0.1) > 15.0, "ends in silence");
    // Half way in, about -6 dB.
    let half = difference(0.45, 0.1);
    assert!(half > 4.0 && half < 8.0, "half way in: {half:.2} dB");
}

#[test]
fn cancelling_a_render_leaves_no_file() {
    let dir = scratch("cancel");
    let destination = dir.join("short.mp4");
    let recorder = Recorder::default();
    recorder.stop.set(true);
    let error = ffmpeg()
        .render(
            &short_plan(),
            &vertical(VideoEncoder::OpenH264),
            None,
            &destination,
            &recorder,
        )
        .unwrap_err();
    assert!(matches!(error, MediaError::Cancelled), "{error}");
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
}

#[test]
fn previews_the_timeline_from_the_playhead() {
    let size = FrameSize::new(180, 320);
    let stream = ffmpeg()
        .preview(&short_plan(), secs(1.0), size, (30, 1))
        .unwrap();
    let first = stream.next_frame().unwrap();
    assert_eq!(first.at, secs(1.0));
    assert_eq!(first.bgra.len(), size.bgra_len());
    let rest = stream.count();
    // 2.5 s of timeline left at 30 fps.
    assert!((73..=76).contains(&(rest + 1)), "{} frames", rest + 1);
}

#[test]
fn a_still_proxy_is_a_small_jpeg_of_the_image() {
    let ffmpeg = ffmpeg();
    let dir = scratch("still-proxy");
    let destination = dir.join("proxy-still.jpg");
    ffmpeg
        .build_still_proxy(&fixture("still-640x360.png"), &destination, 180)
        .unwrap();
    let video = ffmpeg.probe(&destination).unwrap().video.unwrap();
    assert_eq!(
        (video.codec.as_str(), video.width, video.height),
        ("mjpeg", 320, 180)
    );
    assert!(!dir.join("proxy-still.partial.jpg").exists());
}

/// A rough cut: a still, a clip asked to run longer than it is (2 s of
/// clip for 3 s), and a stretch with nothing to show.
fn rough_cut_plan() -> RenderPlan {
    RenderPlan {
        video: vec![
            VideoClip {
                source: ClipSource::Still(fixture("still-640x360.png")),
                start: Duration::ZERO,
                duration: secs(1.0),
                framing: Framing::Fit,
            },
            VideoClip {
                source: ClipSource::Video(fixture("clip-b.mp4")),
                start: Duration::ZERO,
                duration: secs(3.0),
                framing: Framing::Fit,
            },
            VideoClip {
                source: ClipSource::Black,
                start: Duration::ZERO,
                duration: secs(0.5),
                framing: Framing::Fit,
            },
        ],
        audio: vec![AudioTrack {
            clips: vec![AudioClip {
                source: fixture("voice-3s.mp3"),
                start: Duration::ZERO,
                duration: secs(3.0),
                at: Duration::ZERO,
                gain_db: 0.0,
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
                skipped: Duration::ZERO,
            }],
            gain_db: 0.0,
            duck: None,
        }],
    }
}

#[test]
fn preview_holds_a_short_clip_and_keeps_every_frame_in_time() {
    let size = FrameSize::new(160, 90);
    let stream = ffmpeg()
        .preview(&rough_cut_plan(), Duration::ZERO, size, (30, 1))
        .unwrap();
    let frames: Vec<_> = stream.collect();
    // 1 s + 3 s + 0.5 s at 30 fps, exactly: a clip that ends early holds
    // its last frame instead of pulling the rest of the cut forward.
    assert_eq!(frames.len(), 135);
    let last = frames.last().unwrap();
    assert!(last.bgra.chunks(4).all(|pixel| pixel[..3] == [0, 0, 0]));
    let held = &frames[100];
    assert!(held.bgra.chunks(4).any(|pixel| pixel[..3] != [0, 0, 0]));
}

#[test]
fn preview_audio_streams_the_mix_from_the_playhead() {
    let stream = ffmpeg()
        .preview_audio(&rough_cut_plan(), secs(2.5))
        .unwrap();
    assert_eq!((stream.channels, stream.sample_rate), (2, 48_000));
    let mut samples = 0u64;
    let mut loudest = 0.0f32;
    while let Some(chunk) = stream.next_chunk() {
        assert_eq!(chunk.len() % 2, 0);
        samples += chunk.len() as u64;
        loudest = chunk.iter().fold(loudest, |peak, s| peak.max(s.abs()));
    }
    // The 4.5 s cut from 2.5 s on: 2 s of stereo at 48 kHz.
    assert!(
        close(stream.duration_of(samples), 2.0, 0.03),
        "{samples} samples"
    );
    assert!(loudest > 0.01, "the voice is heard");
}
