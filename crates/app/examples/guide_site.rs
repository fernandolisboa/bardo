//! Writes the sources of the user guide website (issue #112) from
//! `docs/guide/`: one mdBook book per language, then mdBook builds them.
//!
//! ```sh
//! cargo run -p bardo-app --example guide_site -- target/guide-site
//! mdbook build target/guide-site/en-US
//! mdbook build target/guide-site/pt-BR
//! # the site is in target/guide-site/site/
//! ```
//!
//! Options: `--base-path /bardo` when the site is served below the
//! domain's root (GitHub Pages serves a repository's site at
//! `/<repository>`), and `--repository <url>` for each page's edit link.
//! Exits with an error, listing every problem, on a broken link or a page
//! missing in a language.

use std::path::PathBuf;
use std::process::ExitCode;

use bardo_app::{SiteOptions, guide_site};

fn main() -> ExitCode {
    let mut out = None;
    let mut options = SiteOptions {
        base_path: String::new(),
        repository: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--base-path" => options.base_path = args.next().unwrap_or_default(),
            "--repository" => options.repository = args.next().filter(|url| !url.is_empty()),
            _ if out.is_none() && !arg.starts_with("--") => out = Some(PathBuf::from(arg)),
            _ => {
                eprintln!("unexpected argument {arg:?}");
                return usage();
            }
        }
    }
    let Some(out) = out else {
        return usage();
    };

    let files = match guide_site(&options) {
        Ok(files) => files,
        Err(problems) => {
            eprintln!("the guide site cannot be built:");
            for problem in problems {
                eprintln!("  {problem}");
            }
            return ExitCode::FAILURE;
        }
    };
    // Clear the pages of a previous run, so a page removed from the guide
    // leaves the site too. Only the books' page folders: nothing else in
    // the output folder is touched.
    let mut page_folders: Vec<PathBuf> = files
        .iter()
        .filter_map(|file| out.join(&file.path).parent().map(PathBuf::from))
        .filter(|folder| folder.ends_with("src"))
        .collect();
    page_folders.sort();
    page_folders.dedup();
    for folder in page_folders.iter().filter(|folder| folder.exists()) {
        if let Err(error) = std::fs::remove_dir_all(folder) {
            eprintln!("cannot clear {}: {error}", folder.display());
            return ExitCode::FAILURE;
        }
    }
    for file in &files {
        let path = out.join(&file.path);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, &file.contents));
        if let Err(error) = written {
            eprintln!("cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }
    println!("wrote {} files to {}", files.len(), out.display());
    ExitCode::SUCCESS
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: cargo run -p bardo-app --example guide_site -- <out-dir> \
         [--base-path /bardo] [--repository https://github.com/owner/repo]"
    );
    ExitCode::FAILURE
}
