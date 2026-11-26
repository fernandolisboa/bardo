//! An end card's text (#281, ADR-0044): the headline, the second line and
//! the offer line drawn by libass in the fonts Bardo ships, in one of the
//! thumbnail looks (ADR-0021), on the card picture `bardo-media`
//! composed. Written as a PNG of the same size.

use std::path::Path;

use bardo_domain::{CardLayout, CardText, CardTextAlign, ThumbnailLook};

use super::captions::{TempFile, ass_colour, ass_text, fonts_dir, subtitles_filter};
use super::{Ffmpeg, MediaError, partial_path, path_arg, process, remove_partial};

/// The letters' heights, as shares of the frame's short side: the
/// headline, the second line and the offer.
const HEADLINE: f64 = 0.096;
const SECOND_LINE: f64 = 0.05;
const OFFER: f64 = 0.06;

/// The ASS script drawing `text` in `layout`'s text box.
pub(super) fn card_script(text: &CardText, layout: &CardLayout) -> String {
    let short = f64::from(layout.width.min(layout.height));
    // Poppins is wider than Anton.
    let scale = match text.look {
        ThumbnailLook::Banner => 0.72,
        ThumbnailLook::Bold | ThumbnailLook::Yellow => 1.0,
    };
    let size = |share: f64| (share * short * scale).round() as u32;
    let (
        font,
        bold,
        headline_fill,
        line_fill,
        offer_fill,
        border_style,
        outline,
        back,
        shadow,
        upper,
    ) = match text.look {
        ThumbnailLook::Bold => (
            "Anton",
            0,
            0xFFFFFF,
            0xFFFFFF,
            0xFFD43B,
            1,
            8,
            ass_colour(0x000000, 0.6),
            3,
            true,
        ),
        ThumbnailLook::Yellow => (
            "Anton",
            0,
            0xFFD43B,
            0xFFFFFF,
            0xFFFFFF,
            1,
            8,
            ass_colour(0x000000, 0.6),
            3,
            true,
        ),
        ThumbnailLook::Banner => (
            "Poppins",
            -1,
            0xFFFFFF,
            0xFFFFFF,
            0xFFD43B,
            4,
            18,
            ass_colour(0x000000, 0.82),
            0,
            false,
        ),
    };
    let outline_colour = match border_style {
        4 => ass_colour(0x000000, 0.0),
        _ => ass_colour(0x000000, 1.0),
    };
    let box_ = layout.text;
    let (align, margin_v, position) = match layout.align {
        CardTextAlign::Top => (8, box_.y, String::new()),
        CardTextAlign::Bottom => (2, layout.height - box_.bottom(), String::new()),
        CardTextAlign::Middle => (
            5,
            0,
            format!(
                "\\pos({},{})",
                box_.x + box_.width / 2,
                box_.y + box_.height / 2
            ),
        ),
    };
    let mut lines = vec![format!(
        "{{\\fs{}\\c{}}}{}",
        size(HEADLINE),
        ass_colour(headline_fill, 1.0),
        ass_text(&text.headline, upper)
    )];
    if !text.second_line.trim().is_empty() {
        lines.push(format!(
            "{{\\fs{}\\c{}}}{}",
            size(SECOND_LINE),
            ass_colour(line_fill, 1.0),
            // A link or handle keeps its case.
            ass_text(&text.second_line, false)
        ));
    }
    if let Some(offer) = text
        .offer
        .as_deref()
        .filter(|offer| !offer.trim().is_empty())
    {
        lines.push(format!(
            "{{\\fs{}\\c{}}}{}",
            size(OFFER),
            ass_colour(offer_fill, 1.0),
            ass_text(offer, upper)
        ));
    }
    let margin_l = box_.x;
    let margin_r = layout.width - box_.right();
    format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: {width}\n\
         PlayResY: {height}\n\
         WrapStyle: 0\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
         BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Card,{font},{headline},{fill},{fill},{outline_colour},{back},{bold},0,0,0,\
         100,100,0,0,{border_style},{outline},{shadow},{align},{margin_l},{margin_r},{margin_v},1\n\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
         Dialogue: 0,0:00:00.00,0:00:10.00,Card,,{margin_l},{margin_r},{margin_v},,{{\\an{align}{position}}}{text}\n",
        width = layout.width,
        height = layout.height,
        headline = size(HEADLINE),
        fill = ass_colour(headline_fill, 1.0),
        text = lines.join("\\N"),
    )
}

impl Ffmpeg {
    /// Sets the card's `text` on the picture at `picture`, in `layout`'s
    /// text box, into the PNG at `destination`, of the same size.
    pub fn draw_card_text(
        &self,
        picture: &Path,
        layout: &CardLayout,
        text: &CardText,
        destination: &Path,
    ) -> Result<(), MediaError> {
        let fonts = fonts_dir()?;
        let script = TempFile::write("card.ass", card_script(text, layout).as_bytes())?;
        let graph = format!("{},format=rgb24", subtitles_filter(script.path(), fonts));
        let partial = partial_path(destination);
        let mut command = self.ffmpeg();
        command
            .arg("-y")
            .arg("-i")
            .arg(path_arg(picture))
            .args(["-frames:v", "1", "-vf"])
            .arg(&graph)
            .args(["-c:v", "png", "-f", "image2", "-update", "1"])
            .arg(path_arg(&partial));
        if let Err(error) = process::output(command) {
            remove_partial(&partial);
            return Err(error);
        }
        std::fs::rename(&partial, destination)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bardo_domain::{AspectRatio, CardLogoPlace, CardTextPlace};

    fn text(look: ThumbnailLook, offer: Option<&str>) -> CardText {
        CardText {
            headline: "Get yours {now}".into(),
            second_line: "pocketloom.com".into(),
            offer: offer.map(str::to_owned),
            look,
        }
    }

    #[test]
    fn the_lines_go_in_the_text_box_one_under_the_other() {
        let layout = CardLayout::of(
            AspectRatio::Vertical,
            CardTextPlace::Above,
            CardLogoPlace::Corner,
            true,
            false,
        );
        let script = card_script(&text(ThumbnailLook::Bold, Some("20% off")), &layout);
        assert!(script.contains("PlayResX: 1080") && script.contains("PlayResY: 1920"));
        // Above the image: set at the bottom of its box, within its sides.
        let margin_v = layout.height - layout.text.bottom();
        let margin_r = layout.width - layout.text.right();
        assert!(script.contains(&format!(
            ",{},{},{margin_v},,{{\\an2}}",
            layout.text.x, margin_r
        )));
        // Three lines; the headline and the offer in capitals, the link as
        // typed, and braces escaped.
        assert!(script.contains("GET YOURS \\{NOW\\}\\N"));
        assert!(script.contains("pocketloom.com\\N"));
        assert!(script.contains("20% OFF"));
        assert!(script.contains("Anton"));
    }

    #[test]
    fn a_card_of_text_sets_it_in_the_middle_and_leaves_out_what_it_lacks() {
        let layout = CardLayout::of(
            AspectRatio::Landscape,
            CardTextPlace::Below,
            CardLogoPlace::Corner,
            false,
            false,
        );
        let mut plain = text(ThumbnailLook::Banner, None);
        plain.second_line = String::new();
        let script = card_script(&plain, &layout);
        assert!(script.contains("{\\an5\\pos("));
        assert!(script.contains("Poppins"));
        assert!(!script.contains("\\N"), "one line only");
        assert!(
            script.contains("Get yours \\{now\\}"),
            "Banner keeps the case"
        );
    }
}
