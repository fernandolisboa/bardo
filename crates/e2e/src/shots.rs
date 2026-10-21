//! `check-shots <folder>`: what the look job can tell from the screenshots
//! of a showcase walk (`bardo --showcase`, issue #166) without a person.
//! Each `<shot>.png` is read beside the `<shot>.json` Bardo wrote for it.
//!
//! - Drawn: the screenshot is not one flat colour (a blank or black
//!   window).
//! - Sized: it is as large as the shot asked the window to be.
//! - In its theme: its most common colour is one of the theme's grounds,
//!   so the theme was applied to the whole window.
//! - Its own place: no two places of one layout, theme, language and size
//!   look the same, so each navigation went somewhere.
//! - In its language: Portuguese and English shots of a place differ.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, bail};
use serde_json::{Value, json};

/// A colour within this distance on every channel counts as the same: a
/// software rasterizer may round a blended edge differently.
const TOLERANCE: i32 = 3;
/// Fewer distinct colours than this on a sample of the pixels is a blank
/// window.
const FEWEST_COLOURS: usize = 8;

struct Shot {
    name: String,
    layout: String,
    theme: String,
    language: String,
    place: String,
    width: u32,
    height: u32,
    grounds: Vec<[u8; 3]>,
}

impl Shot {
    fn read(json: &Path) -> anyhow::Result<Shot> {
        let value: Value = serde_json::from_slice(&std::fs::read(json)?)?;
        let text = |key: &str| -> anyhow::Result<String> {
            Ok(value[key]
                .as_str()
                .with_context(|| format!("{} has no {key}", json.display()))?
                .to_owned())
        };
        let number = |key: &str| -> anyhow::Result<u32> {
            Ok(u32::try_from(value[key].as_u64().unwrap_or_default())?)
        };
        let grounds = value["grounds"]
            .as_array()
            .context("no grounds")?
            .iter()
            .filter_map(Value::as_str)
            .filter_map(parse_hex)
            .collect();
        Ok(Shot {
            name: text("name")?,
            layout: text("layout")?,
            theme: text("theme")?,
            language: text("language")?,
            place: text("place")?,
            width: number("width")?,
            height: number("height")?,
            grounds,
        })
    }

    /// The shots that should look alike but for their place.
    fn group(&self) -> (String, String, String, u32, u32) {
        (
            self.layout.clone(),
            self.theme.clone(),
            self.language.clone(),
            self.width,
            self.height,
        )
    }
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let value = u32::from_str_radix(text.strip_prefix('#')?, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (i32::from(*x) - i32::from(y)).abs() <= TOLERANCE)
}

/// What one screenshot shows.
struct Seen {
    width: u32,
    height: u32,
    colours: usize,
    top: [u8; 3],
    top_share: f64,
    fingerprint: u64,
}

fn look_at(png: &Path) -> anyhow::Result<Seen> {
    let image = image::open(png)
        .with_context(|| format!("reading {}", png.display()))?
        .to_rgb8();
    let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
    let mut fingerprint: u64 = 0xcbf2_9ce4_8422_2325;
    for pixel in image.pixels() {
        *counts.entry(pixel.0).or_default() += 1;
        for byte in pixel.0 {
            fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    let total = (image.width() * image.height()).max(1) as f64;
    let (top, top_count) = counts
        .iter()
        .max_by_key(|(_, count)| **count)
        .map(|(colour, count)| (*colour, *count))
        .unwrap_or_default();
    Ok(Seen {
        width: image.width(),
        height: image.height(),
        colours: counts.len(),
        top,
        top_share: top_count as f64 / total,
        fingerprint,
    })
}

pub fn check_shots(folder: &Path) -> anyhow::Result<Value> {
    let mut shots = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        let is_shot = path.extension().is_some_and(|ext| ext == "json")
            && path.with_extension("png").exists();
        if is_shot {
            let shot = Shot::read(&path)?;
            let seen = look_at(&path.with_extension("png"))?;
            shots.push((shot, seen));
        }
    }
    if shots.is_empty() {
        bail!("no screenshots in {}", folder.display());
    }
    shots.sort_by(|a, b| a.0.name.cmp(&b.0.name));

    let mut blank = Vec::new();
    let mut wrong_size = Vec::new();
    let mut off_theme = Vec::new();
    for (shot, seen) in &shots {
        if seen.colours < FEWEST_COLOURS {
            blank.push(json!({ "shot": shot.name, "colours": seen.colours }));
        }
        if (seen.width, seen.height) != (shot.width, shot.height) {
            wrong_size.push(json!({
                "shot": shot.name,
                "size": format!("{}x{}", seen.width, seen.height),
            }));
        }
        if !shot.grounds.iter().any(|ground| near(*ground, seen.top)) {
            off_theme.push(json!({
                "shot": shot.name,
                "most_common": hex(seen.top),
                "share": (seen.top_share * 100.).round(),
                "grounds": shot.grounds.iter().copied().map(hex).collect::<Vec<_>>(),
            }));
        }
    }

    let mut alike = Vec::new();
    let mut by_look: HashMap<_, Vec<&str>> = HashMap::new();
    for (shot, seen) in &shots {
        by_look
            .entry((shot.group(), seen.fingerprint))
            .or_default()
            .push(&shot.name);
    }
    for names in by_look.values().filter(|names| names.len() > 1) {
        alike.push(json!(names));
    }

    let mut same_in_both_languages = Vec::new();
    let mut by_place: HashMap<_, Vec<(&str, u64)>> = HashMap::new();
    for (shot, seen) in &shots {
        by_place
            .entry((
                shot.layout.clone(),
                shot.theme.clone(),
                shot.width,
                shot.height,
                shot.place.clone(),
            ))
            .or_default()
            .push((&shot.language, seen.fingerprint));
    }
    let mut compared = 0;
    for (key, languages) in &by_place {
        if languages.len() > 1 {
            compared += 1;
            if languages.iter().all(|(_, look)| *look == languages[0].1) {
                same_in_both_languages.push(json!(format!(
                    "{}-{}-{}x{}-{}",
                    key.0, key.1, key.2, key.3, key.4
                )));
            }
        }
    }

    Ok(json!({
        "shots": shots.len(),
        "groups": shots.iter().map(|(shot, _)| shot.group()).collect::<std::collections::HashSet<_>>().len(),
        "places_in_both_languages": compared,
        "blank": blank,
        "wrong_size": wrong_size,
        "off_theme": off_theme,
        "alike": alike,
        "same_in_both_languages": same_in_both_languages,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_back_and_compare_with_tolerance() {
        assert_eq!(parse_hex("#F4F1EA"), Some([0xF4, 0xF1, 0xEA]));
        assert_eq!(hex([0xF4, 0xF1, 0xEA]), "#F4F1EA");
        assert!(near([10, 10, 10], [13, 7, 10]));
        assert!(!near([10, 10, 10], [14, 10, 10]));
    }
}
