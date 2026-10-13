//! Embeds Bardo's icon in `bardo.exe` on Windows (issue #131). GPUI gives
//! its window class icon resource 1 of the executable, so the window, Alt+Tab
//! and the taskbar show it, and Explorer shows it on the file.

use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let icon = manifest_dir
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate sits in crates/ of the workspace")
        .join("assets")
        .join("brand")
        .join("bardo.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    let script = out_dir.join("bardo.rc");
    let path = icon.display().to_string().replace('\\', "\\\\");
    fs::write(&script, format!("1 ICON \"{path}\"\n")).expect("writing the resource script");
    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_optional()
        .expect("compiling the icon resource");
}
