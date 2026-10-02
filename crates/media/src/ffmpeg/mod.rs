//! ffmpeg as sidecar executables (ADR-0007): Bardo runs the bundled
//! `ffmpeg` and `ffprobe` as child processes and talks to them through
//! arguments, pipes and `-progress` output. Nothing links against the
//! libraries, so a crash or a hang in a decoder stays in the child, and
//! cancelling is killing it.
//!
//! Every call here blocks until the child is done; the app runs them on its
//! job threads and passes a [`Monitor`] for progress and cancel.

mod encoders;
mod frames;
mod probe;
mod process;
mod proxy;
mod render;
mod waveform;

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub use encoders::{Encoders, VideoEncoder};
pub use frames::{FramePoll, FrameSize, FrameStream, VideoFrame};
pub use probe::{AudioStream, MediaInfo, VideoStream};
pub use proxy::{ProxyCodec, ProxySettings};
pub use render::{
    AudioClip, AudioTrack, ClipSource, Framing, Loudness, LoudnessTarget, Output, RenderPlan,
    VideoClip,
};
pub use waveform::Waveform;

/// The oldest ffmpeg whose flags and filters Bardo relies on. The bundled
/// build is newer (`crates/media/ffmpeg.toml`); this only guards against
/// an old system ffmpeg picked up from PATH during development.
pub const MIN_VERSION: Version = Version { major: 7, minor: 1 };

/// The environment variable naming a folder with `ffmpeg` and `ffprobe`.
/// `.cargo/config.toml` points it at `.ffmpeg/bin` for cargo runs.
pub const DIR_VARIABLE: &str = "BARDO_FFMPEG_DIR";

/// Where the installer puts the bundled build, next to the Bardo executable.
const INSTALLED_DIR: &str = "ffmpeg";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    /// Reads the version from the first line of `ffmpeg -version`:
    /// "ffmpeg version n8.1.3-9-g29e6…" (BtbN), "ffmpeg version 8.1.3-essentials…"
    /// (gyan.dev), "ffmpeg version 6.1.1-3ubuntu5" (distributions). Builds
    /// from git master ("N-12345-g…") carry no release number.
    pub fn parse(version_output: &str) -> Option<Version> {
        let line = version_output.lines().next()?;
        let rest = line.split("version ").nth(1)?;
        let number = rest.strip_prefix('n').unwrap_or(rest);
        let mut parts = number.split(|c: char| !c.is_ascii_digit());
        let major = parts.next()?.parse().ok()?;
        let minor = parts
            .next()
            .and_then(|minor| minor.parse().ok())
            .unwrap_or(0);
        Some(Version { major, minor })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Progress and cancel for a long ffmpeg run. The app implements it over a
/// job's context.
pub trait Monitor {
    /// Polled about every 100 ms; `true` kills the child.
    fn should_stop(&self) -> bool {
        false
    }

    /// How far the work is, 0.0 to 1.0.
    fn progress(&self, _fraction: f32) {}
}

/// A run nobody watches or cancels.
impl Monitor for () {}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("ffmpeg was not found (looked in {})", .tried.join(", "))]
    NotFound { tried: Vec<String> },
    #[error("ffmpeg {found} is too old; Bardo needs {MIN_VERSION} or newer")]
    TooOld { found: Version },
    #[error("could not tell which ffmpeg version {path} is")]
    UnknownVersion { path: PathBuf },
    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    /// ffmpeg ran and failed; `log` is the end of what it printed.
    #[error("{program} failed ({status}): {log}")]
    Failed {
        program: String,
        status: String,
        log: String,
    },
    #[error("cancelled")]
    Cancelled,
    /// ffmpeg's output was not what Bardo expected.
    #[error("unexpected ffmpeg output: {0}")]
    Parse(String),
    #[error("cannot render this plan: {0}")]
    InvalidPlan(&'static str),
    #[error("no video encoder works on this machine")]
    NoEncoder,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A located ffmpeg build: the two executables and their version.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    version: Version,
}

impl Ffmpeg {
    /// Finds ffmpeg in, by order: the folder [`DIR_VARIABLE`] names, an
    /// `ffmpeg` folder next to the running executable (the installed
    /// layout), and PATH. The first folder holding both executables, at
    /// [`MIN_VERSION`] or newer, wins.
    pub fn locate() -> Result<Ffmpeg, MediaError> {
        let mut tried = Vec::new();
        let mut candidates = Vec::new();
        if let Some(dir) = std::env::var_os(DIR_VARIABLE) {
            candidates.push((PathBuf::from(dir), true));
        }
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(INSTALLED_DIR)))
        {
            candidates.push((dir, true));
        }
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&path).map(|dir| (dir, false)));
            tried.push("PATH".to_string());
        }
        for (dir, named) in candidates {
            let ffmpeg = dir.join(executable("ffmpeg"));
            let ffprobe = dir.join(executable("ffprobe"));
            if !(ffmpeg.is_file() && ffprobe.is_file()) {
                if named {
                    tried.push(dir.display().to_string());
                }
                continue;
            }
            // An old ffmpeg on PATH must not hide the bundled one further on.
            match Ffmpeg::with(ffmpeg, ffprobe) {
                Ok(found) => return Ok(found),
                Err(error) => tried.push(format!("{} ({error})", dir.display())),
            }
        }
        Err(MediaError::NotFound { tried })
    }

    /// The build in `dir`.
    pub fn in_dir(dir: &Path) -> Result<Ffmpeg, MediaError> {
        Ffmpeg::with(
            dir.join(executable("ffmpeg")),
            dir.join(executable("ffprobe")),
        )
    }

    fn with(ffmpeg: PathBuf, ffprobe: PathBuf) -> Result<Ffmpeg, MediaError> {
        let output = hidden(Command::new(&ffmpeg))
            .arg("-version")
            .stdin(Stdio::null())
            .output()
            .map_err(|source| MediaError::Spawn {
                program: ffmpeg.display().to_string(),
                source,
            })?;
        let version =
            Version::parse(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
                MediaError::UnknownVersion {
                    path: ffmpeg.clone(),
                }
            })?;
        if version < MIN_VERSION {
            return Err(MediaError::TooOld { found: version });
        }
        Ok(Ffmpeg {
            ffmpeg,
            ffprobe,
            version,
        })
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn ffmpeg_path(&self) -> &Path {
        &self.ffmpeg
    }

    /// `ffmpeg` with the options every run shares: no banner, no reading
    /// from the console, errors only on stderr.
    fn ffmpeg(&self) -> Command {
        let mut command = hidden(Command::new(&self.ffmpeg));
        command
            .args(["-hide_banner", "-nostdin", "-loglevel", "error"])
            .stdin(Stdio::null());
        command
    }

    fn ffprobe(&self) -> Command {
        let mut command = hidden(Command::new(&self.ffprobe));
        command
            .args(["-hide_banner", "-loglevel", "error"])
            .stdin(Stdio::null());
        command
    }
}

fn executable(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Keeps a console window from flashing up for every child: Bardo is a GUI
/// app, and Windows gives a console program started from one a new console
/// unless told otherwise.
fn hidden(command: Command) -> Command {
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = command;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    command
}

/// Seconds as ffmpeg reads them, with microsecond precision.
fn seconds(duration: Duration) -> String {
    format!("{}.{:06}", duration.as_secs(), duration.subsec_micros())
}

fn path_arg(path: &Path) -> &OsStr {
    path.as_os_str()
}

/// "clip.mp4" → "clip.partial.mp4" in the same folder: written there and
/// renamed when done (same disk, so the rename is atomic), a cancelled or
/// failed run never leaves a file that looks finished.
fn partial_path(destination: &Path) -> PathBuf {
    let stem = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = destination
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    destination.with_file_name(format!("{stem}.partial{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_release_versions_from_common_builds() {
        let cases = [
            (
                "ffmpeg version n8.1.3-9-g29e619e767-20260930 Copyright",
                (8, 1),
            ),
            (
                "ffmpeg version 9.0.2-essentials_build-www.gyan.dev Copyright",
                (9, 0),
            ),
            (
                "ffmpeg version 6.1.1-3ubuntu5 Copyright (c) 2000-2023",
                (6, 1),
            ),
            ("ffmpeg version 7 Copyright", (7, 0)),
        ];
        for (line, (major, minor)) in cases {
            assert_eq!(
                Version::parse(line),
                Some(Version { major, minor }),
                "{line}"
            );
        }
    }

    #[test]
    fn master_builds_have_no_version() {
        assert_eq!(
            Version::parse("ffmpeg version N-121000-gabcdef Copyright"),
            None
        );
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn versions_order_by_major_then_minor() {
        assert!(Version { major: 7, minor: 1 } >= MIN_VERSION);
        assert!(Version { major: 8, minor: 0 } > MIN_VERSION);
        assert!(Version { major: 7, minor: 0 } < MIN_VERSION);
        assert!(Version { major: 6, minor: 9 } < MIN_VERSION);
    }

    #[test]
    fn partial_file_sits_next_to_the_destination() {
        assert_eq!(
            partial_path(Path::new("out/video.mp4")),
            PathBuf::from("out/video.partial.mp4")
        );
        assert_eq!(
            partial_path(Path::new("proxy.mkv")),
            PathBuf::from("proxy.partial.mkv")
        );
    }

    #[test]
    fn seconds_keep_microseconds() {
        assert_eq!(seconds(Duration::from_millis(1500)), "1.500000");
        assert_eq!(seconds(Duration::from_micros(42)), "0.000042");
    }
}
