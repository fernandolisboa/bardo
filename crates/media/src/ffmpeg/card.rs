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

/// How tall a line of letters is, as a share of their size.
const LINE_HEIGHT: f64 = 1.4;
/// The smallest share of their size the letters shrink to.
const SMALLEST: f64 = 0.3;

/// A generous guess at the advance of a letter, as a share of the
/// letters' size: Anton's capitals, or Poppins in either case.
fn advance(look: ThumbnailLook) -> f64 {
    match look {
        ThumbnailLook::Banner => 0.6,
        ThumbnailLook::Bold | ThumbnailLook::Yellow => 0.46,
    }
}

/// How many lines `text` wraps to at `size` within `width`, breaking
/// between words as libass does, and inside a word too long for a line.
fn wrapped_lines(text: &str, size: f64, width: f64, advance: f64) -> f64 {
    let letter = size * advance;
    let space = letter;
    let (mut lines, mut used) = (1.0_f64, 0.0_f64);
    for word in text.split_whitespace() {
        let wide = word.chars().count() as f64 * letter;
        if wide > width {
            // A word wider than the line takes lines of its own.
            if used > 0.0 {
                lines += 1.0;
            }
            lines += (wide / width).ceil() - 1.0;
            used = wide % width;
            continue;
        }
        let next = if used == 0.0 {
            wide
        } else {
            used + space + wide
        };
        if next > width {
            lines += 1.0;
            used = wide;
        } else {
            used = next;
        }
    }
    lines
}

/// The share of their size the letters are drawn at so that `lines`
/// (each its text and size) fit a box of `width` × `height`, with `pad`
/// kept around them for the outline or band.
fn fitting_scale(
    lines: &[(&str, f64)],
    width: f64,
    height: f64,
    pad: f64,
    look: ThumbnailLook,
) -> f64 {
    let room = ((width - 2.0 * pad).max(1.0), (height - 2.0 * pad).max(1.0));
    let fits = |scale: f64| {
        let tall: f64 = lines
            .iter()
            .map(|(text, size)| {
                let size = size * scale;
                wrapped_lines(text, size, room.0, advance(look)) * size * LINE_HEIGHT
            })
            .sum();
        tall <= room.1
    };
    let mut scale = 1.0;
    while scale > SMALLEST && !fits(scale) {
        scale *= 0.94;
    }
    scale.max(SMALLEST)
}

/// The ASS script drawing `text` in `layout`'s text box: the letters
/// shrink until the lines fit it, and nothing is drawn outside it.
pub(super) fn card_script(text: &CardText, layout: &CardLayout) -> String {
    let short = f64::from(layout.width.min(layout.height));
    // Poppins is wider than Anton.
    let look_scale = match text.look {
        ThumbnailLook::Banner => 0.72,
        ThumbnailLook::Bold | ThumbnailLook::Yellow => 1.0,
    };
    let box_ = layout.text;
    let offer = text
        .offer
        .as_deref()
        .filter(|offer| !offer.trim().is_empty());
    let second = Some(text.second_line.as_str()).filter(|line| !line.trim().is_empty());
    let mut shown = vec![(text.headline.as_str(), HEADLINE * short * look_scale)];
    shown.extend(second.map(|line| (line, SECOND_LINE * short * look_scale)));
    shown.extend(offer.map(|line| (line, OFFER * short * look_scale)));
    // The band's or outline's reach, and the shadow's.
    let pad = match text.look {
        ThumbnailLook::Banner => 24.0,
        ThumbnailLook::Bold | ThumbnailLook::Yellow => 12.0,
    };
    let scale = look_scale
        * fitting_scale(
            &shown,
            f64::from(box_.width),
            f64::from(box_.height),
            pad,
            text.look,
        );
    let size = |share: f64| (share * short * scale).round().max(1.0) as u32;
    let inset = pad as u32;
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
    let (align, margin_v, position) = match layout.align {
        // Kept `inset` inside the box, so the outline, band and shadow
        // stay in it too.
        CardTextAlign::Top => (8, box_.y + inset, String::new()),
        CardTextAlign::Bottom => (2, layout.height - box_.bottom() + inset, String::new()),
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
    if let Some(second) = second {
        lines.push(format!(
            "{{\\fs{}\\c{}}}{}",
            size(SECOND_LINE),
            ass_colour(line_fill, 1.0),
            // A link or handle keeps its case.
            ass_text(second, false)
        ));
    }
    if let Some(offer) = offer {
        lines.push(format!(
            "{{\\fs{}\\c{}}}{}",
            size(OFFER),
            ass_colour(offer_fill, 1.0),
            ass_text(offer, upper)
        ));
    }
    let margin_l = box_.x + inset;
    let margin_r = layout.width - box_.right() + inset;
    // Nothing drawn outside the box, whatever the fonts do.
    let clip = format!(
        "\\clip({},{},{},{})",
        box_.x,
        box_.y,
        box_.right(),
        box_.bottom()
    );
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
         Dialogue: 0,0:00:00.00,0:00:10.00,Card,,{margin_l},{margin_r},{margin_v},,{{\\an{align}{position}{clip}}}{text}\n",
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
        let margin_v = layout.height - layout.text.bottom() + 12;
        let margin_r = layout.width - layout.text.right() + 12;
        assert!(script.contains(&format!(
            ",{},{},{margin_v},,{{\\an2\\clip(",
            layout.text.x + 12,
            margin_r
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

    #[test]
    fn the_longest_lines_shrink_to_fit_every_place_and_stay_in_their_box() {
        let long = CardText {
            headline: "W".repeat(40),
            second_line: "pocketloom.com/collections/winter/sale/2026/notebooks/a5".into(),
            offer: Some("Free shipping on every order this week, no code".into()),
            look: ThumbnailLook::Bold,
        };
        for aspect in bardo_domain::FrameShape::ALL {
            for text_place in CardTextPlace::ALL {
                for logo_place in CardLogoPlace::ALL {
                    for (image, logo) in
                        [(true, true), (true, false), (false, true), (false, false)]
                    {
                        let layout = CardLayout::of(aspect, text_place, logo_place, image, logo);
                        for look in ThumbnailLook::ALL {
                            let text = CardText {
                                look,
                                ..long.clone()
                            };
                            let script = card_script(&text, &layout);
                            let sizes: Vec<f64> = script
                                .split("\\fs")
                                .skip(1)
                                .map(|rest| {
                                    rest.split(|c: char| !c.is_ascii_digit())
                                        .next()
                                        .unwrap()
                                        .parse()
                                        .unwrap()
                                })
                                .collect();
                            let lines = [
                                text.headline.to_uppercase(),
                                text.second_line.clone(),
                                text.offer.clone().unwrap().to_uppercase(),
                            ];
                            let pad = if look == ThumbnailLook::Banner {
                                18.0
                            } else {
                                8.0
                            };
                            let width = f64::from(layout.text.width) - 2.0 * pad;
                            let tall: f64 = lines
                                .iter()
                                .zip(&sizes)
                                .map(|(line, size)| {
                                    wrapped_lines(line, *size, width, advance(look))
                                        * size
                                        * LINE_HEIGHT
                                })
                                .sum();
                            assert!(
                                tall <= f64::from(layout.text.height) - 2.0 * pad + 1.0,
                                "{aspect:?} {text_place:?} {logo_place:?} {image} {logo} {look:?}: {tall}"
                            );
                            let clip = format!(
                                "\\clip({},{},{},{})",
                                layout.text.x,
                                layout.text.y,
                                layout.text.right(),
                                layout.text.bottom()
                            );
                            assert!(script.contains(&clip));
                        }
                    }
                }
            }
        }
        // Short lines keep their full size.
        let layout = CardLayout::of(
            AspectRatio::Vertical,
            CardTextPlace::Above,
            CardLogoPlace::Corner,
            true,
            false,
        );
        let script = card_script(&text(ThumbnailLook::Bold, None), &layout);
        assert!(script.contains(&format!("\\fs{}", (HEADLINE * 1080.0).round())));
    }
}
