//! Frames in and out through the ffmpeg executable, as `bardo-media` does.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::image::Image;

/// The ffmpeg next to the repository (`cargo xtask fetch-ffmpeg`), else the
/// one on PATH. `FFMPEG` overrides both.
pub fn ffmpeg() -> PathBuf {
    if let Some(path) = std::env::var_os("FFMPEG") {
        return path.into();
    }
    let exe = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.ffmpeg/bin")
        .join(exe);
    if bundled.exists() {
        bundled
    } else {
        exe.into()
    }
}

fn ffprobe() -> PathBuf {
    let ffmpeg = ffmpeg();
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    match ffmpeg.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name),
        _ => name.into(),
    }
}

pub struct Probe {
    /// The size frames decode at: turned upright, as ffmpeg turns a phone's
    /// rotated recording.
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    /// The frame rate as ffmpeg takes it (`30000/1001`); frames are read
    /// and written at this constant rate.
    pub rate: String,
    pub has_audio: bool,
}

/// The frame rate from ffprobe's average rate, else its base rate: a
/// rational such as `30000/1001`, and its value. `0/0` and the like are
/// skipped.
fn frame_rate(avg: Option<&str>, base: Option<&str>) -> Option<(String, f64)> {
    [avg, base].into_iter().flatten().find_map(|rate| {
        let (num, den) = rate.split_once('/').unwrap_or((rate, "1"));
        let fps = num.parse::<f64>().ok()? / den.parse::<f64>().ok()?;
        (fps.is_finite() && fps > 0.0).then(|| (rate.to_string(), fps))
    })
}

/// The displayed size of a stored `width` × `height` frame turned by
/// `rotation` degrees.
fn upright(width: usize, height: usize, rotation: f64) -> (usize, usize) {
    if (rotation.round() as i64).rem_euclid(180) == 90 {
        (height, width)
    } else {
        (width, height)
    }
}

pub fn probe(path: &Path) -> Result<Probe> {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-show_entries"])
        .args([
            "stream=codec_type,width,height,r_frame_rate,avg_frame_rate:\
             stream_side_data=rotation:stream_tags=rotate",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .context("running ffprobe")?;
    if !out.status.success() {
        bail!("ffprobe failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = json["streams"].as_array().context("no streams")?;
    let v = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .context("no video stream")?;
    let (rate, fps) = frame_rate(v["avg_frame_rate"].as_str(), v["r_frame_rate"].as_str())
        .unwrap_or(("30/1".into(), 30.0));
    let rotation = v["side_data_list"]
        .as_array()
        .and_then(|list| list.iter().find_map(|d| d["rotation"].as_f64()))
        .or_else(|| v["tags"]["rotate"].as_str().and_then(|r| r.parse().ok()))
        .unwrap_or(0.0);
    let (width, height) = upright(
        v["width"].as_u64().context("width")? as usize,
        v["height"].as_u64().context("height")? as usize,
        rotation,
    );
    Ok(Probe {
        width,
        height,
        fps,
        rate,
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
    })
}

/// The size upstream gives a source image (`resize_to_limit`): the longer
/// side at most `max_dim`, then both sides cut down to even.
pub fn limited_size(
    width: usize,
    height: usize,
    max_dim: usize,
) -> ((usize, usize), (usize, usize)) {
    let scaled = if max_dim > 0 && width.max(height) > max_dim {
        if height > width {
            (
                (width as f64 * max_dim as f64 / height as f64) as usize,
                max_dim,
            )
        } else {
            (
                max_dim,
                (height as f64 * max_dim as f64 / width as f64) as usize,
            )
        }
    } else {
        (width, height)
    };
    (scaled, (scaled.0 - scaled.0 % 2, scaled.1 - scaled.1 % 2))
}

/// A still image, scaled as upstream scales a source.
pub fn read_image(path: &Path, max_dim: usize) -> Result<Image> {
    let p = probe(path)?;
    let ((sw, sh), (w, h)) = limited_size(p.width, p.height, max_dim);
    let out = Command::new(ffmpeg())
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("scale={sw}:{sh}:flags=bilinear,crop={w}:{h}:0:0"),
        ])
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .context("running ffmpeg")?;
    if !out.status.success() || out.stdout.len() != w * h * 3 {
        bail!(
            "ffmpeg could not read {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(Image {
        width: w,
        height: h,
        rgb: out.stdout,
    })
}

/// Decoded frames of a video at its own size, one after another.
pub struct Reader {
    child: Child,
    stdout: ChildStdout,
    width: usize,
    height: usize,
}

impl Reader {
    pub fn open(path: &Path, p: &Probe, max_frames: Option<usize>) -> Result<Self> {
        let mut cmd = Command::new(ffmpeg());
        cmd.args(["-v", "error", "-nostdin", "-i"]).arg(path);
        // A phone records at a variable rate; the frames are read at the
        // constant rate the result is written at, so they keep to the sound.
        cmd.args(["-fps_mode", "cfr", "-r", &p.rate]);
        if let Some(n) = max_frames {
            cmd.args(["-frames:v", &n.to_string()]);
        }
        cmd.args(["-an", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("starting ffmpeg")?;
        let stdout = child.stdout.take().context("ffmpeg stdout")?;
        Ok(Self {
            child,
            stdout,
            width: p.width,
            height: p.height,
        })
    }

    /// The frames scaled as `read_image` scales a still to `max_dim`.
    pub fn open_scaled(
        path: &Path,
        p: &Probe,
        max_dim: usize,
        max_frames: Option<usize>,
    ) -> Result<Self> {
        let ((sw, sh), (width, height)) = limited_size(p.width, p.height, max_dim);
        let mut cmd = Command::new(ffmpeg());
        cmd.args(["-v", "error", "-nostdin", "-i"]).arg(path);
        cmd.args(["-fps_mode", "cfr", "-r", &p.rate]);
        if let Some(n) = max_frames {
            cmd.args(["-frames:v", &n.to_string()]);
        }
        cmd.args([
            "-vf",
            &format!("scale={sw}:{sh}:flags=bilinear,crop={width}:{height}:0:0"),
        ])
        .args(["-an", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("starting ffmpeg")?;
        let stdout = child.stdout.take().context("ffmpeg stdout")?;
        Ok(Self {
            child,
            stdout,
            width,
            height,
        })
    }

    pub fn next_frame(&mut self) -> Result<Option<Image>> {
        let mut frame = Image::new(self.width, self.height);
        let mut filled = 0;
        while filled < frame.rgb.len() {
            let n = self.stdout.read(&mut frame.rgb[filled..])?;
            if n == 0 {
                if filled == 0 {
                    return Ok(None);
                }
                bail!("ffmpeg ended mid-frame");
            }
            filled += n;
        }
        Ok(Some(frame))
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The best H.264 encoder this ffmpeg has (the pinned LGPL build has
/// OpenH264, a GPL build x264), else MPEG-4 Part 2, which every build has.
fn encoder() -> Vec<&'static str> {
    let listed = Command::new(ffmpeg())
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let has = |name: &str| {
        listed
            .lines()
            .any(|l| l.split_whitespace().nth(1) == Some(name))
    };
    if has("libx264") {
        vec!["-c:v", "libx264", "-crf", "16", "-preset", "medium"]
    } else if has("libopenh264") {
        vec!["-c:v", "libopenh264", "-b:v", "16M"]
    } else {
        vec!["-c:v", "mpeg4", "-q:v", "2"]
    }
}

/// Writes RGB frames to an MP4, with the audio of `audio` when given.
pub struct Writer {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Writer {
    pub fn create(
        path: &Path,
        width: usize,
        height: usize,
        rate: &str,
        audio: Option<&Path>,
    ) -> Result<Self> {
        let mut cmd = Command::new(ffmpeg());
        cmd.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"])
            .args(["-s", &format!("{width}x{height}"), "-r", rate, "-i", "-"]);
        if let Some(a) = audio {
            cmd.arg("-i")
                .arg(a)
                .args(["-map", "0:v", "-map", "1:a", "-c:a", "aac", "-shortest"]);
        }
        cmd.args(encoder()).args(["-pix_fmt", "yuv420p"]).arg(path);
        let mut child = cmd
            .stdin(Stdio::piped())
            .spawn()
            .context("starting ffmpeg to write")?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    pub fn write(&mut self, frame: &Image) -> Result<()> {
        self.stdin
            .as_mut()
            .context("writer closed")?
            .write_all(&frame.rgb)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());
        let status = self.child.wait()?;
        if !status.success() {
            bail!("ffmpeg could not write the video");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_limited_and_even_as_upstream_makes_them() {
        assert_eq!(limited_size(768, 768, 1280), ((768, 768), (768, 768)));
        assert_eq!(limited_size(1920, 1080, 1280), ((1280, 720), (1280, 720)));
        assert_eq!(limited_size(1083, 1921, 1280), ((721, 1280), (720, 1280)));
        assert_eq!(limited_size(1001, 777, 0), ((1001, 777), (1000, 776)));
    }

    #[test]
    fn frame_rate_prefers_the_average_and_skips_unknown_rates() {
        let rate = |avg, base| frame_rate(avg, base).map(|(r, f)| (r, (f * 1000.0).round()));
        assert_eq!(
            rate(Some("30000/1001"), Some("60/1")),
            Some(("30000/1001".into(), 29970.0))
        );
        assert_eq!(
            rate(Some("0/0"), Some("25/1")),
            Some(("25/1".into(), 25000.0))
        );
        assert_eq!(rate(None, Some("24")), Some(("24".into(), 24000.0)));
        assert_eq!(rate(Some("0/0"), None), None);
    }

    #[test]
    fn a_phone_recording_turned_a_quarter_is_read_upright() {
        assert_eq!(upright(1920, 1080, -90.0), (1080, 1920));
        assert_eq!(upright(1920, 1080, 90.0), (1080, 1920));
        assert_eq!(upright(1920, 1080, 270.0), (1080, 1920));
        assert_eq!(upright(1920, 1080, 180.0), (1920, 1080));
        assert_eq!(upright(1920, 1080, 0.0), (1920, 1080));
    }
}
