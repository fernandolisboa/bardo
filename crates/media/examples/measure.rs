//! Media measurements for the spec's performance targets (issue #18).
//!
//! Builds a synthetic 60 s project (twelve 5 s 1080p30 clips, a voice and a
//! music track), then times proxy builds, preview start, preview frame
//! rate and the final 9:16 render with every working encoder. Prints a
//! Markdown report.
//!
//!     cargo xtask fetch-ffmpeg
//!     cargo run -p bardo-media --release --example measure [-- --work-dir DIR]
//!
//! Run it on an otherwise idle machine; the numbers are wall-clock times.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use bardo_media::ffmpeg::{
    AudioClip, AudioTrack, ClipSource, Ffmpeg, FrameSize, Framing, LoudnessTarget, Output,
    ProxyCodec, ProxySettings, RenderPlan, VideoClip, VideoEncoder,
};

const CLIPS: usize = 12;
const CLIP_SECONDS: u64 = 5;
const PREVIEW: FrameSize = FrameSize::new(960, 540);
const FULL_HD: FrameSize = FrameSize::new(1920, 1080);
const FPS: (u32, u32) = (30, 1);
const STARTS: usize = 5;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let work = match args.iter().position(|arg| arg == "--work-dir") {
        Some(index) => PathBuf::from(&args[index + 1]),
        None => std::env::temp_dir().join("bardo-measure"),
    };
    std::fs::create_dir_all(&work)?;

    let ffmpeg = Ffmpeg::locate()?;
    println!("# Media measurements\n");
    println!(
        "- ffmpeg {} at `{}`",
        ffmpeg.version(),
        ffmpeg.ffmpeg_path().display()
    );
    println!(
        "- {} ({}), {} logical CPUs",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cpus()
    );
    let started = Instant::now();
    let encoders = ffmpeg.detect_encoders();
    let names: Vec<&str> = encoders
        .working
        .iter()
        .map(|encoder| encoder.name())
        .collect();
    println!(
        "- working encoders: {} (detected in {})",
        names.join(", "),
        ms(started.elapsed())
    );
    let best = encoders.best()?;

    let started = Instant::now();
    let sources = make_sources(&ffmpeg, &work, best)?;
    println!(
        "- synthetic project: {CLIPS} x {CLIP_SECONDS} s 1080p30 clips with {} (made in {})\n",
        best.name(),
        secs(started.elapsed())
    );

    println!("## Proxies (540p, all {CLIPS} clips, one after another)\n");
    println!("| Codec | Time | Per clip | Size |");
    println!("| --- | --- | --- | --- |");
    let mut proxy_codecs = vec![("MJPEG (intra)", ProxyCodec::Mjpeg)];
    proxy_codecs.push(("H.264 openh264", ProxyCodec::H264(VideoEncoder::OpenH264)));
    if best != VideoEncoder::OpenH264 {
        proxy_codecs.push(("H.264 best", ProxyCodec::H264(best)));
    }
    let mut proxy_sets = Vec::new();
    for (label, codec) in &proxy_codecs {
        let started = Instant::now();
        let mut proxies = Vec::new();
        let mut bytes = 0;
        for (index, source) in sources.clips.iter().enumerate() {
            let proxy = work.join(format!("proxy-{}-{index}.mkv", short(label)));
            ffmpeg.build_proxy(
                source,
                &proxy,
                ProxySettings {
                    height: 540,
                    codec: *codec,
                },
                &(),
            )?;
            bytes += std::fs::metadata(&proxy)?.len();
            proxies.push(proxy);
        }
        let took = started.elapsed();
        println!(
            "| {label} | {} | {} | {:.1} MB |",
            secs(took),
            ms(took / CLIPS as u32),
            bytes as f64 / 1e6
        );
        proxy_sets.push((*label, proxies));
    }

    println!("\n## Preview start (spawn to first decoded frame, median of {STARTS})\n");
    println!("| Source | Size | From 0 s | From 31.7 s |");
    println!("| --- | --- | --- | --- |");
    for (label, proxies) in &proxy_sets {
        let plan = plan(proxies, &sources);
        let row: Vec<String> = [Duration::ZERO, Duration::from_millis(31_700)]
            .into_iter()
            .map(|from| median_start(|| ffmpeg.preview(&plan, from, PREVIEW, FPS)))
            .collect::<Result<_, _>>()?;
        println!(
            "| {label} proxies, 60 s timeline | 960x540 | {} | {} |",
            row[0], row[1]
        );
    }
    let originals = plan(&sources.clips, &sources);
    let row: Vec<String> = [Duration::ZERO, Duration::from_millis(31_700)]
        .into_iter()
        .map(|from| median_start(|| ffmpeg.preview(&originals, from, FULL_HD, FPS)))
        .collect::<Result<_, _>>()?;
    println!(
        "| originals, 60 s timeline | 1920x1080 | {} | {} |",
        row[0], row[1]
    );

    println!("\n## Preview frame rate (decode + scale + BGRA pipe, 300 frames, no display)\n");
    println!("| Source | Size | Frames/s | MB/s through the pipe |");
    println!("| --- | --- | --- | --- |");
    for (label, proxies) in &proxy_sets {
        let plan = plan(proxies, &sources);
        let (fps, rate) = throughput(&ffmpeg, &plan, PREVIEW)?;
        println!("| {label} proxies | 960x540 | {fps:.0} | {rate:.0} |");
    }
    let (fps, rate) = throughput(&ffmpeg, &originals, FULL_HD)?;
    println!("| originals | 1920x1080 | {fps:.0} | {rate:.0} |");

    println!("\n## Final render (60 s, 1080x1920 30 fps, 12 Mbit/s, two audio tracks, -14 LUFS)\n");
    println!("| Encoder | Time | Speed | Integrated loudness |");
    println!("| --- | --- | --- | --- |");
    let target = LoudnessTarget {
        integrated: -14.0,
        true_peak: -1.5,
        range: 11.0,
    };
    for encoder in &encoders.working {
        let output = Output {
            size: FrameSize::new(1080, 1920),
            fps: FPS,
            encoder: *encoder,
            video_bitrate: 12_000_000,
            audio_bitrate: 192_000,
        };
        let destination = work.join(format!("render-{}.mp4", encoder.name()));
        let started = Instant::now();
        ffmpeg.render(&originals, &output, Some(target), &destination, &())?;
        let took = started.elapsed();
        let loudness = ffmpeg.measure_loudness(&destination, &())?;
        println!(
            "| {} | {} | {:.1}x realtime | {:.1} LUFS |",
            encoder.name(),
            secs(took),
            originals.duration().as_secs_f64() / took.as_secs_f64(),
            loudness.integrated
        );
    }
    println!("\nWork files in `{}`.", work.display());
    Ok(())
}

struct Sources {
    clips: Vec<PathBuf>,
    voice: PathBuf,
    music: PathBuf,
}

fn make_sources(
    ffmpeg: &Ffmpeg,
    work: &Path,
    encoder: VideoEncoder,
) -> Result<Sources, Box<dyn std::error::Error>> {
    let patterns = ["testsrc2", "smptehdbars", "rgbtestsrc", "testsrc"];
    let mut clips = Vec::new();
    for index in 0..CLIPS {
        let clip = work.join(format!("source-{index}.mp4"));
        if !clip.exists() {
            let pattern = patterns[index % patterns.len()];
            let video = format!("{pattern}=s=1920x1080:r=30:d={CLIP_SECONDS}");
            let audio = format!("sine=f={}:r=48000:d={CLIP_SECONDS}", 200 + 50 * index);
            run(Command::new(ffmpeg.ffmpeg_path())
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    &video,
                ])
                .args(["-f", "lavfi", "-i", &audio])
                .args(encoder.args(12_000_000, 60))
                .args(["-c:a", "aac", "-shortest"])
                .arg(&clip))?;
        }
        clips.push(clip);
    }
    let total = CLIPS as u64 * CLIP_SECONDS;
    let voice = work.join("voice.mp3");
    let music = work.join("music.mp3");
    for (path, source) in [
        (&voice, format!("sine=f=220:r=44100:d={total},volume=0.5")),
        (&music, format!("anoisesrc=c=pink:r=44100:d={total}:a=0.1")),
    ] {
        if !path.exists() {
            run(Command::new(ffmpeg.ffmpeg_path())
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    &source,
                ])
                .args(["-c:a", "libmp3lame", "-b:a", "128k"])
                .arg(path))?;
        }
    }
    Ok(Sources {
        clips,
        voice,
        music,
    })
}

fn plan(clips: &[PathBuf], sources: &Sources) -> RenderPlan {
    let total = Duration::from_secs(CLIPS as u64 * CLIP_SECONDS);
    let audio = |source: &Path, gain_db: f32| AudioTrack {
        clips: vec![AudioClip {
            source: source.to_path_buf(),
            start: Duration::ZERO,
            duration: total,
            at: Duration::ZERO,
            gain_db: 0.0,
        }],
        gain_db,
    };
    RenderPlan {
        video: clips
            .iter()
            .map(|clip| VideoClip {
                source: ClipSource::Video(clip.clone()),
                start: Duration::ZERO,
                duration: Duration::from_secs(CLIP_SECONDS),
                framing: Framing::Crop { x: 0.5, y: 0.5 },
            })
            .collect(),
        audio: vec![audio(&sources.voice, 0.0), audio(&sources.music, -12.0)],
    }
}

fn median_start(
    mut open: impl FnMut() -> Result<bardo_media::ffmpeg::FrameStream, bardo_media::ffmpeg::MediaError>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut times = Vec::new();
    for _ in 0..STARTS {
        let started = Instant::now();
        let stream = open()?;
        stream.next_frame().ok_or("no frame")?;
        times.push(started.elapsed());
    }
    times.sort();
    Ok(ms(times[STARTS / 2]))
}

fn throughput(
    ffmpeg: &Ffmpeg,
    plan: &RenderPlan,
    size: FrameSize,
) -> Result<(f64, f64), Box<dyn std::error::Error>> {
    let stream = ffmpeg.preview(plan, Duration::ZERO, size, FPS)?;
    stream.next_frame().ok_or("no frame")?;
    let started = Instant::now();
    let frames = stream.take(300).count();
    let took = started.elapsed().as_secs_f64();
    let fps = frames as f64 / took;
    Ok((fps, fps * size.bgra_len() as f64 / 1e6))
}

fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{command:?} failed: {status}").into());
    }
    Ok(())
}

fn short(label: &str) -> String {
    label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn cpus() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

fn ms(duration: Duration) -> String {
    format!("{} ms", duration.as_millis())
}

fn secs(duration: Duration) -> String {
    format!("{:.1} s", duration.as_secs_f64())
}
