//! Where a product's image goes in a composed product shot (#278,
//! ADR-0040). The image is shown whole, as a card with rounded corners
//! over a plain backdrop, scaled to fit the room the frame leaves it and
//! never stretched or cut. That room is what stays on screen whatever
//! the cut does with the picture: inside what the strongest zoom shows,
//! and above the band captions use, at every moment of the zoom.

use crate::{AspectRatio, CAPTION_BAND, MotionStrength};

/// The short side of a composed picture, in pixels: 1080 × 1920 for a
/// vertical video and 1920 × 1080 for a wide one.
pub const SHOT_SHORT_SIDE: u32 = 1080;

/// The space kept free around the card, as a share of the frame's short
/// side.
const MARGIN: f64 = 0.03;

/// The roundness of the card's corners, as a share of the frame's short
/// side.
const CORNER: f64 = 0.03;

/// How much larger than its own pixels an image may be shown before it
/// counts as too small for the frame.
const ENLARGED_PAST: f64 = 1.1;

/// A rectangle of a frame, in pixels from its top left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShotArea {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ShotArea {
    pub fn right(self) -> u32 {
        self.x + self.width
    }

    pub fn bottom(self) -> u32 {
        self.y + self.height
    }
}

/// How a product shot is laid out: the frame's size, where the image's
/// card goes and how round its corners are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShotLayout {
    pub width: u32,
    pub height: u32,
    pub card: ShotArea,
    pub radius: u32,
}

/// The size of a composed picture of `aspect`.
pub fn shot_frame(aspect: AspectRatio) -> (u32, u32) {
    let long = SHOT_SHORT_SIDE * 16 / 9;
    match aspect {
        AspectRatio::Vertical => (SHOT_SHORT_SIDE, long),
        AspectRatio::Landscape => (long, SHOT_SHORT_SIDE),
    }
}

/// The part of a frame of `aspect` a product's image may use: what the
/// strongest zoom still shows, less a margin, and above the caption band
/// from the zoom's first moment to its last.
pub fn shot_room(aspect: AspectRatio) -> ShotArea {
    let (width, height) = shot_frame(aspect);
    let zoom = MotionStrength::ALL
        .into_iter()
        .map(MotionStrength::zoom)
        .fold(1.0, f64::max);
    // A zoom keeps its view in the middle: at its closest it leaves this
    // share of the picture out on every side.
    let edge = (1.0 - 1.0 / zoom) / 2.0;
    let margin = (MARGIN * f64::from(SHOT_SHORT_SIDE)).round();
    let left = (edge * f64::from(width)).ceil() + margin;
    let top = (edge * f64::from(height)).ceil() + margin;
    // The band is a share of the screen. Zoomed in, a line of the picture
    // sits lower on the screen than it does in the whole picture.
    let clear = 1.0 - CAPTION_BAND;
    let above_band = clear.min(clear / zoom + edge);
    let bottom = (above_band * f64::from(height)).floor() - margin;
    ShotArea {
        x: left as u32,
        y: top as u32,
        width: (f64::from(width) - 2.0 * left) as u32,
        height: (bottom - top) as u32,
    }
}

impl ShotLayout {
    /// The layout of a picture of `picture` pixels (width, height) in a
    /// frame of `aspect`: as large as its room allows, in the picture's
    /// own shape, in the middle of the room.
    pub fn of(aspect: AspectRatio, picture: (u32, u32)) -> ShotLayout {
        let (width, height) = shot_frame(aspect);
        let room = shot_room(aspect);
        let (picture_width, picture_height) = (picture.0.max(1), picture.1.max(1));
        let scale = (f64::from(room.width) / f64::from(picture_width))
            .min(f64::from(room.height) / f64::from(picture_height));
        let card_width = ((f64::from(picture_width) * scale).round() as u32).clamp(1, room.width);
        let card_height =
            ((f64::from(picture_height) * scale).round() as u32).clamp(1, room.height);
        let radius = ((CORNER * f64::from(SHOT_SHORT_SIDE)).round() as u32)
            .min(card_width.min(card_height) / 8);
        ShotLayout {
            width,
            height,
            card: ShotArea {
                x: room.x + (room.width - card_width) / 2,
                y: room.y + (room.height - card_height) / 2,
                width: card_width,
                height: card_height,
            },
            radius,
        }
    }

    /// Whether a picture `picture_width` pixels wide is too small for
    /// the frame: shown this large it would look soft.
    pub fn enlarges(&self, picture_width: u32) -> bool {
        f64::from(self.card.width) > f64::from(picture_width.max(1)) * ENLARGED_PAST
    }
}

/// Why a product shot could not be composed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShotError {
    /// The product's image is no picture that can be read.
    #[error("the product image cannot be read: {0}")]
    Unreadable(String),
    /// The composed picture could not be written.
    #[error("the picture could not be written: {0}")]
    Failed(String),
}

/// Composes product shots (#278): the product's own image, scaled and
/// set as a card over a plain backdrop in a color taken from it
/// ([`ShotLayout`]). Pixels in, pixels out, on the computer: no model
/// sees the image and nothing is paid for.
pub trait ShotComposer: Send + Sync {
    /// The picture of a product shot of `image` (the file's bytes: PNG,
    /// JPEG or WebP) in a frame of `aspect`, as a PNG file's bytes.
    fn compose(&self, image: &[u8], aspect: AspectRatio) -> Result<Vec<u8>, ShotError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Motion, MotionKind};

    /// Where a point of the picture (as shares of it) is on the screen
    /// under `motion`, `progress` of the way through.
    fn on_screen(motion: Motion, progress: f64, x: f64, y: f64) -> (f64, f64) {
        let view = motion.view_at(progress);
        let room = 1.0 - 1.0 / view.zoom;
        (
            (x - view.x * room) * view.zoom,
            (y - view.y * room) * view.zoom,
        )
    }

    #[test]
    fn a_composed_picture_has_the_frames_shape() {
        assert_eq!(shot_frame(AspectRatio::Vertical), (1080, 1920));
        assert_eq!(shot_frame(AspectRatio::Landscape), (1920, 1080));
    }

    #[test]
    fn the_image_keeps_its_shape_and_fills_its_room_one_way() {
        for aspect in AspectRatio::ALL {
            let room = shot_room(aspect);
            for picture in [(1200, 1200), (3000, 1000), (1000, 3000), (601, 899)] {
                let layout = ShotLayout::of(aspect, picture);
                let card = layout.card;
                let shape = f64::from(picture.0) / f64::from(picture.1);
                let shown = f64::from(card.width) / f64::from(card.height);
                assert!(
                    (shape - shown).abs() / shape < 0.01,
                    "{picture:?} in {aspect}: {card:?}"
                );
                assert!(card.width == room.width || card.height == room.height);
                assert!(card.x >= room.x && card.right() <= room.right());
                assert!(card.y >= room.y && card.bottom() <= room.bottom());
                assert!(layout.radius * 8 <= card.width.min(card.height));
            }
        }
    }

    #[test]
    fn the_card_stays_on_screen_and_above_the_captions_under_the_strongest_zoom() {
        for aspect in AspectRatio::ALL {
            let (width, height) = shot_frame(aspect);
            // The widest and the tallest pictures a product may have.
            for picture in [(3000, 1000), (1000, 3000), (2000, 2000)] {
                let card = ShotLayout::of(aspect, picture).card;
                let corners = [
                    (card.x, card.y),
                    (card.right(), card.y),
                    (card.x, card.bottom()),
                    (card.right(), card.bottom()),
                ];
                for strength in MotionStrength::ALL {
                    for kind in [MotionKind::ZoomIn, MotionKind::ZoomOut] {
                        let motion = Motion::new(kind, strength);
                        for step in 0..=20 {
                            for (x, y) in corners {
                                let (x, y) = on_screen(
                                    motion,
                                    f64::from(step) / 20.0,
                                    f64::from(x) / f64::from(width),
                                    f64::from(y) / f64::from(height),
                                );
                                assert!((0.0..=1.0).contains(&x), "{aspect} {motion:?}: x {x}");
                                assert!(
                                    (0.0..=1.0 - CAPTION_BAND).contains(&y),
                                    "{aspect} {motion:?}: y {y}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn an_image_shown_larger_than_its_pixels_is_too_small() {
        let small = ShotLayout::of(AspectRatio::Vertical, (600, 600));
        assert!(small.card.width > 660);
        assert!(small.enlarges(600));
        let large = ShotLayout::of(AspectRatio::Vertical, (2000, 2000));
        assert!(!large.enlarges(2000));
        // A little larger than its pixels does not show.
        let close = ShotLayout::of(AspectRatio::Vertical, (760, 760));
        assert!(close.card.width > 760 && !close.enlarges(760));
    }
}
