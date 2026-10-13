//! Settings › About (issue #135): Bardo's version, the FFmpeg build it
//! ships and where its source is (the LGPL asks for both), and the
//! third-party notices the installer puts next to `bardo.exe`.

use std::path::{Path, PathBuf};

use crate::Bardo;

/// The ffmpeg pin (ADR-0007), read when Bardo is built.
const FFMPEG_MANIFEST: &str = include_str!("../../media/ffmpeg.toml");

/// Next to `bardo.exe` in the installed layout (`cargo xtask package`).
pub const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.html";
/// In the installed `ffmpeg` folder, as FFmpeg's build ships it.
pub const FFMPEG_LICENSE_FILE: &str = "LICENSE.txt";

/// The LGPL's text.
pub const LGPL_URL: &str = "https://www.gnu.org/licenses/lgpl-3.0.html";

/// What Settings › About shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct About {
    /// Bardo's version, e.g. `0.1.0`.
    pub version: &'static str,
    /// The FFmpeg version Bardo ships, e.g. `8.1.3`.
    pub ffmpeg_version: String,
    /// Bardo's copy of the source that FFmpeg was built from.
    pub ffmpeg_source: String,
    /// The third-party notices, when installed (not in development builds).
    pub notices: Option<PathBuf>,
    /// FFmpeg's license, when installed.
    pub ffmpeg_license: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
struct Manifest {
    version: String,
    mirror: String,
    source: Source,
}

#[derive(serde::Deserialize)]
struct Source {
    file: String,
}

impl About {
    /// About this build, with the installed files found next to `program`.
    pub fn of(program: &Path) -> About {
        let manifest: Manifest =
            toml::from_str(FFMPEG_MANIFEST).expect("ffmpeg.toml is checked by its tests");
        let dir = program.parent();
        let installed = |path: PathBuf| path.is_file().then_some(path);
        About {
            version: env!("CARGO_PKG_VERSION"),
            ffmpeg_source: format!(
                "{}/{}",
                manifest.mirror.trim_end_matches('/'),
                manifest.source.file
            ),
            ffmpeg_version: manifest.version,
            notices: dir.and_then(|dir| installed(dir.join(NOTICES_FILE))),
            ffmpeg_license: dir.and_then(|dir| {
                installed(
                    dir.join(bardo_media::ffmpeg::INSTALLED_DIR)
                        .join(FFMPEG_LICENSE_FILE),
                )
            }),
        }
    }
}

impl Bardo {
    /// About this copy of Bardo.
    pub fn about(&self) -> About {
        About::of(&self.agent_program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_reads_the_ffmpeg_pin() {
        let about = About::of(Path::new("/nowhere/bardo.exe"));
        assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
        assert!(about.ffmpeg_version.starts_with("8."), "{about:?}");
        assert!(
            about.ffmpeg_source.starts_with(&format!(
                "https://github.com/fernandolisboa/bardo/releases/download/ffmpeg-{}/",
                about.ffmpeg_version
            )),
            "{about:?}"
        );
        assert!(about.ffmpeg_source.contains("source"));
        assert_eq!(about.notices, None, "a development build has none");
        assert_eq!(about.ffmpeg_license, None);
    }

    #[test]
    fn about_finds_the_installed_files() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("bardo.exe");
        std::fs::write(dir.path().join(NOTICES_FILE), "notices").unwrap();
        let ffmpeg = dir.path().join(bardo_media::ffmpeg::INSTALLED_DIR);
        std::fs::create_dir_all(&ffmpeg).unwrap();
        std::fs::write(ffmpeg.join(FFMPEG_LICENSE_FILE), "LGPL").unwrap();
        let about = About::of(&program);
        assert_eq!(about.notices, Some(dir.path().join(NOTICES_FILE)));
        assert_eq!(about.ffmpeg_license, Some(ffmpeg.join(FFMPEG_LICENSE_FILE)));
    }
}
