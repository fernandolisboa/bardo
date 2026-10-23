//! Repository checks that plain cargo cannot express.
//!
//! `cargo xtask lint-deps`: fails if any workspace crate other than `bardo-ui`
//! depends on GPUI, directly or transitively, in any dependency kind (ADR-0001).
//!
//! `cargo xtask fetch-ffmpeg`: downloads the pinned ffmpeg build into
//! `.ffmpeg/bin` (ADR-0007).
//!
//! `cargo xtask fetch-models`: downloads the pinned vision models into
//! `.models` (ADR-0024).
//!
//! `cargo xtask package`: stages Bardo as installed and builds the Windows
//! installer (issue #135).
//!
//! `cargo xtask mirror-ffmpeg <dir>`: the pinned ffmpeg archives and their
//! source, for the release that mirrors them.

mod deps;
mod ffmpeg;
mod models;
mod package;

use std::process::ExitCode;

use anyhow::Context as _;
use cargo_metadata::{CargoOpt, MetadataCommand};

const USAGE: &str = "usage: cargo xtask <lint-deps | fetch-ffmpeg [--archive <path>] | \
                     fetch-models [--dir <path>] | \
                     package [--no-build] [--no-installer] [--tag <vX.Y.Z>] | \
                     mirror-ffmpeg <dir>>";

fn main() -> anyhow::Result<ExitCode> {
    let task = std::env::args().nth(1);
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask lives inside the workspace")?;
    match task.as_deref() {
        Some("lint-deps") => lint_deps(),
        Some("package") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            package::package(workspace, &package::Options::parse(&args)?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some("mirror-ffmpeg") => {
            let Some(dir) = std::env::args().nth(2) else {
                eprintln!("{USAGE}");
                return Ok(ExitCode::FAILURE);
            };
            ffmpeg::mirror(workspace, std::path::Path::new(&dir))?;
            Ok(ExitCode::SUCCESS)
        }
        Some("fetch-models") => {
            let dir = match (std::env::args().nth(2).as_deref(), std::env::args().nth(3)) {
                (Some("--dir"), Some(path)) => Some(path.into()),
                (None, _) => None,
                _ => {
                    eprintln!("usage: cargo xtask fetch-models [--dir <path>]");
                    return Ok(ExitCode::FAILURE);
                }
            };
            models::fetch(workspace, dir)?;
            Ok(ExitCode::SUCCESS)
        }
        Some("fetch-ffmpeg") => {
            let archive = match (std::env::args().nth(2).as_deref(), std::env::args().nth(3)) {
                (Some("--archive"), Some(path)) => Some(path.into()),
                (None, _) => None,
                _ => {
                    eprintln!("usage: cargo xtask fetch-ffmpeg [--archive <path>]");
                    return Ok(ExitCode::FAILURE);
                }
            };
            ffmpeg::fetch(workspace, archive)?;
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            eprintln!("{USAGE}");
            Ok(ExitCode::FAILURE)
        }
    }
}

fn lint_deps() -> anyhow::Result<ExitCode> {
    let metadata = MetadataCommand::new()
        .features(CargoOpt::AllFeatures)
        .exec()
        .context("running cargo metadata")?;
    let graph = deps::Graph::from_metadata(&metadata)?;
    let violations = graph.gpui_violations(deps::UI_CRATE);

    if violations.is_empty() {
        println!("lint-deps: only {} depends on GPUI", deps::UI_CRATE);
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "lint-deps: only {} may depend on GPUI (ADR-0001)",
        deps::UI_CRATE
    );
    for violation in &violations {
        eprintln!("  {violation}");
    }
    Ok(ExitCode::FAILURE)
}
