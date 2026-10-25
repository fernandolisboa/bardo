//! liveportrait-probe: runs LivePortrait's networks (issue #242) from Rust
//! through ONNX Runtime and measures them.
//!
//!   liveportrait-probe info --models DIR [--ep cpu|directml] [--placement]
//!   liveportrait-probe animate --models DIR --vision DIR --take VIDEO
//!       (--look IMAGE | --self) --out OUT.mp4 [--side-by-side OUT.mp4]
//!       [--report OUT.json] [--frames N] [--max-dim 1280]
//!       [--warp warping_module] [--generator spade_generator]
//!
//! `--models` holds tools/convert.py's output; `--vision` Bardo's own
//! vision models (`cargo xtask fetch-models`), for MediaPipe's face mesh.
//! `--self` animates the take's first frame by the take, and compares each
//! result with the take's own frame. Common options: --ep cpu|directml
//! (default cpu), --threads N (default: ONNX Runtime's choice).

mod animate;
mod crop;
#[cfg(test)]
mod fixtures;
mod image;
mod motion;
mod runtime;
mod video;

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::runtime::Ep;

struct Args {
    command: String,
    opts: HashMap<String, String>,
    flags: Vec<String>,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut it = std::env::args().skip(1);
        let command = it
            .next()
            .context("usage: liveportrait-probe <info|animate> [options]")?;
        let (mut opts, mut flags) = (HashMap::new(), Vec::new());
        let rest: Vec<String> = it.collect();
        let mut i = 0;
        while i < rest.len() {
            let key = rest[i]
                .strip_prefix("--")
                .with_context(|| format!("unexpected {}", rest[i]))?;
            match rest.get(i + 1) {
                Some(v) if !v.starts_with("--") => {
                    opts.insert(key.to_owned(), v.clone());
                    i += 2;
                }
                _ => {
                    flags.push(key.to_owned());
                    i += 1;
                }
            }
        }
        Ok(Self {
            command,
            opts,
            flags,
        })
    }

    fn get(&self, k: &str) -> Option<&str> {
        self.opts.get(k).map(String::as_str)
    }

    fn need(&self, k: &str) -> Result<&str> {
        self.get(k).with_context(|| format!("--{k} is required"))
    }

    fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f == k)
    }

    fn number(&self, k: &str) -> Result<Option<usize>> {
        self.get(k)
            .map(|v| v.parse().with_context(|| format!("--{k} {v}")))
            .transpose()
    }
}

fn main() -> Result<()> {
    let args = Args::parse()?;
    let ep = Ep::parse(args.get("ep").unwrap_or("cpu"))?;
    let threads = args.number("threads")?.unwrap_or(0);
    match args.command.as_str() {
        "info" => {
            let all = animate::info(Path::new(args.need("models")?), ep, args.flag("placement"))?;
            if let Some(out) = args.get("out") {
                std::fs::write(out, serde_json::to_string_pretty(&all)?)?;
            }
        }
        "animate" => {
            let look = match (args.get("look"), args.flag("self")) {
                (Some(look), false) => Some(Path::new(look)),
                (None, true) => None,
                _ => bail!("give --look IMAGE or --self"),
            };
            let inputs = animate::Inputs {
                models: Path::new(args.need("models")?),
                vision: Path::new(args.need("vision")?),
                look,
                take: Path::new(args.need("take")?),
                out: Path::new(args.need("out")?),
                side_by_side: args.get("side-by-side").map(Path::new),
                frames: args.number("frames")?,
                max_dim: args.number("max-dim")?.unwrap_or(1280),
            };
            let choice = animate::Choice {
                ep,
                threads,
                warping: args.get("warp").unwrap_or("warping_module"),
                generator: args.get("generator").unwrap_or("spade_generator"),
            };
            let report = animate::animate(&inputs, &choice)?;
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
            if let Some(out) = args.get("report") {
                std::fs::write(out, json)?;
            }
        }
        other => bail!("unknown command {other} (info or animate)"),
    }
    Ok(())
}
