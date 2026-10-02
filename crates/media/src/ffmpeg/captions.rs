//! Burned-in captions: the plan's caption lines written as an ASS script
//! and drawn by libass (ffmpeg's `subtitles` filter) over the joined video.
//! The fonts ship inside Bardo and libass is pointed at them, so preview
//! and render draw the same letters on any machine, whatever it has
//! installed.
//!
//! Sizes are fractions of the frame, so a 540-line preview and a 1080x1920
//! render lay a caption out alike.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bardo_domain::CaptionStyle;

use super::MediaError;
use super::frames::FrameSize;

/// The caption lines of a plan, in one style.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionTrack {
    pub style: CaptionStyle,
    /// In order; lines may touch but not overlap.
    pub lines: Vec<CaptionLine>,
}

/// One caption as the plan shows it: its text and where on the plan's
/// timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionLine {
    pub text: String,
    pub at: Duration,
    pub duration: Duration,
}

impl CaptionTrack {
    /// The lines from `from` on, as if the timeline started there.
    pub(super) fn starting_at(&self, from: Duration) -> CaptionTrack {
        CaptionTrack {
            style: self.style,
            lines: self
                .lines
                .iter()
                .filter(|line| line.at + line.duration > from)
                .map(|line| {
                    let skip = from.saturating_sub(line.at);
                    CaptionLine {
                        text: line.text.clone(),
                        at: line.at.saturating_sub(from),
                        duration: line.duration - skip,
                    }
                })
                .collect(),
        }
    }
}

/// What a caption style looks like. Colours are `0xRRGGBB`; sizes are
/// fractions of the frame's shorter side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptionLook {
    /// The font family, one of those Bardo ships.
    pub font: &'static str,
    pub bold: bool,
    /// The letters' height.
    pub size: f32,
    pub fill: u32,
    /// A line around the letters: its colour and width.
    pub outline: Option<(u32, f32)>,
    /// A band behind the text: its colour, opacity (0-1) and padding.
    pub band: Option<(u32, f32, f32)>,
    /// A soft shadow's offset.
    pub shadow: f32,
    pub uppercase: bool,
    /// How far up the frame the text's bottom sits, as a fraction of its
    /// height.
    pub lift: f32,
}

/// How each caption style looks.
pub fn caption_look(style: CaptionStyle) -> CaptionLook {
    match style {
        CaptionStyle::Clean => CaptionLook {
            font: "Poppins",
            bold: true,
            size: 0.058,
            fill: 0xFFFFFF,
            outline: Some((0x000000, 0.004)),
            band: None,
            shadow: 0.002,
            uppercase: false,
            lift: 0.07,
        },
        CaptionStyle::Boxed => CaptionLook {
            font: "Poppins",
            bold: true,
            size: 0.052,
            fill: 0xFFFFFF,
            outline: None,
            band: Some((0x000000, 0.75, 0.016)),
            shadow: 0.0,
            uppercase: false,
            lift: 0.07,
        },
        CaptionStyle::Punch => CaptionLook {
            font: "Anton",
            bold: false,
            size: 0.09,
            fill: 0xFFD43B,
            outline: Some((0x000000, 0.008)),
            band: None,
            shadow: 0.0,
            uppercase: true,
            lift: 0.25,
        },
    }
}

/// The fonts Bardo ships for captions.
const FONTS: [(&str, &[u8]); 2] = [
    (
        "Poppins-Bold.ttf",
        include_bytes!("../../fonts/Poppins-Bold.ttf"),
    ),
    (
        "Anton-Regular.ttf",
        include_bytes!("../../fonts/Anton-Regular.ttf"),
    ),
];

/// Space kept at each side of a caption, as a fraction of the width.
const SIDE_MARGIN: f32 = 0.06;

/// An ASS colour: `&HAABBGGRR`, alpha 0 opaque.
fn ass_colour(rgb: u32, opacity: f32) -> String {
    let alpha = ((1.0 - opacity.clamp(0.0, 1.0)) * 255.0).round() as u32;
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    format!("&H{alpha:02X}{b:02X}{g:02X}{r:02X}")
}

/// An ASS time, `H:MM:SS.CC`, to the nearest hundredth.
fn ass_time(time: Duration) -> String {
    let centis = (time.as_millis() + 5) / 10;
    format!(
        "{}:{:02}:{:02}.{:02}",
        centis / 360_000,
        centis / 6_000 % 60,
        centis / 100 % 60,
        centis % 100
    )
}

/// A caption's text as libass draws it literally: braces would open an
/// override block and a backslash an escape (`\N`, `\h`), so braces are
/// escaped and every backslash is followed by an invisible word joiner.
fn ass_text(text: &str, uppercase: bool) -> String {
    let text = if uppercase {
        text.to_uppercase()
    } else {
        text.to_owned()
    };
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => escaped.push_str("\\\u{2060}"),
            '{' => escaped.push_str("\\{"),
            '}' => escaped.push_str("\\}"),
            '\n' | '\r' => escaped.push(' '),
            c => escaped.push(c),
        }
    }
    escaped
}

/// The ASS script drawing `track` on frames of `size`.
pub(super) fn script(track: &CaptionTrack, size: FrameSize) -> String {
    let look = caption_look(track.style);
    let (width, height) = (size.width as f32, size.height as f32);
    let short = width.min(height);
    let px = |fraction: f32| (fraction * short).round().max(0.0);
    // Style 1 outlines the letters; style 4 sets them on a band, padded by
    // the "outline" width, with the letters' own outline transparent.
    let (border_style, outline_colour, outline, back_colour) = match (look.band, look.outline) {
        (Some((colour, opacity, padding)), _) => (
            4,
            ass_colour(0, 0.0),
            px(padding),
            ass_colour(colour, opacity),
        ),
        (None, Some((colour, width))) => {
            (1, ass_colour(colour, 1.0), px(width), ass_colour(0, 0.5))
        }
        (None, None) => (1, ass_colour(0, 0.0), 0.0, ass_colour(0, 0.5)),
    };
    let fill = ass_colour(look.fill, 1.0);
    let side = (SIDE_MARGIN * width).round();
    let lift = (look.lift * height).round();
    let mut script = format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: {}\n\
         PlayResY: {}\n\
         WrapStyle: 0\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
         BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Caption,{},{},{fill},{fill},{outline_colour},{back_colour},{},0,0,0,100,100,0,0,\
         {border_style},{outline},{},2,{side},{side},{lift},1\n\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        size.width,
        size.height,
        look.font,
        px(look.size),
        if look.bold { -1 } else { 0 },
        px(look.shadow),
    );
    for line in &track.lines {
        script.push_str(&format!(
            "Dialogue: 0,{},{},Caption,,0,0,0,,{}\n",
            ass_time(line.at),
            ass_time(line.at + line.duration),
            ass_text(&line.text, look.uppercase)
        ));
    }
    script
}

/// A file deleted when dropped.
#[derive(Debug)]
pub(super) struct TempFile(PathBuf);

impl TempFile {
    fn write(name: &str, contents: &[u8]) -> io::Result<TempFile> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "bardo-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, contents)?;
        Ok(TempFile(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Writes the shipped fonts to a folder of their own, once per run (and
/// again only when a file there is not the one shipped).
fn fonts_dir() -> Result<&'static Path, MediaError> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    if let Some(dir) = DIR.get() {
        return Ok(dir);
    }
    let dir = std::env::temp_dir().join(format!("bardo-fonts-{}", env!("CARGO_PKG_VERSION")));
    std::fs::create_dir_all(&dir)?;
    for (name, bytes) in FONTS {
        let path = dir.join(name);
        let current = std::fs::metadata(&path).map(|meta| meta.len()).ok();
        if current != Some(bytes.len() as u64) {
            // Written aside and renamed, so a reader never sees half a font.
            let partial = dir.join(format!("{name}.{}.partial", std::process::id()));
            std::fs::write(&partial, bytes)?;
            std::fs::rename(&partial, &path)?;
        }
    }
    Ok(DIR.get_or_init(|| dir))
}

/// Escapes a value for a filter option (`key=value`): `\`, `'` and `:`.
fn escape_option(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | '\'' | ':') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Escapes a filter's arguments for a filter graph: `\`, `'`, `[`, `]`,
/// `,` and `;`.
fn escape_graph(arguments: &str) -> String {
    let mut escaped = String::with_capacity(arguments.len());
    for c in arguments.chars() {
        if matches!(c, '\\' | '\'' | '[' | ']' | ',' | ';') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// A path as a filter option reads it: forward slashes (Windows takes
/// them too), escaped for both levels.
fn path_option(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The `subtitles` filter drawing `script` with the shipped fonts.
pub(super) fn subtitles_filter(script: &Path, fonts: &Path) -> String {
    let arguments = format!(
        "filename={}:fontsdir={}",
        escape_option(&path_option(script)),
        escape_option(&path_option(fonts))
    );
    format!("subtitles={}", escape_graph(&arguments))
}

/// The captions of a plan written for one run: the script lives as long as
/// this does.
#[derive(Debug)]
pub(super) struct CaptionFiles {
    script: TempFile,
    fonts: &'static Path,
}

impl CaptionFiles {
    /// Writes `track`'s script for frames of `size`; `None` when it has no
    /// lines to draw.
    pub(super) fn write(
        track: Option<&CaptionTrack>,
        size: FrameSize,
    ) -> Result<Option<CaptionFiles>, MediaError> {
        let Some(track) = track.filter(|track| !track.lines.is_empty()) else {
            return Ok(None);
        };
        let fonts = fonts_dir()?;
        let script = TempFile::write("captions.ass", script(track, size).as_bytes())?;
        Ok(Some(CaptionFiles { script, fonts }))
    }

    pub(super) fn filter(&self) -> String {
        subtitles_filter(self.script.path(), self.fonts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn line(text: &str, at: u64, duration: u64) -> CaptionLine {
        CaptionLine {
            text: text.into(),
            at: ms(at),
            duration: ms(duration),
        }
    }

    #[test]
    fn colours_are_alpha_blue_green_red() {
        assert_eq!(ass_colour(0xFFD43B, 1.0), "&H003BD4FF");
        assert_eq!(ass_colour(0x000000, 0.75), "&H40000000");
        assert_eq!(ass_colour(0x123456, 0.0), "&HFF563412");
    }

    #[test]
    fn times_are_in_hundredths() {
        assert_eq!(ass_time(Duration::ZERO), "0:00:00.00");
        assert_eq!(ass_time(ms(1_033)), "0:00:01.03");
        assert_eq!(ass_time(ms(1_036)), "0:00:01.04");
        assert_eq!(ass_time(ms(3_725_500)), "1:02:05.50");
    }

    #[test]
    fn text_is_drawn_literally() {
        assert_eq!(ass_text("a {b} c", false), "a \\{b\\} c");
        assert_eq!(ass_text("x \\N y", false), "x \\\u{2060}N y");
        assert_eq!(ass_text("two\nlines", false), "two lines");
        assert_eq!(ass_text("ação já", true), "AÇÃO JÁ");
    }

    #[test]
    fn the_script_sizes_its_style_to_the_frame() {
        let track = CaptionTrack {
            style: CaptionStyle::Clean,
            lines: vec![
                line("The keeper {woke}.", 0, 1_000),
                line("Then", 1_000, 500),
            ],
        };
        let script = script(&track, FrameSize::new(1080, 1920));
        assert!(script.contains("PlayResX: 1080\nPlayResY: 1920\n"));
        // 5.8% of 1080, outlined 0.4%, 7% of 1920 up from the bottom.
        assert!(script.contains(
            "Style: Caption,Poppins,63,&H00FFFFFF,&H00FFFFFF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,4,2,2,65,65,134,1\n"
        ));
        assert!(script.ends_with(
            "Dialogue: 0,0:00:00.00,0:00:01.00,Caption,,0,0,0,,The keeper \\{woke\\}.\n\
             Dialogue: 0,0:00:01.00,0:00:01.50,Caption,,0,0,0,,Then\n"
        ));
    }

    #[test]
    fn a_banded_style_pads_a_band_and_drops_the_outline() {
        let track = CaptionTrack {
            style: CaptionStyle::Boxed,
            lines: vec![line("Boxed", 0, 1_000)],
        };
        let script = script(&track, FrameSize::new(960, 540));
        assert!(script.contains(",&HFF000000,&H40000000,-1,0,0,0,100,100,0,0,4,9,0,2,"));
        let punch = CaptionTrack {
            style: CaptionStyle::Punch,
            lines: vec![line("Loud", 0, 1_000)],
        };
        let script = super::script(&punch, FrameSize::new(960, 540));
        assert!(script.contains("Style: Caption,Anton,49,&H003BD4FF,"));
        assert!(script.ends_with(",,LOUD\n"));
    }

    #[test]
    fn a_track_from_a_point_keeps_what_is_left() {
        let track = CaptionTrack {
            style: CaptionStyle::Clean,
            lines: vec![
                line("a", 0, 1_000),
                line("b", 1_000, 1_000),
                line("c", 2_500, 500),
            ],
        };
        let later = track.starting_at(ms(1_500));
        assert_eq!(later.lines, vec![line("b", 0, 500), line("c", 1_000, 500)]);
    }

    #[test]
    fn paths_are_escaped_for_the_option_and_the_graph() {
        let filter = subtitles_filter(
            Path::new(r"C:\Users\Zoë\Temp\bardo-1-0-captions.ass"),
            Path::new("/tmp/it's [here], ok; yes"),
        );
        assert_eq!(
            filter,
            r"subtitles=filename=C\\:/Users/Zoë/Temp/bardo-1-0-captions.ass:fontsdir=/tmp/it\\\'s \[here\]\, ok\; yes"
        );
    }

    #[test]
    fn no_lines_write_no_files() {
        let empty = CaptionTrack {
            style: CaptionStyle::Clean,
            lines: Vec::new(),
        };
        assert!(
            CaptionFiles::write(Some(&empty), FrameSize::new(960, 540))
                .unwrap()
                .is_none()
        );
        assert!(
            CaptionFiles::write(None, FrameSize::new(960, 540))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn the_script_file_goes_with_its_run() {
        let track = CaptionTrack {
            style: CaptionStyle::Clean,
            lines: vec![line("a", 0, 1_000)],
        };
        let files = CaptionFiles::write(Some(&track), FrameSize::new(960, 540))
            .unwrap()
            .unwrap();
        let path = files.script.path().to_owned();
        assert!(path.is_file());
        for (name, bytes) in FONTS {
            assert_eq!(
                std::fs::metadata(files.fonts.join(name)).unwrap().len(),
                bytes.len() as u64
            );
        }
        drop(files);
        assert!(!path.exists());
    }
}
