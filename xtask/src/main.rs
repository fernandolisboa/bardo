//! Repository checks that plain cargo cannot express.
//!
//! `cargo xtask lint-deps`: fails if any workspace crate other than `bardo-ui`
//! depends on GPUI, directly or transitively, in any dependency kind (ADR-0001).

mod deps;

use std::process::ExitCode;

use anyhow::Context as _;
use cargo_metadata::{CargoOpt, MetadataCommand};

fn main() -> anyhow::Result<ExitCode> {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("lint-deps") => lint_deps(),
        _ => {
            eprintln!("usage: cargo xtask lint-deps");
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
