//! An ad's end card (#281, ADR-0044): its last picture, composed by Bardo
//! like a thumbnail (ADR-0021) over a backdrop, with the call to action,
//! the link or handle and, optionally, the offer, the product image and
//! the logo. Text is drawn in Bardo's own fonts, never by a model; the
//! card is free and never sent to a model.
//!
//! The card is the plan's last scene when that scene is of kind *end
//! card*: making the last scene another kind turns the card off. Its
//! backdrop, the product image it shows and how that image is set come
//! from the scene's shot look, as for a product shot (ADR-0041); what is
//! here is the card's own design.

use std::time::Duration;

use crate::{
    BriefText, FrameShape, RepositoryError, ShotArea, ThumbnailLook, VideoProjectId, shot_frame,
};

/// The longest headline, in characters.
pub const MAX_CARD_HEADLINE: usize = 40;
/// The longest second line, in characters.
pub const MAX_CARD_SECOND_LINE: usize = 60;
/// The longest offer the card shows: a longer one is not cut, which
/// could drop its conditions, so the card leaves it out.
pub const MAX_CARD_OFFER: usize = 60;
/// How long the card stays after the narration's last word.
pub const END_CARD_TAIL: Duration = Duration::from_millis(1500);
/// The least time the card is on screen.
pub const END_CARD_MIN: Duration = Duration::from_millis(2500);

/// Where the card's text sits: above the product image or below it. A
/// card with no image sets its text in the middle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CardTextPlace {
    #[default]
    Above,
    Below,
}

impl CardTextPlace {
    pub const ALL: [CardTextPlace; 2] = [CardTextPlace::Above, CardTextPlace::Below];

    pub fn code(self) -> &'static str {
        match self {
            CardTextPlace::Above => "above",
            CardTextPlace::Below => "below",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|place| place.code() == code)
    }
}

/// Where the logo sits: in the top left corner of the room the card
/// keeps clear, or centered just above the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CardLogoPlace {
    #[default]
    Corner,
    WithText,
}

impl CardLogoPlace {
    pub const ALL: [CardLogoPlace; 2] = [CardLogoPlace::Corner, CardLogoPlace::WithText];

    pub fn code(self) -> &'static str {
        match self {
            CardLogoPlace::Corner => "corner",
            CardLogoPlace::WithText => "with_text",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|place| place.code() == code)
    }
}

/// A line the user typed on the card that will not do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CardFieldError {
    #[error("the headline is empty")]
    HeadlineEmpty,
    #[error("the headline is longer than {MAX_CARD_HEADLINE} characters")]
    HeadlineTooLong,
    #[error("the second line is longer than {MAX_CARD_SECOND_LINE} characters")]
    SecondLineTooLong,
}

impl CardFieldError {
    pub const ALL: [CardFieldError; 3] = [
        CardFieldError::HeadlineEmpty,
        CardFieldError::HeadlineTooLong,
        CardFieldError::SecondLineTooLong,
    ];
}

/// `text` as one line: spaces and line breaks run together, trimmed.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` as one line of at most `max` characters, cut at the last word
/// that fits (or mid-word when the first word is longer).
fn fitted(text: &str, max: usize) -> String {
    let line = one_line(text);
    if line.chars().count() <= max {
        return line;
    }
    let mut kept = String::new();
    for word in line.split(' ') {
        let next = if kept.is_empty() {
            word.to_owned()
        } else {
            format!("{kept} {word}")
        };
        if next.chars().count() > max {
            break;
        }
        kept = next;
    }
    if kept.is_empty() {
        kept = line.chars().take(max).collect();
    }
    kept
}

/// A link as a viewer reads it: without `https://`, `http://` or a
/// trailing slash.
fn readable_link(link: &str) -> String {
    let link = one_line(link);
    let link = link
        .strip_prefix("https://")
        .or_else(|| link.strip_prefix("http://"))
        .unwrap_or(&link);
    link.trim_end_matches('/').to_owned()
}

/// A link as a viewer reads it, within `max` characters and still a link
/// that works: past the limit, without its query and fragment, then
/// without its last path parts; a host longer than the limit leaves the
/// line empty, never cut mid-way. A handle or text with spaces is cut at
/// a word.
fn short_link(link: &str, max: usize) -> String {
    let link = readable_link(link);
    if link.chars().count() <= max {
        return link;
    }
    if link.contains(' ') {
        return fitted(&link, max);
    }
    let mut short = link
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_owned();
    while short.chars().count() > max {
        match short.rfind('/') {
            Some(at) => short.truncate(at),
            None => return String::new(),
        }
    }
    short
}

/// What the user decides about the card, always valid. The headline and
/// the second line are the user's own text and are not read by the
/// claims check; the offer line can only show the Brief stage's offer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CardDesign {
    headline: String,
    second_line: String,
    /// Whether the card shows the Brief stage's offer.
    pub offer: bool,
    pub look: ThumbnailLook,
    pub text_place: CardTextPlace,
    pub logo_place: CardLogoPlace,
}

impl CardDesign {
    /// A design with these lines: each on one line, the headline not
    /// empty, each within its limit.
    pub fn new(
        headline: &str,
        second_line: &str,
        offer: bool,
        look: ThumbnailLook,
        text_place: CardTextPlace,
        logo_place: CardLogoPlace,
    ) -> Result<Self, Vec<CardFieldError>> {
        let headline = one_line(headline);
        let second_line = one_line(second_line);
        let mut errors = Vec::new();
        if headline.is_empty() {
            errors.push(CardFieldError::HeadlineEmpty);
        }
        if headline.chars().count() > MAX_CARD_HEADLINE {
            errors.push(CardFieldError::HeadlineTooLong);
        }
        if second_line.chars().count() > MAX_CARD_SECOND_LINE {
            errors.push(CardFieldError::SecondLineTooLong);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            headline,
            second_line,
            offer,
            look,
            text_place,
            logo_place,
        })
    }

    /// The design a card starts with: the headline is the Brief stage's
    /// call to action and the second line its link or handle, each cut
    /// to its limit; the offer shows when there is one.
    pub fn start(brief: &BriefText) -> Self {
        let headline = match fitted(brief.call_to_action(), MAX_CARD_HEADLINE) {
            empty if empty.is_empty() => "Learn more".to_owned(),
            headline => headline,
        };
        Self {
            headline,
            second_line: short_link(brief.link(), MAX_CARD_SECOND_LINE),
            offer: Self::offer_fits(brief),
            look: ThumbnailLook::default(),
            text_place: CardTextPlace::default(),
            logo_place: CardLogoPlace::default(),
        }
    }

    pub fn headline(&self) -> &str {
        &self.headline
    }

    /// May be empty.
    pub fn second_line(&self) -> &str {
        &self.second_line
    }

    /// Whether the card can show the Brief stage's offer: there is one,
    /// within [`MAX_CARD_OFFER`] characters.
    pub fn offer_fits(brief: &BriefText) -> bool {
        let offer = brief.offer();
        !offer.is_empty() && offer.chars().count() <= MAX_CARD_OFFER
    }

    /// The offer line the card shows: the Brief stage's offer, when the
    /// design shows it and it fits the card.
    pub fn offer_line<'a>(&self, brief: &'a BriefText) -> Option<&'a str> {
        (self.offer && Self::offer_fits(brief)).then(|| brief.offer())
    }

    /// What the card says, as it is composed.
    pub fn text(&self, brief: &BriefText) -> CardText {
        CardText {
            headline: self.headline.clone(),
            second_line: self.second_line.clone(),
            offer: self.offer_line(brief).map(str::to_owned),
            look: self.look,
        }
    }
}

/// The card of a project, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndCard {
    pub project: VideoProjectId,
    pub design: CardDesign,
}

/// The card's lines and look, as the media engine draws them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CardText {
    pub headline: String,
    /// May be empty.
    pub second_line: String,
    pub offer: Option<String>,
    pub look: ThumbnailLook,
}

/// Everything a composed card shows besides the scene's own image, look
/// and shape, as a short fingerprint: a picture composed from another is
/// out of date. `logo` is the logo file's bytes, if the card shows one.
pub fn card_fingerprint(design: &CardDesign, brief: &BriefText, logo: Option<&[u8]>) -> String {
    let text = design.text(brief);
    let hash = |bytes: &mut dyn Iterator<Item = u8>, start: u64| {
        bytes.fold(start, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let fields = [
        text.headline.as_str(),
        text.second_line.as_str(),
        text.offer.as_deref().unwrap_or(""),
        design.look.code(),
        design.text_place.code(),
        design.logo_place.code(),
    ];
    let mut bytes = fields
        .iter()
        .flat_map(|field| field.bytes().chain(std::iter::once(0)));
    let mut state = hash(&mut bytes, 0xcbf2_9ce4_8422_2325);
    if let Some(logo) = logo {
        state = hash(&mut logo.iter().copied(), state ^ 1);
    }
    format!("{state:016x}")
}

/// The part of a card's frame its text, image and logo may use: clear of
/// what the networks lay over a video (in a vertical frame the bottom,
/// with the caption and buttons, and the right edge, with the buttons; in
/// a wide one a band at the bottom) and of what a zoom crops.
pub fn card_room(shape: impl Into<FrameShape>) -> ShotArea {
    let shape = shape.into();
    let (width, height) = shot_frame(shape);
    // As shares of the frame: left, top, right, bottom. A 4:5 or 1:1 ad
    // (#283) shows in a feed with the network's text below it, not over it.
    let (left, top, right, bottom) = match shape {
        FrameShape::Vertical => (0.07, 0.09, 0.15, 0.24),
        FrameShape::Landscape => (0.07, 0.08, 0.07, 0.16),
        FrameShape::Portrait => (0.07, 0.07, 0.07, 0.1),
        FrameShape::Square => (0.07, 0.07, 0.07, 0.09),
    };
    let x = (left * f64::from(width)).round() as u32;
    let y = (top * f64::from(height)).round() as u32;
    let right = (right * f64::from(width)).round() as u32;
    let bottom = (bottom * f64::from(height)).round() as u32;
    ShotArea {
        x,
        y,
        width: width - x - right,
        height: height - y - bottom,
    }
}

/// Where a card's parts go in its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardLayout {
    pub width: u32,
    pub height: u32,
    /// The box the text is set in.
    pub text: ShotArea,
    /// How the text sits in its box.
    pub align: CardTextAlign,
    /// Where the product image goes, shown whole within it; none for a
    /// card of text.
    pub product: Option<ShotArea>,
    /// Where the logo goes, shown whole within it.
    pub logo: Option<ShotArea>,
}

/// How a card's text sits in its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardTextAlign {
    Top,
    Middle,
    Bottom,
}

/// The text's share of the room's height when the card shows an image.
const TEXT_SHARE: f64 = 0.36;
/// The logo's box, as a share of the frame's short side: its width and
/// its height.
const LOGO_WIDTH: f64 = 0.26;
const LOGO_HEIGHT: f64 = 0.11;
/// The gap kept between the parts, as a share of the frame's short side.
const GAP: f64 = 0.03;

impl CardLayout {
    /// The layout of a card in `shape` with its text at `text_place`, its
    /// logo at `logo_place` when it has one, and a product image when it
    /// has one. Every part stays inside [`card_room`] and no two overlap.
    pub fn of(
        shape: impl Into<FrameShape>,
        text_place: CardTextPlace,
        logo_place: CardLogoPlace,
        image: bool,
        logo: bool,
    ) -> CardLayout {
        let shape = shape.into();
        let (width, height) = shot_frame(shape);
        let short = f64::from(width.min(height));
        let gap = (GAP * short).round() as u32;
        let mut room = card_room(shape);
        let logo_box = (
            ((LOGO_WIDTH * short).round() as u32).min(room.width),
            ((LOGO_HEIGHT * short).round() as u32).min(room.height / 4),
        );
        // A logo in the corner takes a band across the top of the room.
        let mut logo_area = None;
        if logo && logo_place == CardLogoPlace::Corner {
            logo_area = Some(ShotArea {
                x: room.x,
                y: room.y,
                width: logo_box.0,
                height: logo_box.1,
            });
            let taken = logo_box.1 + gap;
            room.y += taken;
            room.height = room.height.saturating_sub(taken);
        }
        let (mut text, align, product) = if image {
            let text_height = (TEXT_SHARE * f64::from(room.height)).round() as u32;
            let product_height = room.height.saturating_sub(text_height + gap);
            let (text, align, product) = match text_place {
                CardTextPlace::Above => (
                    ShotArea {
                        height: text_height,
                        ..room
                    },
                    CardTextAlign::Bottom,
                    ShotArea {
                        y: room.y + text_height + gap,
                        height: product_height,
                        ..room
                    },
                ),
                CardTextPlace::Below => (
                    ShotArea {
                        y: room.y + product_height + gap,
                        height: text_height,
                        ..room
                    },
                    CardTextAlign::Top,
                    ShotArea {
                        height: product_height,
                        ..room
                    },
                ),
            };
            (text, align, Some(product))
        } else {
            (room, CardTextAlign::Middle, None)
        };
        // A logo with the text takes the top of the text's box.
        if logo && logo_place == CardLogoPlace::WithText {
            logo_area = Some(ShotArea {
                x: text.x + (text.width - logo_box.0) / 2,
                y: text.y,
                width: logo_box.0,
                height: logo_box.1,
            });
            let taken = logo_box.1 + gap;
            text.y += taken;
            text.height = text.height.saturating_sub(taken);
        }
        CardLayout {
            width,
            height,
            text,
            align,
            product,
            logo: logo_area,
        }
    }
}

/// When the end card leaves the screen: [`END_CARD_TAIL`] after the
/// narration's last word, later when that would leave it less than
/// [`END_CARD_MIN`] from its start (the call to action's first word).
pub fn end_card_end(card_start: Duration, last_word_end: Duration) -> Duration {
    (last_word_end + END_CARD_TAIL).max(card_start + END_CARD_MIN)
}

/// Persistence port for end cards.
pub trait EndCardRepository: Send + Sync {
    /// The project's card design, if the user ever saved one.
    fn end_card(&self, project: VideoProjectId) -> Result<Option<EndCard>, RepositoryError>;

    /// Makes `card` the project's design, replacing any other.
    fn save_end_card(&self, card: &EndCard) -> Result<(), RepositoryError>;
}

impl<T: EndCardRepository + ?Sized> EndCardRepository for std::sync::Arc<T> {
    fn end_card(&self, project: VideoProjectId) -> Result<Option<EndCard>, RepositoryError> {
        (**self).end_card(project)
    }

    fn save_end_card(&self, card: &EndCard) -> Result<(), RepositoryError> {
        (**self).save_end_card(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AspectRatio;

    fn brief(offer: &str, call: &str, link: &str) -> BriefText {
        BriefText::new(offer, call, link).unwrap()
    }

    fn design(headline: &str, second: &str) -> Result<CardDesign, Vec<CardFieldError>> {
        CardDesign::new(
            headline,
            second,
            true,
            ThumbnailLook::Bold,
            CardTextPlace::Above,
            CardLogoPlace::Corner,
        )
    }

    #[test]
    fn the_lines_keep_to_their_limits_and_the_headline_is_needed() {
        let fits = design(&"a".repeat(40), &"b".repeat(60)).unwrap();
        assert_eq!(fits.headline().chars().count(), 40);
        assert_eq!(
            design(&"a".repeat(41), "").unwrap_err(),
            [CardFieldError::HeadlineTooLong]
        );
        assert_eq!(
            design("Shop", &"b".repeat(61)).unwrap_err(),
            [CardFieldError::SecondLineTooLong]
        );
        assert_eq!(
            design("  \n ", "").unwrap_err(),
            [CardFieldError::HeadlineEmpty]
        );
        let typed = design("  Get   it\nnow ", " pocketloom.com ").unwrap();
        assert_eq!(typed.headline(), "Get it now");
        assert_eq!(typed.second_line(), "pocketloom.com");
        // Ă is one character, two bytes.
        assert!(design(&"Ă".repeat(40), "").is_ok());
    }

    #[test]
    fn a_card_starts_from_the_briefs_call_to_action_link_and_offer() {
        let started = CardDesign::start(&brief(
            "20% off the first month",
            "Order yours today at the link below while stock lasts",
            "https://pocketloom.com/",
        ));
        assert_eq!(started.headline(), "Order yours today at the link below");
        assert!(started.headline().chars().count() <= MAX_CARD_HEADLINE);
        assert_eq!(started.second_line(), "pocketloom.com");
        assert!(started.offer);

        let plain = CardDesign::start(&brief("", "Follow @loom", ""));
        assert_eq!(plain.headline(), "Follow @loom");
        assert_eq!(plain.second_line(), "");
        assert!(!plain.offer);

        // A long link is shortened to one that still works, never cut
        // mid-way.
        let long = |link: &str| {
            CardDesign::start(&brief("", "Buy", link))
                .second_line()
                .to_owned()
        };
        assert_eq!(
            long(
                "https://pocketloom.com/collections/winter-sale-2026?utm_source=tiktok&utm_medium=ad"
            ),
            "pocketloom.com/collections/winter-sale-2026"
        );
        assert_eq!(
            long("https://pocketloom.com/collections/winter/sale/2026/notebooks/refills/a5-dotted"),
            "pocketloom.com/collections/winter/sale/2026/notebooks"
        );
        assert_eq!(long(&format!("https://{}.com/a", "x".repeat(70))), "");
    }

    #[test]
    fn the_offer_line_only_ever_shows_the_briefs_offer() {
        let with = brief("Free shipping", "Buy", "");
        let card = design("Buy now", "").unwrap();
        assert_eq!(card.offer_line(&with), Some("Free shipping"));
        assert_eq!(card.text(&with).offer.as_deref(), Some("Free shipping"));

        let off = CardDesign {
            offer: false,
            ..card.clone()
        };
        assert_eq!(off.offer_line(&with), None);
        // On, with no offer in the brief: no line.
        assert_eq!(card.offer_line(&brief("", "Buy", "")), None);
        // An offer too long for the card is left out whole, never cut.
        let long = brief(&"Free shipping on every order ".repeat(3), "Buy", "");
        assert!(!CardDesign::offer_fits(&long));
        assert_eq!(card.offer_line(&long), None);
        assert!(!CardDesign::start(&long).offer);
    }

    #[test]
    fn a_change_to_what_the_card_shows_changes_its_fingerprint() {
        let text = brief("Free shipping", "Buy", "");
        let card = design("Buy now", "loom.com").unwrap();
        let first = card_fingerprint(&card, &text, None);
        assert_eq!(first, card_fingerprint(&card, &text, None));
        let retyped = design("Buy it now", "loom.com").unwrap();
        assert_ne!(first, card_fingerprint(&retyped, &text, None));
        assert_ne!(
            first,
            card_fingerprint(&card, &brief("Two for one", "Buy", ""), None)
        );
        assert_ne!(first, card_fingerprint(&card, &text, Some(b"logo")));
        let moved = CardDesign {
            text_place: CardTextPlace::Below,
            ..card.clone()
        };
        assert_ne!(first, card_fingerprint(&moved, &text, None));
    }

    fn inside(area: ShotArea, room: ShotArea) -> bool {
        area.x >= room.x
            && area.y >= room.y
            && area.right() <= room.right()
            && area.bottom() <= room.bottom()
            && area.width > 0
            && area.height > 0
    }

    fn apart(a: ShotArea, b: ShotArea) -> bool {
        a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y
    }

    #[test]
    fn every_part_of_a_card_keeps_clear_of_the_networks_and_of_each_other() {
        for aspect in FrameShape::ALL {
            let room = card_room(aspect);
            let (width, height) = shot_frame(aspect);
            if aspect == FrameShape::Vertical {
                // The bottom fifth and the right edge stay clear.
                assert!(room.bottom() <= height * 4 / 5);
                assert!(room.right() <= width * 7 / 8);
            }
            for text_place in CardTextPlace::ALL {
                for logo_place in CardLogoPlace::ALL {
                    for image in [true, false] {
                        for logo in [true, false] {
                            let layout =
                                CardLayout::of(aspect, text_place, logo_place, image, logo);
                            let mut parts = vec![layout.text];
                            parts.extend(layout.product);
                            parts.extend(layout.logo);
                            assert_eq!(layout.product.is_some(), image);
                            assert_eq!(layout.logo.is_some(), logo);
                            for (i, part) in parts.iter().enumerate() {
                                assert!(inside(*part, room), "{aspect:?} {part:?}");
                                for other in &parts[i + 1..] {
                                    assert!(apart(*part, *other), "{part:?} {other:?}");
                                }
                            }
                        }
                    }
                }
            }
        }
        // Text above the image is set at the bottom of its box, next to
        // the image; below it, at the top.
        let above = CardLayout::of(
            AspectRatio::Vertical,
            CardTextPlace::Above,
            CardLogoPlace::Corner,
            true,
            false,
        );
        assert_eq!(above.align, CardTextAlign::Bottom);
        assert!(above.text.bottom() <= above.product.unwrap().y);
        let text_only = CardLayout::of(
            AspectRatio::Vertical,
            CardTextPlace::Below,
            CardLogoPlace::Corner,
            false,
            false,
        );
        assert_eq!(text_only.align, CardTextAlign::Middle);
    }

    #[test]
    fn the_card_stays_a_while_after_the_last_word_and_never_too_briefly() {
        let s = Duration::from_secs_f64;
        // A call to action of 2 s: 1.5 s after the last word.
        assert_eq!(end_card_end(s(10.0), s(12.0)), s(13.5));
        // Of 0.6 s: the card has its 2.5 s, 1.9 s after the last word.
        assert_eq!(end_card_end(s(10.0), s(10.6)), s(12.5));
    }
}
