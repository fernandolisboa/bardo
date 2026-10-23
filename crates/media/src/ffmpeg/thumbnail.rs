//! A YouTube thumbnail (#182, ADR-0021): a picture scaled to cover the
//! 16:9 frame and centred, with the text drawn by libass in the fonts
//! Bardo ships, as the captions are. Written as a JPEG small enough for
//! every YouTube limit.

use std::path::{Path, PathBuf};

use bardo_domain::{
    MAX_THUMBNAIL_BYTES, THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH, TextPlacement, ThumbnailLook,
};

use super::captions::{TempFile, ass_colour, ass_text, fonts_dir, subtitles_filter};
use super::{Ffmpeg, MediaError, partial_path, path_arg, process, remove_partial};

/// What a thumbnail is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailSpec {
    /// The picture behind the text: any image ffmpeg reads.
    pub background: PathBuf,
    /// One line; empty for a picture alone.
    pub text: String,
    pub placement: TextPlacement,
    pub look: ThumbnailLook,
}

/// JPEG qualities tried in turn (ffmpeg's `-q:v`, lower is better) until
/// the file is small enough.
const QUALITIES: [u8; 4] = [2, 4, 7, 12];

/// Space kept between the text and the frame's edges, in pixels.
const MARGIN: u32 = 56;

/// The letters' height for `text`: the shorter the text, the bigger.
fn font_size(text: &str, look: ThumbnailLook) -> u32 {
    let size = match text.chars().count() {
        0..=12 => 190,
        13..=24 => 150,
        _ => 112,
    };
    match look {
        // Poppins is wider than Anton.
        ThumbnailLook::Banner => size * 7 / 10,
        ThumbnailLook::Bold | ThumbnailLook::Yellow => size,
    }
}

/// The ASS alignment (numeric keypad) of a placement.
fn alignment(placement: TextPlacement) -> u8 {
    match placement {
        TextPlacement::TopLeft => 7,
        TextPlacement::Top => 8,
        TextPlacement::TopRight => 9,
        TextPlacement::BottomLeft => 1,
        TextPlacement::Bottom => 2,
        TextPlacement::BottomRight => 3,
    }
}

/// The ASS script drawing `text` on the thumbnail.
pub(super) fn thumbnail_script(
    text: &str,
    placement: TextPlacement,
    look: ThumbnailLook,
) -> String {
    let size = font_size(text, look);
    // Style 1 outlines the letters; style 4 sets them on a band, padded by
    // the "outline" width.
    let (font, bold, fill, border_style, outline_colour, outline, back_colour, shadow, upper) =
        match look {
            ThumbnailLook::Bold => (
                "Anton",
                0,
                0xFFFFFF,
                1,
                ass_colour(0x000000, 1.0),
                10,
                ass_colour(0x000000, 0.6),
                4,
                true,
            ),
            ThumbnailLook::Yellow => (
                "Anton",
                0,
                0xFFD43B,
                1,
                ass_colour(0x000000, 1.0),
                10,
                ass_colour(0x000000, 0.6),
                4,
                true,
            ),
            ThumbnailLook::Banner => (
                "Poppins",
                -1,
                0xFFFFFF,
                4,
                ass_colour(0x000000, 0.0),
                22,
                ass_colour(0x000000, 0.82),
                0,
                false,
            ),
        };
    format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: {THUMBNAIL_WIDTH}\n\
         PlayResY: {THUMBNAIL_HEIGHT}\n\
         WrapStyle: 0\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
         BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Thumb,{font},{size},{fill},{fill},{outline_colour},{back_colour},{bold},0,0,0,\
         100,100,0,0,{border_style},{outline},{shadow},{align},{MARGIN},{MARGIN},{MARGIN},1\n\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
         Dialogue: 0,0:00:00.00,0:00:10.00,Thumb,,0,0,0,,{text}\n",
        fill = ass_colour(fill, 1.0),
        align = alignment(placement),
        text = ass_text(text, upper),
    )
}

/// The filter graph: the background covering the frame, centred, then the
/// text when there is one.
fn filter(script: Option<&Path>, fonts: &Path) -> String {
    let mut graph = format!(
        "scale={THUMBNAIL_WIDTH}:{THUMBNAIL_HEIGHT}:force_original_aspect_ratio=increase:\
         flags=bicubic,crop={THUMBNAIL_WIDTH}:{THUMBNAIL_HEIGHT},setsar=1,format=yuv420p"
    );
    if let Some(script) = script {
        graph.push(',');
        graph.push_str(&subtitles_filter(script, fonts));
    }
    graph
}

impl Ffmpeg {
    /// Composes the thumbnail `spec` describes into the JPEG at
    /// `destination`, within [`MAX_THUMBNAIL_BYTES`].
    pub fn compose_thumbnail(
        &self,
        spec: &ThumbnailSpec,
        destination: &Path,
    ) -> Result<(), MediaError> {
        let fonts = fonts_dir()?;
        let script = if spec.text.trim().is_empty() {
            None
        } else {
            let script = thumbnail_script(&spec.text, spec.placement, spec.look);
            Some(TempFile::write("thumbnail.ass", script.as_bytes())?)
        };
        let graph = filter(script.as_ref().map(TempFile::path), fonts);
        let partial = partial_path(destination);
        for quality in QUALITIES {
            let mut command = self.ffmpeg();
            command
                .arg("-y")
                .arg("-i")
                .arg(path_arg(&spec.background))
                .args(["-frames:v", "1", "-vf"])
                .arg(&graph)
                .args(["-c:v", "mjpeg", "-q:v", &quality.to_string()])
                .args(["-pix_fmt", "yuvj420p", "-f", "mjpeg"])
                .arg(path_arg(&partial));
            if let Err(error) = process::output(command) {
                remove_partial(&partial);
                return Err(error);
            }
            if std::fs::metadata(&partial)?.len() <= MAX_THUMBNAIL_BYTES {
                std::fs::rename(&partial, destination)?;
                return Ok(());
            }
        }
        remove_partial(&partial);
        Err(MediaError::Io(std::io::Error::other(
            "the thumbnail stays over 2 MB at every quality",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_goes_where_it_was_placed_in_the_look_picked() {
        let script = thumbnail_script(
            "It was never found",
            TextPlacement::Bottom,
            ThumbnailLook::Yellow,
        );
        assert!(script.contains("PlayResX: 1280\nPlayResY: 720"));
        assert!(
            script.contains("Style: Thumb,Anton,150,&H003BD4FF,"),
            "{script}"
        );
        assert!(script.contains(",1,10,4,2,56,56,56,1\n"), "{script}");
        assert!(script.contains(",,IT WAS NEVER FOUND\n"));
        let banner = thumbnail_script(
            "Lost {here}",
            TextPlacement::TopRight,
            ThumbnailLook::Banner,
        );
        assert!(
            banner.contains("Style: Thumb,Poppins,133,&H00FFFFFF,"),
            "{banner}"
        );
        assert!(
            banner.contains(",-1,0,0,0,100,100,0,0,4,22,0,9,"),
            "{banner}"
        );
        assert!(banner.contains(",,Lost \\{here\\}\n"), "{banner}");
    }

    #[test]
    fn shorter_text_is_bigger() {
        assert!(
            font_size("Gone", ThumbnailLook::Bold)
                > font_size("Gone for good, they say", ThumbnailLook::Bold)
        );
        assert!(
            font_size("Gone for good, they say", ThumbnailLook::Bold)
                > font_size("Nobody has seen it since that night", ThumbnailLook::Bold)
        );
        let placements: std::collections::HashSet<u8> =
            TextPlacement::ALL.into_iter().map(alignment).collect();
        assert_eq!(placements.len(), 6);
    }

    #[test]
    fn the_background_covers_the_frame_and_the_text_is_optional() {
        let fonts = Path::new("/tmp/fonts");
        assert_eq!(
            filter(None, fonts),
            "scale=1280:720:force_original_aspect_ratio=increase:flags=bicubic,\
             crop=1280:720,setsar=1,format=yuv420p"
        );
        let with = filter(Some(Path::new("/tmp/t.ass")), fonts);
        assert!(with.ends_with(&subtitles_filter(Path::new("/tmp/t.ass"), fonts)));
    }
}
