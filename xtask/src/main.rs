//! Repository checks that plain cargo cannot express.
//!
//! `cargo xtask lint-deps`: fails if any workspace crate other than `bardo-ui`
//! depends on GPUI, directly or transitively, in any dependency kind (ADR-0001).
//!
//! `cargo xtask fetch-ffmpeg`: downloads the pinned ffmpeg build into
//! `.ffmpeg/bin` (ADR-0007).

mod deps;
mod ffmpeg;

use std::process::ExitCode;

use anyhow::Context as _;
use cargo_metadata::{CargoOpt, MetadataCommand};

fn main() -> anyhow::Result<ExitCode> {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("lint-deps") => lint_deps(),
        Some("fetch-ffmpeg") => {
            let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .context("xtask lives inside the workspace")?;
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
            eprintln!("usage: cargo xtask <lint-deps | fetch-ffmpeg>");
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
