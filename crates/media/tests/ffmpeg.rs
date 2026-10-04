//! The ffmpeg sidecar against the short clips in `tests/fixtures`. Needs
//! the pinned build: `cargo xtask fetch-ffmpeg` (CI does it).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bardo_domain::{CaptionStyle, VideoCodec};
use bardo_media::ffmpeg::{
    AudioClip, AudioTrack, CaptionLine, CaptionTrack, ClipSource, Dip, Duck, Ffmpeg, FrameSize,
    Framing, LoudnessTarget, MIN_VERSION, MediaError, Monitor, Output, ProxyCodec, ProxySettings,
    RenderPlan, VideoClip, VideoEncoder,
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
        captions: None,
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
fn probing_a_file_that_is_not_media_fails() {
    let path = scratch("not-media").join("song.mp3");
    std::fs::write(&path, b"this is not an mp3 at all, only text").unwrap();
    let error = ffmpeg().probe(&path).unwrap_err();
    assert!(matches!(error, MediaError::Failed { .. }), "{error}");
}

#[test]
fn a_still_image_probes_as_a_picture_with_no_length() {
    // Imports tell images apart from video by this.
    let probed = ffmpeg().probe(&fixture("still-640x360.png"));
    match probed {
        Err(MediaError::Parse(_)) => {}
        Ok(info) => assert!(info.duration.is_zero() && info.audio.is_none(), "{info:?}"),
        Err(error) => panic!("{error}"),
    }
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
    assert!(!encoders.software(VideoCodec::H264).unwrap().is_hardware());
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

    // A Reel as Instagram takes it: MP4 with its index first (fast start),
    // H.264, 23 to 60 fps.
    let layout = bardo_domain::mp4_layout(&mut std::fs::File::open(&destination).unwrap()).unwrap();
    let reel = bardo_domain::ReelFile {
        layout,
        codec: Some(video.codec.clone()),
        width: video.width,
        fps: video.fps(),
        duration: info.duration,
        size: std::fs::metadata(&destination).unwrap().len(),
    };
    assert_eq!(bardo_domain::check_reel(&reel), [], "{layout:?}");

    let loudness = ffmpeg.measure_loudness(&destination, &()).unwrap();
    assert!(
        (loudness.integrated - TARGET.integrated).abs() <= 1.0,
        "integrated {} LUFS",
        loudness.integrated
    );
    // Under the -1 dBTP networks ask for, after AAC.
    assert!(
        loudness.true_peak <= -1.0,
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
fn the_limiter_holds_the_peaks_a_gain_lifts_over_the_ceiling() {
    // A -12 dBTP ceiling under a -9 LUFS target: the gain lifts the
    // short's peaks well over it, and the limiter holds them under it
    // through AAC.
    let ffmpeg = ffmpeg();
    let dir = scratch("limited");
    let destination = dir.join("short.mp4");
    let target = LoudnessTarget {
        integrated: -9.0,
        true_peak: -12.0,
    };
    let plan = short_plan();
    let mix = ffmpeg.measure_mix_loudness(&plan, &()).unwrap();
    let lifted = mix.true_peak + (target.integrated - mix.integrated);
    assert!(lifted > target.true_peak + 3.0, "{mix:?}");
    ffmpeg
        .render(
            &plan,
            &vertical(VideoEncoder::OpenH264),
            Some(target),
            &destination,
            &(),
        )
        .unwrap();
    let loudness = ffmpeg.measure_loudness(&destination, &()).unwrap();
    assert!(
        loudness.true_peak <= target.true_peak + 0.5,
        "true peak {}",
        loudness.true_peak
    );
    assert!(
        loudness.integrated <= target.integrated,
        "integrated {} LUFS",
        loudness.integrated
    );
}

#[test]
fn renders_with_every_working_encoder() {
    // NVENC, AMF, QSV and Media Foundation join on machines that have them;
    // OpenH264 and Kvazaar everywhere.
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
            (encoder.codec().code(), 640),
            "{}",
            encoder.name()
        );
    }
}

#[test]
fn renders_hevc_in_software_at_a_quieter_target() {
    // A preset may ask for HEVC and its own loudness target; Kvazaar is the
    // software HEVC encoder every machine has.
    let ffmpeg = ffmpeg();
    let encoder = ffmpeg
        .detect_encoders()
        .software(VideoCodec::Hevc)
        .expect("Kvazaar ships in the LGPL build");
    let dir = scratch("hevc");
    let destination = dir.join("short.mp4");
    let target = LoudnessTarget {
        integrated: -16.0,
        true_peak: -1.0,
    };
    ffmpeg
        .render(
            &short_plan(),
            &vertical(encoder),
            Some(target),
            &destination,
            &(),
        )
        .unwrap();
    let info = ffmpeg.probe(&destination).unwrap();
    let video = info.video.unwrap();
    assert_eq!(
        (video.codec.as_str(), video.width, video.height),
        ("hevc", 360, 640)
    );
    assert!(close(info.duration, 3.5, 0.1), "{:?}", info.duration);
    let loudness = ffmpeg.measure_loudness(&destination, &()).unwrap();
    assert!(
        (loudness.integrated - target.integrated).abs() <= 1.0,
        "integrated {} LUFS",
        loudness.integrated
    );
}

#[test]
fn measures_the_mix_before_rendering() {
    let ffmpeg = ffmpeg();
    let mix = ffmpeg.measure_mix_loudness(&short_plan(), &()).unwrap();
    assert!(!mix.is_silent());
    assert!(mix.integrated > -40.0 && mix.integrated < -5.0, "{mix:?}");
    let mut quieter = short_plan();
    for track in &mut quieter.audio {
        track.gain_db -= 6.0;
    }
    let lower = ffmpeg.measure_mix_loudness(&quieter, &()).unwrap();
    assert!(
        ((mix.integrated - lower.integrated) - 6.0).abs() < 0.5,
        "{mix:?} {lower:?}"
    );
}

#[test]
fn a_silent_plan_renders_as_it_is_with_a_loudness_target() {
    let ffmpeg = ffmpeg();
    let mut plan = short_plan();
    plan.audio.clear();
    let mix = ffmpeg.measure_mix_loudness(&plan, &()).unwrap();
    assert!(mix.is_silent(), "{mix:?}");
    let dir = scratch("silent");
    let destination = dir.join("silent.mp4");
    ffmpeg
        .render(
            &plan,
            &vertical(VideoEncoder::OpenH264),
            Some(TARGET),
            &destination,
            &(),
        )
        .unwrap();
    let loudness = ffmpeg.measure_loudness(&destination, &()).unwrap();
    assert!(loudness.is_silent(), "{loudness:?}");
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
        captions: None,
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
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
        captions: None,
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

/// Two seconds of black with one caption from 0.5 s to 1.5 s.
fn captioned_plan(style: CaptionStyle) -> RenderPlan {
    RenderPlan {
        video: vec![VideoClip {
            source: ClipSource::Black,
            start: Duration::ZERO,
            duration: secs(2.0),
            framing: Framing::Fit,
        }],
        audio: Vec::new(),
        captions: Some(CaptionTrack {
            style,
            lines: vec![CaptionLine {
                text: "The keeper {lit} the lamp".into(),
                at: secs(0.5),
                duration: secs(1.0),
            }],
        }),
    }
}

/// How many pixels of a frame are bright: caption letters on black.
fn bright_pixels(bgra: &[u8]) -> usize {
    bgra.chunks(4)
        .filter(|pixel| pixel[..3].iter().any(|channel| *channel > 160))
        .count()
}

#[test]
fn renders_captions_burned_in_while_they_show() {
    let ffmpeg = ffmpeg();
    let dir = scratch("captions");
    for style in CaptionStyle::ALL {
        let destination = dir.join(format!("{style}.mp4"));
        let output = vertical(VideoEncoder::OpenH264);
        ffmpeg
            .render(&captioned_plan(style), &output, None, &destination, &())
            .unwrap_or_else(|error| panic!("{style}: {error}"));
        let frame = |at: f64| {
            ffmpeg
                .frame_at(&destination, secs(at), output.size)
                .unwrap()
        };
        assert_eq!(bright_pixels(&frame(0.2).bgra), 0, "{style}: before");
        let shown = bright_pixels(&frame(1.0).bgra);
        assert!(shown > 200, "{style}: {shown} bright pixels");
        assert_eq!(bright_pixels(&frame(1.8).bgra), 0, "{style}: after");
    }
    let mut hidden = captioned_plan(CaptionStyle::Clean);
    hidden.captions = None;
    let destination = dir.join("none.mp4");
    let output = vertical(VideoEncoder::OpenH264);
    ffmpeg
        .render(&hidden, &output, None, &destination, &())
        .unwrap();
    let frame = ffmpeg
        .frame_at(&destination, secs(1.0), output.size)
        .unwrap();
    assert_eq!(bright_pixels(&frame.bgra), 0, "captions off");
}

#[test]
fn the_preview_shows_captions_from_the_playhead() {
    let size = FrameSize::new(304, 540);
    let stream = ffmpeg()
        .preview(
            &captioned_plan(CaptionStyle::Punch),
            secs(1.0),
            size,
            (30, 1),
        )
        .unwrap();
    let frames: Vec<_> = stream.collect();
    assert_eq!(frames.len(), 30);
    assert!(bright_pixels(&frames[0].bgra) > 100, "at 1.0 s");
    assert_eq!(bright_pixels(&frames[20].bgra), 0, "at 1.67 s");
}

/// The band of `bands-320x180.png` a column falls in: red, green or blue,
/// `None` within a few pixels of where two meet.
fn band_at(column: f64) -> Option<usize> {
    const EDGES: [f64; 2] = [107.0, 213.0];
    if EDGES.iter().any(|edge| (column - edge).abs() < 4.0) {
        return None;
    }
    Some(EDGES.iter().filter(|edge| column > **edge).count())
}

/// Which of red, green and blue a BGRA pixel is, if it is clearly one.
fn colour_of(pixel: &[u8]) -> Option<usize> {
    let (blue, green, red) = (pixel[0], pixel[1], pixel[2]);
    [red, green, blue]
        .iter()
        .position(|channel| *channel > 160)
        .filter(|_| [red, green, blue].iter().filter(|c| **c > 80).count() == 1)
}

#[test]
fn renders_each_clip_through_its_crop_window_at_the_output_size() {
    use bardo_domain::{AspectRatio, CropPosition, PictureSize, crop_window};

    let positions = [0, 250, 500, 1_000];
    let mut video: Vec<VideoClip> = positions
        .iter()
        .map(|&x| {
            let (x, y) = CropPosition::new(x, 500).fractions();
            VideoClip {
                source: ClipSource::Still(fixture("bands-320x180.png")),
                start: Duration::ZERO,
                duration: secs(0.5),
                framing: Framing::Crop { x, y },
            }
        })
        .collect();
    video.push(VideoClip {
        source: ClipSource::Still(fixture("bands-320x180.png")),
        start: Duration::ZERO,
        duration: secs(0.5),
        framing: Framing::Fit,
    });
    let plan = RenderPlan {
        video,
        audio: Vec::new(),
        captions: None,
    };
    let ffmpeg = ffmpeg();
    let destination = scratch("framing").join("framed.mp4");
    let output = vertical(VideoEncoder::OpenH264);
    ffmpeg
        .render(&plan, &output, None, &destination, &())
        .unwrap();
    let video = ffmpeg.probe(&destination).unwrap().video.unwrap();
    assert_eq!((video.width, video.height), (360, 640));

    let size = output.size;
    let pixel = |frame: &[u8], x: u32, y: u32| {
        let at = ((y * size.width + x) * 4) as usize;
        frame[at..at + 4].to_vec()
    };
    for (index, &x) in positions.iter().enumerate() {
        // What the domain says the window is; the picture must match it.
        let window = crop_window(
            PictureSize::new(320, 180),
            AspectRatio::Vertical,
            CropPosition::new(x, 500),
        );
        let frame = ffmpeg
            .frame_at(&destination, secs(index as f64 * 0.5 + 0.25), size)
            .unwrap();
        for fraction in [0.05, 0.25, 0.5, 0.75, 0.95] {
            let column = (fraction * f64::from(size.width)) as u32;
            let source = f64::from(window.x) + fraction * f64::from(window.width);
            let Some(expected) = band_at(source) else {
                continue;
            };
            let found = pixel(&frame.bgra, column, size.height / 2);
            assert_eq!(
                colour_of(&found),
                Some(expected),
                "crop at {x}: column {column} (source {source:.0}) is {found:?}"
            );
        }
    }

    // Fit: the whole picture across the middle, black above and below.
    let frame = ffmpeg.frame_at(&destination, secs(2.25), size).unwrap();
    let middle = size.height / 2;
    let across: Vec<Option<usize>> = [0.15, 0.5, 0.85]
        .iter()
        .map(|fraction| colour_of(&pixel(&frame.bgra, (fraction * 360.0) as u32, middle)))
        .collect();
    assert_eq!(across, [Some(0), Some(1), Some(2)]);
    for row in [20, size.height - 20] {
        assert!(
            pixel(&frame.bgra, size.width / 2, row)[..3]
                .iter()
                .all(|channel| *channel < 30),
            "bars at row {row}"
        );
    }
}
