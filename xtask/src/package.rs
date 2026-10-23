//! `cargo xtask package`: Bardo for Windows as users get it (issue #135).
//!
//! 1. Builds `bardo.exe` in release (skip with `--no-build`).
//! 2. Fetches the pinned ffmpeg (ADR-0007).
//! 3. Fetches the pinned vision models (ADR-0024).
//! 4. Stages the installed layout in `target/package/Bardo/`:
//!    `bardo.exe` with the ONNX Runtime library it loads beside it
//!    (`DirectML.dll` on Windows), `ffmpeg\` (ffmpeg, ffprobe, their DLLs
//!    unmodified, `LICENSE.txt` and `SOURCE.txt`), `models\` (the vision
//!    models, each checked against its pin, and `NOTICES.txt` with their
//!    sources and licenses), and `THIRD-PARTY-NOTICES.html` made by
//!    `cargo about` for the crates `bardo.exe` is built from.
//! 5. Where Inno Setup 6 is installed, compiles `installer/bardo.iss` into
//!    `target/package/Bardo-Setup-<version>-x64.exe`, with its SHA-256 next
//!    to it (skip with `--no-installer`).
//!
//! `--tag <vX.Y.Z>` fails unless the tag matches the workspace version, so
//! a release never ships a mislabeled build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};

use crate::ffmpeg::Manifest;

/// What `cargo xtask package` does.
#[derive(Debug, Default)]
pub struct Options {
    pub no_build: bool,
    pub no_installer: bool,
    pub tag: Option<String>,
}

impl Options {
    pub fn parse(args: &[String]) -> anyhow::Result<Options> {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--no-build" => options.no_build = true,
                "--no-installer" => options.no_installer = true,
                "--tag" => {
                    options.tag = Some(args.next().context("--tag needs a value")?.clone());
                }
                other => bail!("unknown option {other}"),
            }
        }
        Ok(options)
    }
}

/// The folder the installer copies, and its file names.
pub const STAGE_DIR: &str = "Bardo";
pub const FFMPEG_DIR: &str = "ffmpeg";
pub const NOTICES: &str = "THIRD-PARTY-NOTICES.html";
pub const FFMPEG_SOURCE: &str = "SOURCE.txt";
pub const MODELS_DIR: &str = "models";
pub const MODELS_NOTICES: &str = "NOTICES.txt";
/// The libraries ONNX Runtime loads at run time, copied beside `bardo.exe`
/// by `ort`'s build: DirectML on Windows (linked statically otherwise).
pub const RUNTIME_LIBRARIES: [&str; 1] = ["DirectML.dll"];

pub fn package(workspace: &Path, options: &Options) -> anyhow::Result<()> {
    let version = workspace_version(workspace)?;
    if let Some(tag) = &options.tag {
        check_tag(tag, &version)?;
    }
    let target = target_dir(workspace)?;
    if !options.no_build {
        run(Command::new(cargo()).current_dir(workspace).args([
            "build",
            "-p",
            "bardo-ui",
            "--release",
            "--locked",
        ]))?;
    }
    crate::ffmpeg::fetch(workspace, None)?;
    let manifest = Manifest::read(workspace)?;
    let models_dir = workspace.join(".models");
    crate::models::fetch(workspace, Some(models_dir.clone()))?;
    let models = crate::models::Manifest::read(workspace)?;

    let out = target.join("package");
    let stage_dir = out.join(STAGE_DIR);
    let release = target.join("release");
    let program = release.join(executable("bardo"));
    let libraries: Vec<PathBuf> = if cfg!(windows) {
        RUNTIME_LIBRARIES
            .iter()
            .map(|name| release.join(name))
            .collect()
    } else {
        Vec::new()
    };
    stage(
        &Sources {
            program: &program,
            libraries: &libraries,
            ffmpeg_bin: &workspace.join(".ffmpeg").join("bin"),
            ffmpeg_source: &source_offer(&manifest),
            models_dir: &models_dir,
            models: &models.model,
            models_notices: &models.notices(),
        },
        &stage_dir,
    )?;
    notices(workspace, &stage_dir.join(NOTICES))?;
    println!("package: Bardo {version} staged in {}", stage_dir.display());

    if options.no_installer {
        return Ok(());
    }
    let Some(iscc) = find_iscc() else {
        if cfg!(windows) {
            bail!(
                "Inno Setup 6 was not found: install it from https://jrsoftware.org/isdl.php, \
                 set ISCC to its ISCC.exe, or pass --no-installer"
            );
        }
        println!("package: no installer outside Windows");
        return Ok(());
    };
    run(Command::new(&iscc)
        .arg(format!("/DAppVersion={version}"))
        .arg(format!("/DStageDir={}", stage_dir.display()))
        .arg(format!("/DOutputDir={}", out.display()))
        .arg(format!("/DRepoRoot={}", workspace.display()))
        .arg(workspace.join("installer").join("bardo.iss")))?;
    let installer = out.join(installer_name(&version));
    let digest = crate::ffmpeg::sha256_file(&installer)?;
    let checksum = out.join(format!("{}.sha256", installer_name(&version)));
    fs::write(
        &checksum,
        format!("{digest}  {}\n", installer_name(&version)),
    )?;
    println!("package: {}", installer.display());
    Ok(())
}

/// What goes into the staged folder.
pub struct Sources<'a> {
    /// The release `bardo` executable.
    pub program: &'a Path,
    /// `.ffmpeg/bin` as `fetch-ffmpeg` leaves it.
    pub ffmpeg_bin: &'a Path,
    /// The text of `ffmpeg\SOURCE.txt`.
    pub ffmpeg_source: &'a str,
    /// Libraries that go beside the program, each required.
    pub libraries: &'a [PathBuf],
    /// `.models` as `fetch-models` leaves it, and the files it must hold.
    pub models_dir: &'a Path,
    pub models: &'a [crate::models::Model],
    /// The text of `models\NOTICES.txt`.
    pub models_notices: &'a str,
}

/// Fills `dir` (emptied first) with the installed layout.
pub fn stage(sources: &Sources, dir: &Path) -> anyhow::Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("emptying {}", dir.display()))?;
    }
    let ffmpeg = dir.join(FFMPEG_DIR);
    fs::create_dir_all(&ffmpeg)?;
    let program_name = sources
        .program
        .file_name()
        .context("the program has a file name")?;
    fs::copy(sources.program, dir.join(program_name))
        .with_context(|| format!("copying {}", sources.program.display()))?;
    let mut found = (false, false, false);
    for entry in fs::read_dir(sources.ffmpeg_bin)
        .with_context(|| format!("reading {}", sources.ffmpeg_bin.display()))?
    {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !path.is_file() || crate::ffmpeg::UNUSED.contains(&name) {
            continue;
        }
        let stem = name.trim_end_matches(".exe");
        found.0 |= stem == "ffmpeg";
        found.1 |= stem == "ffprobe";
        found.2 |= name == "LICENSE.txt";
        fs::copy(&path, ffmpeg.join(name)).with_context(|| format!("copying {name}"))?;
    }
    match found {
        (true, true, true) => {}
        _ => bail!(
            "{} must hold ffmpeg, ffprobe and LICENSE.txt: run cargo xtask fetch-ffmpeg",
            sources.ffmpeg_bin.display()
        ),
    }
    fs::write(ffmpeg.join(FFMPEG_SOURCE), sources.ffmpeg_source)?;
    for library in sources.libraries {
        let name = library.file_name().context("a library has a file name")?;
        fs::copy(library, dir.join(name)).with_context(|| {
            format!(
                "copying {}: build bardo-ui in release on Windows first",
                library.display()
            )
        })?;
    }
    let models = dir.join(MODELS_DIR);
    fs::create_dir_all(&models)?;
    for model in sources.models {
        let path = sources.models_dir.join(&model.file);
        if !crate::models::matches(&path, model) {
            bail!(
                "{} is missing or not the pinned file: run cargo xtask fetch-models",
                path.display()
            );
        }
        fs::copy(&path, models.join(&model.file))
            .with_context(|| format!("copying {}", model.file))?;
    }
    fs::write(models.join(MODELS_NOTICES), sources.models_notices)?;
    Ok(())
}

/// `ffmpeg\SOURCE.txt`: what the bundled FFmpeg is, its license, and where
/// its source is (the LGPL's source offer).
pub fn source_offer(manifest: &Manifest) -> String {
    let windows = &manifest.windows;
    let source = &manifest.source;
    format!(
        "FFmpeg {version}\r\n\
         \r\n\
         Bardo runs FFmpeg (https://ffmpeg.org) as the separate programs \
         ffmpeg.exe and ffprobe.exe in this folder. FFmpeg is licensed under \
         the GNU Lesser General Public License version 3 (LICENSE.txt). Its \
         programs and libraries are shipped unmodified, from this build:\r\n\
         \r\n\
         {build_url}\r\n\
         SHA-256 {build_sha}\r\n\
         \r\n\
         Source code of this build:\r\n\
         {source_mirror}\r\n\
         (also at {source_url})\r\n\
         \r\n\
         Build scripts and configure options:\r\n\
         {scripts}\r\n\
         \r\n\
         Bardo's copy of the build:\r\n\
         {build_mirror}\r\n",
        version = manifest.version,
        build_url = windows.url,
        build_sha = windows.sha256,
        source_mirror = manifest.mirrored(&source.file),
        source_url = source.url,
        scripts = source.build,
        build_mirror = manifest.mirrored(windows.file_name()),
    )
}

/// `cargo about` for the crates in `bardo.exe` on Windows, as one HTML page.
fn notices(workspace: &Path, out: &Path) -> anyhow::Result<()> {
    let status = Command::new(cargo())
        .current_dir(workspace)
        .args(["about", "generate", "--locked", "--fail"])
        .args(["--manifest-path", "crates/ui/Cargo.toml"])
        .args(["--config", "installer/about.toml"])
        .arg("--output-file")
        .arg(out)
        .arg("installer/about.hbs")
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => bail!("cargo about failed: {status}"),
        Err(error) => bail!(
            "could not run cargo about ({error}): install it with \
             `cargo install cargo-about --locked --features cli`"
        ),
    }
}

/// `Bardo-Setup-0.1.0-x64.exe`.
pub fn installer_name(version: &str) -> String {
    format!("Bardo-Setup-{version}-x64.exe")
}

fn check_tag(tag: &str, version: &str) -> anyhow::Result<()> {
    if tag.strip_prefix('v') != Some(version) {
        bail!("the tag {tag} does not match the workspace version {version}: expected v{version}");
    }
    Ok(())
}

/// `[workspace.package] version` of the root `Cargo.toml`.
pub fn workspace_version(workspace: &Path) -> anyhow::Result<String> {
    let path = workspace.join("Cargo.toml");
    let manifest: toml::Table = toml::from_str(&fs::read_to_string(&path)?)
        .with_context(|| format!("parsing {}", path.display()))?;
    manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_owned)
        .context("Cargo.toml has no [workspace.package] version")
}

fn target_dir(workspace: &Path) -> anyhow::Result<PathBuf> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(workspace)
        .no_deps()
        .exec()
        .context("running cargo metadata")?;
    Ok(metadata.target_directory.into_std_path_buf())
}

/// Inno Setup's compiler: `ISCC`, then its default folders, then PATH.
fn find_iscc() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ISCC") {
        return Some(PathBuf::from(path));
    }
    let mut candidates = Vec::new();
    for variable in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(dir) = std::env::var_os(variable) {
            let dir = PathBuf::from(dir);
            candidates.push(dir.join("Inno Setup 6").join("ISCC.exe"));
            candidates.push(dir.join("Programs").join("Inno Setup 6").join("ISCC.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(executable("iscc"))));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn executable(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

fn run(command: &mut Command) -> anyhow::Result<()> {
    let status = command
        .status()
        .with_context(|| format!("running {:?}", command.get_program()))?;
    if !status.success() {
        bail!("{:?} failed: {status}", command.get_program());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }

    fn fake_ffmpeg(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        for name in [
            "ffmpeg.exe",
            "ffprobe.exe",
            "ffplay.exe",
            "avcodec-62.dll",
            "LICENSE.txt",
        ] {
            fs::write(dir.join(name), name).unwrap();
        }
    }

    /// A models folder with one pinned file, `abc`.
    fn fake_models(dir: &Path) -> (PathBuf, Vec<crate::models::Model>) {
        let models = dir.join("models-src");
        fs::create_dir_all(&models).unwrap();
        fs::write(models.join("face.onnx"), "abc").unwrap();
        let pin = crate::models::Model {
            file: "face.onnx".into(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            size: 3,
        };
        (models, vec![pin])
    }

    #[test]
    fn a_stage_without_a_pinned_model_fails() {
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("bardo.exe");
        fs::write(&program, "bardo").unwrap();
        let bin = temp.path().join("bin");
        fake_ffmpeg(&bin);
        let (models_dir, models) = fake_models(temp.path());
        fs::write(models_dir.join("face.onnx"), "abd").unwrap();
        let error = stage(
            &Sources {
                program: &program,
                libraries: &[],
                ffmpeg_bin: &bin,
                ffmpeg_source: "offer",
                models_dir: &models_dir,
                models: &models,
                models_notices: "notices",
            },
            &temp.path().join("stage"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("fetch-models"), "{error}");
    }

    #[test]
    fn the_stage_holds_the_installed_layout() {
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("bardo.exe");
        fs::write(&program, "bardo").unwrap();
        let bin = temp.path().join("bin");
        fake_ffmpeg(&bin);
        let dir = temp.path().join("stage");
        fs::create_dir_all(dir.join("old")).unwrap();

        let library = temp.path().join("DirectML.dll");
        fs::write(&library, "directml").unwrap();
        let (models_dir, models) = fake_models(temp.path());
        stage(
            &Sources {
                program: &program,
                libraries: std::slice::from_ref(&library),
                ffmpeg_bin: &bin,
                ffmpeg_source: "offer",
                models_dir: &models_dir,
                models: &models,
                models_notices: "notices",
            },
            &dir,
        )
        .unwrap();

        let mut names: Vec<_> = walk(&dir);
        names.sort();
        assert_eq!(
            names,
            [
                "DirectML.dll",
                "bardo.exe",
                "ffmpeg/LICENSE.txt",
                "ffmpeg/SOURCE.txt",
                "ffmpeg/avcodec-62.dll",
                "ffmpeg/ffmpeg.exe",
                "ffmpeg/ffprobe.exe",
                "models/NOTICES.txt",
                "models/face.onnx",
            ],
            "no ffplay, nothing left from before"
        );
        assert_eq!(
            fs::read_to_string(dir.join("ffmpeg/avcodec-62.dll")).unwrap(),
            "avcodec-62.dll",
            "copied unmodified"
        );
    }

    #[test]
    fn a_stage_without_the_ffmpeg_license_fails() {
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("bardo.exe");
        fs::write(&program, "bardo").unwrap();
        let bin = temp.path().join("bin");
        fake_ffmpeg(&bin);
        fs::remove_file(bin.join("LICENSE.txt")).unwrap();
        let (models_dir, models) = fake_models(temp.path());
        let error = stage(
            &Sources {
                program: &program,
                libraries: &[],
                ffmpeg_bin: &bin,
                ffmpeg_source: "offer",
                models_dir: &models_dir,
                models: &models,
                models_notices: "notices",
            },
            &temp.path().join("stage"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("LICENSE.txt"), "{error}");
    }

    #[test]
    fn the_source_offer_points_at_the_mirror() {
        let manifest = Manifest::read(workspace()).unwrap();
        let offer = source_offer(&manifest);
        assert!(offer.contains("Lesser General Public License version 3"));
        assert!(offer.contains(&manifest.mirrored(&manifest.source.file)));
        assert!(offer.contains(&manifest.windows.sha256));
        assert!(offer.contains(&manifest.source.build));
    }

    #[test]
    fn a_release_tag_must_match_the_version() {
        let version = workspace_version(workspace()).unwrap();
        assert!(check_tag(&format!("v{version}"), &version).is_ok());
        assert!(check_tag(&version, &version).is_err());
        assert!(check_tag("v99.0.0", &version).is_err());
        assert_eq!(installer_name("1.2.3"), "Bardo-Setup-1.2.3-x64.exe");
    }

    #[test]
    fn options_read_back() {
        let args: Vec<String> = ["--no-build", "--tag", "v1.0.0"]
            .into_iter()
            .map(String::from)
            .collect();
        let options = Options::parse(&args).unwrap();
        assert!(options.no_build);
        assert!(!options.no_installer);
        assert_eq!(options.tag.as_deref(), Some("v1.0.0"));
        assert!(Options::parse(&["--tag".to_string()]).is_err());
        assert!(Options::parse(&["--nope".to_string()]).is_err());
    }

    fn walk(dir: &Path) -> Vec<String> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                names.extend(walk(&path).into_iter().map(|n| format!("{name}/{n}")));
            } else {
                names.push(name);
            }
        }
        names
    }
}
