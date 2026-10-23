//! Frames in and out through the ffmpeg executable, as `bardo-media` does.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, bail};

/// An RGB frame, 8 bits per channel, rows top to bottom.
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgb: vec![0; width * height * 3],
        }
    }

    /// Bilinear sample at a pixel position (pixel centers at .5), clamped
    /// to the edge. Returns the three channels as 0..255 floats.
    pub fn sample(&self, x: f32, y: f32) -> [f32; 3] {
        let x = (x - 0.5).clamp(0.0, (self.width - 1) as f32);
        let y = (y - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let px = |xx: usize, yy: usize, c: usize| self.rgb[(yy * self.width + xx) * 3 + c] as f32;
        let mut out = [0.0; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
            let bottom = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
            *o = top * (1.0 - fy) + bottom * fy;
        }
        out
    }

    pub fn put(&mut self, x: i64, y: i64, rgb: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let i = (y as usize * self.width + x as usize) * 3;
            self.rgb[i..i + 3].copy_from_slice(&rgb);
        }
    }
}

pub struct Probe {
    pub width: usize,
    pub height: usize,
    pub fps: f64,
}

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

pub fn probe(path: &Path) -> Result<Probe> {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=width,height,r_frame_rate",
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
    let s = &json["streams"][0];
    let rate = s["r_frame_rate"].as_str().unwrap_or("30/1");
    let (num, den) = rate.split_once('/').unwrap_or((rate, "1"));
    Ok(Probe {
        width: s["width"].as_u64().context("width")? as usize,
        height: s["height"].as_u64().context("height")? as usize,
        fps: num.parse::<f64>()? / den.parse::<f64>()?,
    })
}

/// Decoded frames of a file at its own size, one after another.
pub struct Reader {
    child: Child,
    stdout: ChildStdout,
    width: usize,
    height: usize,
}

impl Reader {
    pub fn open(
        path: &Path,
        width: usize,
        height: usize,
        max_frames: Option<usize>,
    ) -> Result<Self> {
        let mut cmd = Command::new(ffmpeg());
        cmd.args(["-v", "error", "-nostdin", "-i"]).arg(path);
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
            width,
            height,
        })
    }

    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        let mut frame = Frame::new(self.width, self.height);
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

/// Writes RGB frames to a video file (MPEG-4 Part 2, which every ffmpeg
/// build has) for looking at the results.
pub struct Writer {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Writer {
    pub fn create(path: &Path, width: usize, height: usize, fps: f64) -> Result<Self> {
        let mut child = Command::new(ffmpeg())
            .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"])
            .args([
                "-s",
                &format!("{width}x{height}"),
                "-r",
                &format!("{fps}"),
                "-i",
                "-",
            ])
            .args(["-c:v", "mpeg4", "-q:v", "3", "-pix_fmt", "yuv420p"])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .context("starting ffmpeg to write")?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    pub fn write(&mut self, frame: &Frame) -> Result<()> {
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
