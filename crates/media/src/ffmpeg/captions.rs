//! Burned-in captions: the plan's caption lines written as an ASS script
//! and drawn by libass (ffmpeg's `subtitles` filter) over the joined video.
//! The fonts ship inside Bardo and libass is pointed at them, so preview
//! and render draw the same letters on any machine, whatever it has
//! installed.
//!
//! Sizes are fractions of the frame, so a 540-line preview and a 1080x1920
//! render lay a caption out alike.
//!
//! A highlighted line is drawn as one event per word spoken: the whole
//! line each time, the spoken word in the highlight colour. Only colours
//! change between them, so libass lays every one out alike and the line
//! never moves. (Karaoke tags would keep a word lit after it is spoken.)

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use bardo_domain::{CaptionHighlight, CaptionStyle};
use serde::{Deserialize, Serialize};

use super::MediaError;
use super::frames::FrameSize;

/// The caption lines of a plan, in one style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionTrack {
    #[serde(with = "style_code")]
    pub style: CaptionStyle,
    /// The colour the spoken word lights up in, if any. Left out of a plan
    /// with none, so plans from before read the same.
    #[serde(
        default = "highlight_off",
        skip_serializing_if = "highlight_is_off",
        with = "highlight_code"
    )]
    pub highlight: CaptionHighlight,
    /// In order; lines may touch but not overlap.
    pub lines: Vec<CaptionLine>,
}

fn highlight_off() -> CaptionHighlight {
    CaptionHighlight::Off
}

fn highlight_is_off(highlight: &CaptionHighlight) -> bool {
    !highlight.is_on()
}

/// One caption as the plan shows it: its text and where on the plan's
/// timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionLine {
    pub text: String,
    pub at: Duration,
    pub duration: Duration,
    /// When each word of the text starts to light up, on the plan's
    /// timeline: one per word as the narrator reads them
    /// (`bardo_domain::spoken_words`), in order, or none for a line drawn
    /// whole.
    /// A word starting before `at` lights up from there; one starting at
    /// or past the line's end does not light up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Duration>,
}

/// A highlight by its stored code, so a plan can be written down.
mod highlight_code {
    use bardo_domain::CaptionHighlight;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        highlight: &CaptionHighlight,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(highlight.code())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<CaptionHighlight, D::Error> {
        let code = String::deserialize(deserializer)?;
        code.parse().map_err(serde::de::Error::custom)
    }
}

/// A caption style by its stored code, so a plan can be written down.
mod style_code {
    use bardo_domain::CaptionStyle;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        style: &CaptionStyle,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(style.code())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<CaptionStyle, D::Error> {
        let code = String::deserialize(deserializer)?;
        code.parse().map_err(serde::de::Error::custom)
    }
}

impl CaptionTrack {
    /// The lines from `from` on, as if the timeline started there.
    pub(super) fn starting_at(&self, from: Duration) -> CaptionTrack {
        CaptionTrack {
            style: self.style,
            highlight: self.highlight,
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
                        words: line
                            .words
                            .iter()
                            .map(|start| start.saturating_sub(from))
                            .collect(),
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

/// The colour (`0xRRGGBB`) the spoken word lights up in; `None` when off.
pub fn highlight_colour(highlight: CaptionHighlight) -> Option<u32> {
    match highlight {
        CaptionHighlight::Off => None,
        CaptionHighlight::Yellow => Some(0xFFD43B),
        CaptionHighlight::Green => Some(0x4ADE80),
        CaptionHighlight::Cyan => Some(0x38D9F5),
        CaptionHighlight::Pink => Some(0xFF5FA2),
        CaptionHighlight::Orange => Some(0xFF9F1C),
    }
}

impl CaptionLook {
    /// The colour of the words not being spoken: the style's own, or white
    /// when a word lights up, so the lit word stands out in every style
    /// (Punch's letters are yellow).
    pub fn words_fill(&self, highlight: CaptionHighlight) -> u32 {
        if highlight.is_on() {
            0xFFFFFF
        } else {
            self.fill
        }
    }
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

/// A colour for an override tag (`\\1c`): `&HBBGGRR&`.
fn ass_tag_colour(rgb: u32) -> String {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    format!("&H{b:02X}{g:02X}{r:02X}&")
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
    let fill = ass_colour(look.words_fill(track.highlight), 1.0);
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
    let lit = highlight_colour(track.highlight).map(ass_tag_colour);
    let unlit = ass_tag_colour(look.words_fill(track.highlight));
    let mut event = |from: Duration, to: Duration, text: String| {
        // Times as drawn: a stretch under a hundredth draws nothing.
        let (from, to) = (ass_time(from), ass_time(to));
        if from != to {
            script.push_str(&format!("Dialogue: 0,{from},{to},Caption,,0,0,0,,{text}\n"));
        }
    };
    for line in &track.lines {
        let end = line.at + line.duration;
        // Words as the narrator reads them, as their times were given.
        let words = bardo_domain::spoken_words(&line.text);
        let Some(lit) = lit
            .as_ref()
            .filter(|_| !line.words.is_empty() && line.words.len() == words.len())
        else {
            event(line.at, end, ass_text(&line.text, look.uppercase));
            continue;
        };
        // The line whole until its first word lights up.
        let first = line.words[0].clamp(line.at, end);
        event(line.at, first, ass_text(&line.text, look.uppercase));
        for (index, start) in line.words.iter().enumerate() {
            let from = (*start).clamp(line.at, end);
            let to = line
                .words
                .get(index + 1)
                .map_or(end, |next| (*next).clamp(from, end));
            if from >= to {
                continue;
            }
            let spoken = words[index].clone();
            let text = format!(
                "{}{{\\1c{lit}}}{}{{\\1c{unlit}}}{}",
                ass_text(&line.text[..spoken.start], look.uppercase),
                ass_text(&line.text[spoken.clone()], look.uppercase),
                ass_text(&line.text[spoken.end..], look.uppercase),
            );
            event(from, to, text);
        }
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
        // A new file only: never write through one already there.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        io::Write::write_all(&mut file, contents)?;
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
/// again only when a file there is not byte for byte the one shipped).
fn fonts_dir() -> Result<&'static Path, MediaError> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    // The preview and a render may both start first.
    static WRITING: Mutex<()> = Mutex::new(());
    if let Some(dir) = DIR.get() {
        return Ok(dir);
    }
    let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(dir) = DIR.get() {
        return Ok(dir);
    }
    let dir = std::env::temp_dir().join(format!("bardo-fonts-{}", env!("CARGO_PKG_VERSION")));
    std::fs::create_dir_all(&dir)?;
    for (name, bytes) in FONTS {
        let path = dir.join(name);
        if std::fs::read(&path).ok().as_deref() != Some(bytes) {
            // Written aside and renamed, so a reader never sees half a font.
            let partial = dir.join(format!("{name}.{}.partial", std::process::id()));
            std::fs::write(&partial, bytes)?;
            std::fs::rename(&partial, &path)?;
        }
    }
    Ok(DIR.get_or_init(|| dir))
}

/// Escapes a value for a filter option (`key=value`): `\`, `'` and `:`.
pub(super) fn escape_option(value: &str) -> String {
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
pub(super) fn escape_graph(arguments: &str) -> String {
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
pub(super) fn path_option(path: &Path) -> String {
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
            words: Vec::new(),
        }
    }

    fn lit(text: &str, at: u64, duration: u64, words: &[u64]) -> CaptionLine {
        CaptionLine {
            words: words.iter().map(|&start| ms(start)).collect(),
            ..line(text, at, duration)
        }
    }

    /// The script's events: start, end and text.
    fn events(script: &str) -> Vec<(String, String, String)> {
        script
            .lines()
            .filter_map(|line| line.strip_prefix("Dialogue: 0,"))
            .map(|event| {
                let fields: Vec<&str> = event.splitn(9, ',').collect();
                (
                    fields[0].to_owned(),
                    fields[1].to_owned(),
                    fields[8].to_owned(),
                )
            })
            .collect()
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
            highlight: CaptionHighlight::Off,
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
            highlight: CaptionHighlight::Off,
            lines: vec![line("Boxed", 0, 1_000)],
        };
        let script = script(&track, FrameSize::new(960, 540));
        assert!(script.contains(",&HFF000000,&H40000000,-1,0,0,0,100,100,0,0,4,9,0,2,"));
        let punch = CaptionTrack {
            style: CaptionStyle::Punch,
            highlight: CaptionHighlight::Off,
            lines: vec![line("Loud", 0, 1_000)],
        };
        let script = super::script(&punch, FrameSize::new(960, 540));
        assert!(script.contains("Style: Caption,Anton,49,&H003BD4FF,"));
        assert!(script.ends_with(",,LOUD\n"));
    }

    #[test]
    fn the_spoken_word_lights_up_in_one_event_each() {
        let track = CaptionTrack {
            style: CaptionStyle::Clean,
            highlight: CaptionHighlight::Yellow,
            // Lit from 1.2 s: shown whole until then.
            lines: vec![lit(
                "The {keeper} woke",
                1_000,
                1_500,
                &[1_200, 1_500, 2_000],
            )],
        };
        let script = script(&track, FrameSize::new(1080, 1920));
        let (lit, white) = ("{\\1c&H3BD4FF&}", "{\\1c&HFFFFFF&}");
        assert_eq!(
            events(&script),
            [
                (
                    "0:00:01.00",
                    "0:00:01.20",
                    "The \\{keeper\\} woke".to_owned()
                ),
                (
                    "0:00:01.20",
                    "0:00:01.50",
                    format!("{lit}The{white} \\{{keeper\\}} woke")
                ),
                (
                    "0:00:01.50",
                    "0:00:02.00",
                    format!("The {lit}\\{{keeper\\}}{white} woke")
                ),
                (
                    "0:00:02.00",
                    "0:00:02.50",
                    format!("The \\{{keeper\\}} {lit}woke{white}")
                ),
            ]
            .map(|(from, to, text)| (from.to_owned(), to.to_owned(), text))
        );
    }

    #[test]
    fn a_highlight_turns_the_other_words_white_in_every_style() {
        let track = |style, highlight| CaptionTrack {
            style,
            highlight,
            lines: vec![lit("Loud now", 0, 1_000, &[0, 500])],
        };
        let script = script(
            &track(CaptionStyle::Punch, CaptionHighlight::Green),
            FrameSize::new(960, 540),
        );
        // White letters, the spoken word green, in capitals.
        assert!(script.contains("Style: Caption,Anton,49,&H00FFFFFF,"));
        assert_eq!(
            events(&script)[0].2,
            "{\\1c&H80DE4A&}LOUD{\\1c&HFFFFFF&} NOW"
        );
        // Off, the same line is drawn whole in the style's own yellow.
        let script = script_of(&track(CaptionStyle::Punch, CaptionHighlight::Off));
        assert!(script.contains("Style: Caption,Anton,49,&H003BD4FF,"));
        assert_eq!(events(&script).len(), 1);
    }

    fn script_of(track: &CaptionTrack) -> String {
        script(track, FrameSize::new(960, 540))
    }

    #[test]
    fn words_outside_the_line_and_mismatched_times_draw_it_whole() {
        let track = |lines| CaptionTrack {
            style: CaptionStyle::Clean,
            highlight: CaptionHighlight::Cyan,
            lines,
        };
        // A line after a cut: "a" was spoken before it, "c" after it.
        let events = events(&script_of(&track(vec![lit(
            "a b c",
            1_000,
            1_000,
            &[500, 1_400, 2_000],
        )])));
        assert_eq!(events.len(), 2);
        assert_eq!(
            (events[0].0.as_str(), events[0].1.as_str()),
            ("0:00:01.00", "0:00:01.40")
        );
        assert!(events[0].2.starts_with("{\\1c&HF5D938&}a{"));
        assert_eq!(
            (events[1].0.as_str(), events[1].1.as_str()),
            ("0:00:01.40", "0:00:02.00")
        );
        // Times for two words of three, or none: the line whole.
        for words in [&[1_000, 1_500][..], &[]] {
            let events =
                super::tests::events(&script_of(&track(vec![lit("a b c", 1_000, 1_000, words)])));
            assert_eq!(
                events,
                [("0:00:01.00".into(), "0:00:02.00".into(), "a b c".into())]
            );
        }
        // A word lit for less than a hundredth draws nothing of its own.
        let events = super::tests::events(&script_of(&track(vec![lit("a b", 0, 1_000, &[0, 2])])));
        assert_eq!(events.len(), 1);
        assert!(events[0].2.ends_with("b{\\1c&HFFFFFF&}"));
    }

    #[test]
    fn a_lone_mark_lights_up_with_its_word() {
        let track = CaptionTrack {
            style: CaptionStyle::Clean,
            highlight: CaptionHighlight::Yellow,
            lines: vec![lit("Wait — what", 0, 1_000, &[0, 500])],
        };
        let events = events(&script_of(&track));
        assert_eq!(
            events
                .iter()
                .map(|event| event.2.as_str())
                .collect::<Vec<_>>(),
            [
                "{\\1c&H3BD4FF&}Wait —{\\1c&HFFFFFF&} what",
                "Wait — {\\1c&H3BD4FF&}what{\\1c&HFFFFFF&}",
            ]
        );
    }

    #[test]
    fn a_plan_with_no_highlight_writes_down_as_before() {
        let track = CaptionTrack {
            style: CaptionStyle::Boxed,
            highlight: CaptionHighlight::Off,
            lines: vec![line("a", 0, 1_000)],
        };
        let json = serde_json::to_string(&track).unwrap();
        assert_eq!(
            json,
            r#"{"style":"boxed","lines":[{"text":"a","at":{"secs":0,"nanos":0},"duration":{"secs":1,"nanos":0}}]}"#
        );
        assert_eq!(serde_json::from_str::<CaptionTrack>(&json).unwrap(), track);
        let lit = CaptionTrack {
            highlight: CaptionHighlight::Pink,
            lines: vec![super::tests::lit("a", 0, 1_000, &[0])],
            ..track
        };
        let json = serde_json::to_string(&lit).unwrap();
        assert!(json.contains(r#""highlight":"pink""#));
        assert_eq!(serde_json::from_str::<CaptionTrack>(&json).unwrap(), lit);
    }

    #[test]
    fn a_track_from_a_point_keeps_what_is_left() {
        let track = CaptionTrack {
            style: CaptionStyle::Clean,
            highlight: CaptionHighlight::Off,
            lines: vec![
                line("a", 0, 1_000),
                line("b", 1_000, 1_000),
                line("c", 2_500, 500),
            ],
        };
        let later = track.starting_at(ms(1_500));
        assert_eq!(later.lines, vec![line("b", 0, 500), line("c", 1_000, 500)]);
        let words = CaptionTrack {
            lines: vec![lit("b c", 1_000, 1_000, &[1_000, 1_800])],
            ..track
        };
        assert_eq!(
            words.starting_at(ms(1_500)).lines[0].words,
            [ms(0), ms(300)]
        );
    }

    #[test]
    fn paths_are_escaped_for_the_option_and_the_graph() {
        let filter = subtitles_filter(
            Path::new(r"C:\Users\Données\Temp\bardo-1-0-captions.ass"),
            Path::new("/tmp/it's [here], ok; yes"),
        );
        assert_eq!(
            filter,
            r"subtitles=filename=C\\:/Users/Données/Temp/bardo-1-0-captions.ass:fontsdir=/tmp/it\\\'s \[here\]\, ok\; yes"
        );
    }

    #[test]
    fn no_lines_write_no_files() {
        let empty = CaptionTrack {
            style: CaptionStyle::Clean,
            highlight: CaptionHighlight::Off,
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
            highlight: CaptionHighlight::Off,
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
