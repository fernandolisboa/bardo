//! Updating Bardo from its published releases (issue #144, ADR-0011).
//!
//! A **release feed** says which release is the latest and hands over its
//! files; the **installer store** keeps the downloaded installer until it
//! runs; the **installer runner** starts it. The rules that decide what a
//! release means for this copy (newer or not, which file to take, how its
//! checksum reads) live here.

use std::cmp::Ordering;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;

/// A release's version, as its tag names it (`v1.2.3`, `1.2.3-rc.1`),
/// ordered by Semantic Versioning: a pre-release comes before its release.
/// Build metadata (`+…`) is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Dot-separated pre-release identifiers, empty for a release.
    pub pre: Vec<PreRelease>,
}

/// One pre-release identifier: numbers compare as numbers and come before
/// words.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PreRelease {
    Number(u64),
    Word(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a version: {0}")]
pub struct InvalidVersion(pub String);

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
        }
    }

    pub fn is_pre_release(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl FromStr for Version {
    type Err = InvalidVersion;

    /// Reads `1.2.3`, `v1.2.3`, `1.2.3-beta.2` and `1.2.3+build`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidVersion(text.to_owned());
        let trimmed = text.trim();
        let bare = trimmed
            .strip_prefix('v')
            .or_else(|| trimmed.strip_prefix('V'))
            .unwrap_or(trimmed);
        let bare = bare.split_once('+').map_or(bare, |(version, _)| version);
        let (core, pre) = match bare.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (bare, None),
        };
        let number = |part: &str| -> Option<u64> {
            let leading_zero = part.len() > 1 && part.starts_with('0');
            (!part.is_empty() && !leading_zero && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        };
        let mut parts = core.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid());
        };
        let (Some(major), Some(minor), Some(patch)) = (number(major), number(minor), number(patch))
        else {
            return Err(invalid());
        };
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => pre
                .split('.')
                .map(|id| {
                    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    {
                        None
                    } else if id.bytes().all(|b| b.is_ascii_digit()) {
                        number(id).map(PreRelease::Number)
                    } else {
                        Some(PreRelease::Word(id.to_owned()))
                    }
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(invalid)?,
        };
        Ok(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                // Identifier by identifier; a longer list wins a tie.
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        for (index, id) in self.pre.iter().enumerate() {
            f.write_str(if index == 0 { "-" } else { "." })?;
            match id {
                PreRelease::Number(n) => write!(f, "{n}")?,
                PreRelease::Word(word) => f.write_str(word)?,
            }
        }
        Ok(())
    }
}

/// A file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    /// Where it downloads from.
    pub url: String,
    /// Its size in bytes, as the feed reports it.
    pub size: u64,
    /// Its SHA-256 in lowercase hex, when the feed reports one.
    pub sha256: Option<String>,
}

/// The latest published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The release's page.
    pub page: String,
    /// What changed, in Markdown.
    pub notes: String,
    pub published_at: Option<SystemTime>,
    /// Every file attached to it.
    pub assets: Vec<ReleaseAsset>,
}

impl Release {
    /// The Windows installer of this version, `Bardo-Setup-<v>-x64.exe`.
    pub fn installer(&self) -> Option<&ReleaseAsset> {
        let name = installer_name(&self.version);
        self.assets.iter().find(|asset| asset.name == name)
    }

    /// The installer's checksum file, `<installer>.sha256`.
    pub fn checksum(&self) -> Option<&ReleaseAsset> {
        let name = format!("{}.sha256", installer_name(&self.version));
        self.assets.iter().find(|asset| asset.name == name)
    }
}

/// The installer's file name for `version`, as `cargo xtask package`
/// writes it.
pub fn installer_name(version: &Version) -> String {
    format!("Bardo-Setup-{version}-x64.exe")
}

/// The version an installer's file name is for, if it is one.
pub fn installer_version(name: &str) -> Option<Version> {
    name.strip_prefix("Bardo-Setup-")?
        .strip_suffix("-x64.exe")?
        .parse()
        .ok()
}

/// The SHA-256 a checksum file gives for `file`: lines of `<hex>  <name>`
/// (`sha256sum`'s format, a `*` before the name for binary mode), or one
/// bare hash. Lowercase hex, or `None` when it names no such file.
pub fn checksum_for(text: &str, file: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let named = lines.iter().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file).then_some(hash)
    });
    let hash = match (named, lines.as_slice()) {
        (Some(hash), _) => hash,
        (None, [only]) if !only.contains(char::is_whitespace) => only,
        _ => return None,
    };
    is_sha256(hash).then(|| hash.to_ascii_lowercase())
}

/// Whether `text` is a SHA-256 in hex.
pub fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Why a check found nothing to say about updates.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateCheckError {
    /// No answer: no network, DNS, a proxy or a timeout.
    #[error("the release feed could not be reached: {0}")]
    Offline(String),
    /// The feed limits how often it is asked; it can be asked again after
    /// `until`, when it says.
    #[error("the release feed is rate limited")]
    RateLimited { until: Option<SystemTime> },
    /// Nothing is published yet.
    #[error("no release is published")]
    NoRelease,
    /// An answer Bardo does not understand.
    #[error("unexpected answer from the release feed: {0}")]
    Unexpected(String),
}

/// Why a download did not finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DownloadError {
    /// No answer, or the connection dropped. Trying again may work.
    #[error("the download was cut short: {0}")]
    Unreachable(String),
    /// The server answered with an error, or the body was not the file.
    #[error("unexpected answer: {0}")]
    Unexpected(String),
    /// The file is bigger than allowed.
    #[error("the file is bigger than {0} bytes")]
    TooBig(u64),
    /// The link is not one of this repository's release files.
    #[error("not a release file: {0}")]
    NotAReleaseFile(String),
    /// Writing it failed.
    #[error("{0}")]
    Write(String),
    /// Told to stop.
    #[error("stopped")]
    Stopped,
}

/// Where Bardo learns about its releases (GitHub Releases).
pub trait ReleaseFeed: Send + Sync {
    /// The latest published release. Blocking: run it off the UI thread.
    fn latest(&self) -> Result<Release, UpdateCheckError>;

    /// A small text file of a release (its checksum), up to `max_bytes`.
    fn text(&self, url: &str, max_bytes: u64) -> Result<String, DownloadError>;

    /// Writes the file at `url` to `to`, at most `max_bytes`, calling
    /// `progress` with the bytes written so far; it stops when `progress`
    /// returns false. Returns the bytes written.
    fn download(
        &self,
        url: &str,
        to: &mut dyn Write,
        max_bytes: u64,
        progress: &mut dyn FnMut(u64) -> bool,
    ) -> Result<u64, DownloadError>;
}

/// Where downloaded installers wait to run: `%LOCALAPPDATA%\Bardo\updates`.
pub trait InstallerStore: Send + Sync {
    /// Starts writing `name` afresh, beside the finished files: a file
    /// that is not whole never has the finished name.
    fn begin(&self, name: &str) -> std::io::Result<Box<dyn Write + Send>>;

    /// The file begun as `name` is whole and checked: it takes its name.
    fn finish(&self, name: &str) -> std::io::Result<()>;

    /// Deletes `name`, finished or not. Nothing there is not an error.
    fn discard(&self, name: &str) -> std::io::Result<()>;

    /// Reads the finished `name`, if it is there.
    fn open(&self, name: &str) -> std::io::Result<Option<Box<dyn Read + Send>>>;

    /// The finished file's path, to run it.
    fn path(&self, name: &str) -> PathBuf;

    /// Whether the finished `name` is there.
    fn exists(&self, name: &str) -> bool;

    /// Deletes every file but the finished ones `keep` names, and any
    /// unfinished one unless `keep_unfinished`.
    fn prune(&self, keep: &dyn Fn(&str) -> bool, keep_unfinished: bool) -> std::io::Result<()>;
}

/// Starts a downloaded installer, which outlives Bardo.
pub trait InstallerRunner: Send + Sync {
    fn run(&self, installer: &Path, arguments: &[&str]) -> std::io::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        text.parse().unwrap()
    }

    #[test]
    fn versions_read_with_or_without_the_tag_prefix() {
        assert_eq!(v("v0.1.0"), Version::new(0, 1, 0));
        assert_eq!(v("12.30.4"), Version::new(12, 30, 4));
        assert_eq!(v(" V1.2.3 "), Version::new(1, 2, 3));
        assert_eq!(v("1.2.3+build.7"), Version::new(1, 2, 3));
        assert_eq!(
            v("1.0.0-rc.1").pre,
            vec![PreRelease::Word("rc".into()), PreRelease::Number(1)]
        );
    }

    #[test]
    fn what_is_not_a_version_is_refused() {
        for text in [
            "",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "01.2.3",
            "1.2.3-",
            "1.2.3-rc..1",
            "1.2.3-01",
            "ffmpeg-8.1",
            "1.2.x",
        ] {
            assert!(text.parse::<Version>().is_err(), "{text}");
        }
    }

    #[test]
    fn versions_print_as_they_read() {
        for text in ["0.1.0", "1.20.300", "1.0.0-beta.2", "2.0.0-rc.1.x-y"] {
            assert_eq!(v(text).to_string(), text);
        }
    }

    #[test]
    fn versions_follow_semantic_versioning_precedence() {
        let ordered = [
            "0.1.0",
            "0.1.1",
            "0.2.0",
            "0.10.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("v1.0.0").cmp(&v("1.0.0+other")), Ordering::Equal);
    }

    #[test]
    fn the_installer_and_its_checksum_are_found_by_name() {
        let asset = |name: &str| ReleaseAsset {
            name: name.into(),
            url: format!("https://example.test/{name}"),
            size: 1,
            sha256: None,
        };
        let release = Release {
            version: v("0.2.0"),
            page: String::new(),
            notes: String::new(),
            published_at: None,
            assets: vec![
                asset("Bardo-Setup-0.1.0-x64.exe"),
                asset("Bardo-Setup-0.2.0-x64.exe.sha256"),
                asset("Bardo-Setup-0.2.0-x64.exe"),
            ],
        };
        assert_eq!(
            release.installer().unwrap().name,
            "Bardo-Setup-0.2.0-x64.exe"
        );
        assert_eq!(
            release.checksum().unwrap().name,
            "Bardo-Setup-0.2.0-x64.exe.sha256"
        );
        assert_eq!(
            installer_version("Bardo-Setup-0.2.0-x64.exe"),
            Some(v("0.2.0"))
        );
        assert_eq!(installer_version("Bardo-Setup-0.2.0-x64.exe.part"), None);
        assert_eq!(installer_version("notes.txt"), None);
    }

    #[test]
    fn a_checksum_file_gives_the_hash_of_the_named_file() {
        let hash = "A".repeat(64);
        let file = "Bardo-Setup-0.2.0-x64.exe";
        let lower = "a".repeat(64);
        assert_eq!(
            checksum_for(&format!("{hash}  {file}\n"), file),
            Some(lower.clone())
        );
        assert_eq!(
            checksum_for(&format!("{hash} *{file}\r\n"), file),
            Some(lower.clone())
        );
        assert_eq!(checksum_for(&format!("{hash}\n"), file), Some(lower));
        let other = format!("{}  other.exe\n", "b".repeat(64));
        assert_eq!(checksum_for(&other, file), None);
        assert_eq!(checksum_for(&format!("abc  {file}"), file), None);
        assert_eq!(checksum_for("", file), None);
        assert_eq!(
            checksum_for(&format!("{other}{}  {file}\n", "c".repeat(64)), file),
            Some("c".repeat(64))
        );
    }
}
