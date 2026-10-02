//! Frame-to-screen probe for issue #18: plays a video in a GPUI window
//! through the stock image path (ffmpeg decodes to BGRA on a pipe, each
//! frame becomes a `RenderImage`, GPUI uploads it to its sprite atlas) and
//! reports what that costs.
//!
//!     cargo xtask fetch-ffmpeg
//!     cargo run -p bardo-ui --release --example preview_probe -- <video> [WIDTHxHEIGHT] [SECONDS] [static]
//!
//! Defaults: 960x540 for 10 seconds; the video should be at least that
//! long. `static` keeps showing the first frame while the window still
//! redraws every frame: the baseline without per-frame uploads. Prints presented and dropped frames,
//! the time between window frames and the process's CPU use (the ffmpeg
//! child's CPU is separate; see Task Manager).
//!
//! A measurement probe, not editor code: the editor gets frames through
//! `bardo-app` (#20).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bardo_media::ffmpeg::{Ffmpeg, FrameSize, FrameStream, VideoFrame};
use cpu_time::ProcessTime;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, ImageSource, ObjectFit, RenderImage, SharedString, Window, WindowBounds,
    WindowOptions, div, img, px, rgb, size,
};

const FPS: (u32, u32) = (30, 1);

struct Probe {
    stream: FrameStream,
    pending: Option<VideoFrame>,
    shown: Option<Arc<RenderImage>>,
    clock: Option<Instant>,
    cpu_start: ProcessTime,
    last_render: Option<Instant>,
    render_gaps: Vec<Duration>,
    presented: u32,
    dropped: u32,
    stalls: u32,
    shown_at: Duration,
    run_for: Duration,
    size: FrameSize,
    still: bool,
    finished: bool,
}

impl Probe {
    /// Takes the newest frame that is due; frames passed over are dropped.
    fn due_frame(&mut self, now: Duration) -> Option<VideoFrame> {
        let mut newest: Option<VideoFrame> = None;
        while let Some(frame) = self.pending.take().or_else(|| self.stream.try_next_frame()) {
            if frame.at <= now {
                if newest.replace(frame).is_some() {
                    self.dropped += 1;
                }
            } else {
                self.pending = Some(frame);
                break;
            }
        }
        newest
    }

    fn report(&self, elapsed: Duration) {
        let mut gaps = self.render_gaps.clone();
        gaps.sort();
        let at = |q: f64| {
            gaps.get(((gaps.len() as f64 - 1.0) * q) as usize)
                .copied()
                .unwrap_or_default()
        };
        let cpu = self.cpu_start.elapsed().as_secs_f64();
        println!(
            "| Size | Presented | Dropped | Stalls | Window frame p50 / p95 / max | Bardo CPU |"
        );
        println!("| --- | --- | --- | --- | --- | --- |");
        println!(
            "| {}x{} | {} ({:.1} fps) | {} | {} | {:.1} / {:.1} / {:.1} ms | {:.0}% of one core |",
            self.size.width,
            self.size.height,
            self.presented,
            f64::from(self.presented) / elapsed.as_secs_f64(),
            self.dropped,
            self.stalls,
            at(0.5).as_secs_f64() * 1e3,
            at(0.95).as_secs_f64() * 1e3,
            gaps.last().copied().unwrap_or_default().as_secs_f64() * 1e3,
            100.0 * cpu / elapsed.as_secs_f64(),
        );
    }
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if let Some(last) = self.last_render.replace(now) {
            self.render_gaps.push(now - last);
        }
        let clock = *self.clock.get_or_insert(now);
        let elapsed = now - clock;
        if !self.finished {
            let due = if self.still && self.presented > 0 {
                None
            } else {
                self.due_frame(elapsed)
            };
            if let Some(frame) = due {
                let VideoFrame { size, at, bgra } = frame;
                // RenderImage holds BGRA in an RGBA-typed buffer; no swizzle.
                let buffer = image::RgbaImage::from_raw(size.width, size.height, bgra)
                    .expect("frame matches its size");
                let next = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                // Each frame is a new atlas entry; free the last one.
                if let Some(old) = self.shown.replace(next) {
                    cx.drop_image(old, Some(window));
                }
                self.presented += 1;
                self.shown_at = at;
            } else if !self.still
                && self.presented > 0
                && elapsed.saturating_sub(self.shown_at) > Duration::from_millis(50)
            {
                self.stalls += 1;
            }
            if elapsed >= self.run_for {
                self.finished = true;
                self.report(elapsed);
                cx.quit();
            } else {
                window.request_animation_frame();
            }
        }

        let picture = match &self.shown {
            Some(image) => img(ImageSource::Render(image.clone()))
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .child(SharedString::from("waiting for ffmpeg…"))
                .into_any_element(),
        };
        div().size_full().bg(rgb(0x101010)).child(picture)
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let video = PathBuf::from(
        args.get(1)
            .ok_or_else(|| anyhow::anyhow!("usage: preview_probe <video> [WxH] [seconds]"))?,
    );
    let frame_size = match args.get(2).and_then(|text| text.split_once('x')) {
        Some((width, height)) => FrameSize::new(width.parse()?, height.parse()?),
        None => FrameSize::new(960, 540),
    };
    let run_for = Duration::from_secs(
        args.get(3)
            .map(|text| text.parse())
            .transpose()?
            .unwrap_or(10),
    );
    let still = args.get(4).is_some_and(|mode| mode == "static");

    let ffmpeg = Ffmpeg::locate()?;
    let started = Instant::now();
    let stream = ffmpeg.frames(&video, Duration::ZERO, frame_size, FPS)?;
    let first = stream
        .next_frame()
        .ok_or_else(|| anyhow::anyhow!("no frames in {}", video.display()))?;
    println!(
        "First frame decoded in {} ms.\n",
        started.elapsed().as_millis()
    );

    gpui_kit::application().run(move |cx: &mut App| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1280.), px(760.)),
                cx,
            ))),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| {
            cx.new(|_| Probe {
                stream,
                pending: Some(first),
                shown: None,
                clock: None,
                cpu_start: ProcessTime::now(),
                last_render: None,
                render_gaps: Vec::new(),
                presented: 0,
                dropped: 0,
                stalls: 0,
                shown_at: Duration::ZERO,
                run_for,
                size: frame_size,
                still,
                finished: false,
            })
        })
        .expect("open the probe window");
        cx.activate(true);
    });
    Ok(())
}
