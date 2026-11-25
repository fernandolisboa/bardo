//! Where a product's image goes in a composed product shot (#278,
//! ADR-0040; layouts #285, ADR-0041). The image is shown whole (as a card
//! with rounded corners, standing cut out on the backdrop, or as the
//! screen of a plain device outline), scaled to fit the room the frame
//! leaves it and never stretched or cut. That room is what stays on screen whatever
//! the cut does with the picture: inside what the strongest zoom shows,
//! and above the band captions use, at every moment of the zoom.

use crate::{
    AspectRatio, BackdropDirection, CAPTION_BAND, DeviceKind, MotionStrength, ProductLayout,
    ShotColor, ShotPlacement,
};

/// The short side of a composed picture, in pixels: 1080 × 1920 for a
/// vertical video and 1920 × 1080 for a wide one.
pub const SHOT_SHORT_SIDE: u32 = 1080;

/// The space kept free around the card, as a share of the frame's short
/// side.
const MARGIN: f64 = 0.03;

/// The roundness of the card's corners, as a share of the frame's short
/// side.
const CORNER: f64 = 0.03;

/// How far a cut-out product's shadow falls below it, and how soft it
/// is, as shares of the frame's short side. The room is kept for it, so
/// the shadow stays above the captions too.
pub const SHADOW_DROP: f64 = 0.018;
pub const SHADOW_BLUR: f64 = 0.02;

/// A phone's outline around its screen, as shares of the screen's width:
/// the bezel on every side and the outline's corner.
const PHONE_BEZEL: f64 = 0.045;
const PHONE_CORNER: f64 = 0.14;

/// A browser window's outline around its screen, as shares of the
/// screen's width: its sides and bottom, its bar on top, and its corner.
const BROWSER_SIDE: f64 = 0.012;
const BROWSER_BAR: f64 = 0.06;
const BROWSER_CORNER: f64 = 0.02;

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

/// The plain outline a screen is set in (#285): a phone or a browser
/// window, drawn by Bardo, with no maker's marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceOutline {
    pub kind: DeviceKind,
    /// The whole outline, the screen inside it.
    pub area: ShotArea,
    /// The roundness of the outline's corners.
    pub radius: u32,
    /// A browser's bar above the screen, in pixels; none for a phone.
    pub bar: u32,
}

/// How a product shot is laid out: the frame's size, where the image goes
/// and how round its corners are, and what is drawn around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShotLayout {
    pub width: u32,
    pub height: u32,
    /// Where the image itself goes: its pixels, only scaled.
    pub card: ShotArea,
    /// The roundness of the image's corners: none for a cut-out photo.
    pub radius: u32,
    /// The outline around a screen.
    pub device: Option<DeviceOutline>,
    /// A cut-out photo casts a soft shadow on the backdrop.
    pub shadow: bool,
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
    /// frame of `aspect`, set as `layout` and placed at `placement`: as
    /// large as its room allows with what is drawn around it, in the
    /// picture's own shape, in the middle of the room's height.
    pub fn of(
        aspect: AspectRatio,
        picture: (u32, u32),
        layout: ProductLayout,
        placement: ShotPlacement,
    ) -> ShotLayout {
        let (width, height) = shot_frame(aspect);
        let mut room = shot_room(aspect);
        if layout == ProductLayout::Cutout {
            let kept = ((SHADOW_DROP + SHADOW_BLUR) * f64::from(SHOT_SHORT_SIDE)).ceil() as u32;
            room.height = room.height.saturating_sub(kept).max(1);
        }
        let (picture_width, picture_height) = (picture.0.max(1), picture.1.max(1));
        let (w, h) = (f64::from(picture_width), f64::from(picture_height));
        let device = (layout == ProductLayout::Device).then(|| DeviceKind::of(picture));
        // What is drawn around the picture, in the picture's own pixels:
        // left and right, top, bottom.
        let (side, top, bottom) = match device {
            Some(DeviceKind::Phone) => (PHONE_BEZEL * w, PHONE_BEZEL * w, PHONE_BEZEL * w),
            Some(DeviceKind::Browser) => (
                BROWSER_SIDE * w,
                (BROWSER_SIDE + BROWSER_BAR) * w,
                BROWSER_SIDE * w,
            ),
            None => (0.0, 0.0, 0.0),
        };
        let (outer_w, outer_h) = (w + 2.0 * side, h + top + bottom);
        let scale = (f64::from(room.width) / outer_w).min(f64::from(room.height) / outer_h);
        let outer_width = ((outer_w * scale).round() as u32).clamp(1, room.width);
        let outer_height = ((outer_h * scale).round() as u32).clamp(1, room.height);
        let outer_x = match placement {
            ShotPlacement::Left => room.x,
            ShotPlacement::Center => room.x + (room.width - outer_width) / 2,
            ShotPlacement::Right => room.right() - outer_width,
        };
        let outer_y = room.y + (room.height - outer_height) / 2;
        let inset_x = (side * scale).round() as u32;
        let inset_top = (top * scale).round() as u32;
        let inset_bottom = (bottom * scale).round() as u32;
        let card_width = if device.is_some() {
            outer_width.saturating_sub(2 * inset_x).max(1)
        } else {
            outer_width
        };
        let card_height = if device.is_some() {
            outer_height.saturating_sub(inset_top + inset_bottom).max(1)
        } else {
            outer_height
        };
        let card = ShotArea {
            x: outer_x + inset_x,
            y: outer_y + inset_top,
            width: card_width,
            height: card_height,
        };
        let corner = ((CORNER * f64::from(SHOT_SHORT_SIDE)).round() as u32)
            .min(card_width.min(card_height) / 8);
        let (radius, device) = match device {
            None if layout == ProductLayout::Cutout => (0, None),
            None => (corner, None),
            Some(kind) => {
                let outer = ShotArea {
                    x: outer_x,
                    y: outer_y,
                    width: outer_width,
                    height: outer_height,
                };
                let (outline_corner, screen_corner) = match kind {
                    DeviceKind::Phone => {
                        let outline = (PHONE_CORNER * outer_w * scale).round() as u32;
                        (outline, outline.saturating_sub(inset_x))
                    }
                    DeviceKind::Browser => ((BROWSER_CORNER * outer_w * scale).round() as u32, 0),
                };
                let bar = match kind {
                    DeviceKind::Phone => 0,
                    DeviceKind::Browser => inset_top.saturating_sub(inset_x),
                };
                (
                    screen_corner.min(card_width.min(card_height) / 4),
                    Some(DeviceOutline {
                        kind,
                        area: outer,
                        radius: outline_corner.min(outer_width.min(outer_height) / 4),
                        bar,
                    }),
                )
            }
        };
        ShotLayout {
            width,
            height,
            card,
            radius,
            device,
            shadow: layout == ProductLayout::Cutout,
        }
    }

    /// What the product takes of the frame: its outline, or the image.
    pub fn product(&self) -> ShotArea {
        self.device.map_or(self.card, |device| device.area)
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

/// What is painted behind the product, as the composer gets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropPaint<'a> {
    /// A tone of the image's own color.
    Auto,
    /// A color, or a gradient between two.
    Plain {
        from: ShotColor,
        to: ShotColor,
        direction: BackdropDirection,
    },
    /// A drawn backdrop: the file's bytes (PNG, JPEG or WebP). It covers
    /// the frame, cut to its shape when it has another.
    Picture(&'a [u8]),
}

/// How to compose one product shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShotRecipe<'a> {
    pub aspect: AspectRatio,
    pub layout: ProductLayout,
    pub placement: ShotPlacement,
    pub backdrop: BackdropPaint<'a>,
}

/// A composed product shot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedPicture {
    /// The picture, as a PNG file's bytes.
    pub png: Vec<u8>,
    /// Tones taken from the product's image, for the user to build a
    /// backdrop from.
    pub tones: Vec<ShotColor>,
}

/// Composes product shots (#278, #285): the product's own image, scaled
/// and set as its layout says ([`ShotLayout`]) over its backdrop. Pixels
/// in, pixels out, on the computer: no model sees the image and nothing
/// is paid for.
pub trait ShotComposer: Send + Sync {
    /// The picture of a product shot of `image` (the file's bytes: PNG,
    /// JPEG or WebP) as `recipe` says. A cut-out layout of an image with
    /// no see-through pixel is a card.
    fn compose(&self, image: &[u8], recipe: &ShotRecipe<'_>) -> Result<ComposedPicture, ShotError>;
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

    /// Every layout of every shape of picture, at every place.
    fn every_layout(picture: (u32, u32), mut check: impl FnMut(AspectRatio, ShotLayout)) {
        for aspect in AspectRatio::ALL {
            for layout in ProductLayout::ALL {
                for placement in ShotPlacement::ALL {
                    check(aspect, ShotLayout::of(aspect, picture, layout, placement));
                }
            }
        }
    }

    const PICTURES: [(u32, u32); 6] = [
        (1200, 1200),
        (3000, 1000),
        (1000, 3000),
        (601, 899),
        (1080, 2340),
        (1920, 1080),
    ];

    #[test]
    fn the_image_keeps_its_shape_and_fills_its_room_one_way() {
        for picture in PICTURES {
            every_layout(picture, |aspect, layout| {
                let room = shot_room(aspect);
                let card = layout.card;
                let product = layout.product();
                let shape = f64::from(picture.0) / f64::from(picture.1);
                let shown = f64::from(card.width) / f64::from(card.height);
                assert!(
                    (shape - shown).abs() / shape < 0.01,
                    "{picture:?} in {aspect}: {layout:?}"
                );
                assert!(
                    product.width == room.width || product.height >= room.height - 60,
                    "{picture:?} in {aspect}: {layout:?}"
                );
                assert!(product.x >= room.x && product.right() <= room.right());
                assert!(product.y >= room.y && product.bottom() <= room.bottom());
                assert!(card.x >= product.x && card.right() <= product.right());
                assert!(card.y >= product.y && card.bottom() <= product.bottom());
                assert!(layout.radius * 4 <= card.width.min(card.height));
            });
        }
    }

    #[test]
    fn each_layout_draws_what_it_says_around_the_image() {
        let aspect = AspectRatio::Vertical;
        let center = ShotPlacement::Center;
        let card = ShotLayout::of(aspect, (1000, 1000), ProductLayout::Card, center);
        assert!(card.radius > 0 && card.device.is_none() && !card.shadow);
        let cutout = ShotLayout::of(aspect, (1000, 1000), ProductLayout::Cutout, center);
        assert!(cutout.radius == 0 && cutout.device.is_none() && cutout.shadow);
        // Room is kept below a cut-out product for its shadow.
        let drop = ((SHADOW_DROP + SHADOW_BLUR) * f64::from(SHOT_SHORT_SIDE)) as u32;
        assert!(cutout.card.bottom() + drop <= shot_room(aspect).bottom());
        let phone = ShotLayout::of(aspect, (1080, 2340), ProductLayout::Device, center);
        let outline = phone.device.unwrap();
        assert_eq!((outline.kind, outline.bar), (DeviceKind::Phone, 0));
        assert!(outline.area.width > phone.card.width && phone.radius > 0);
        let browser = ShotLayout::of(aspect, (1920, 1080), ProductLayout::Device, center);
        let outline = browser.device.unwrap();
        assert_eq!(outline.kind, DeviceKind::Browser);
        assert!(outline.bar > 0 && browser.card.y > outline.area.y + outline.bar);
        assert_eq!(browser.radius, 0);
    }

    #[test]
    fn a_product_sits_left_in_the_middle_or_right_of_its_room() {
        let aspect = AspectRatio::Landscape;
        let room = shot_room(aspect);
        let at = |placement| {
            ShotLayout::of(aspect, (1000, 1000), ProductLayout::Card, placement).product()
        };
        assert_eq!(at(ShotPlacement::Left).x, room.x);
        assert_eq!(at(ShotPlacement::Right).right(), room.right());
        let middle = at(ShotPlacement::Center);
        assert!(middle.x > room.x && middle.right() < room.right());
        assert_eq!(at(ShotPlacement::Left).y, middle.y);
    }

    #[test]
    fn the_card_stays_on_screen_and_above_the_captions_under_the_strongest_zoom() {
        for aspect in AspectRatio::ALL {
            let (width, height) = shot_frame(aspect);
            // The widest and the tallest pictures a product may have, in
            // every layout and place.
            let layouts = [(3000, 1000), (1000, 3000), (2000, 2000)]
                .into_iter()
                .flat_map(|picture| {
                    ProductLayout::ALL.into_iter().flat_map(move |layout| {
                        ShotPlacement::ALL.into_iter().map(move |placement| {
                            ShotLayout::of(aspect, picture, layout, placement)
                        })
                    })
                });
            for layout in layouts {
                let card = layout.product();
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
        let card = |picture| {
            ShotLayout::of(
                AspectRatio::Vertical,
                picture,
                ProductLayout::Card,
                ShotPlacement::Center,
            )
        };
        let small = card((600, 600));
        assert!(small.card.width > 660);
        assert!(small.enlarges(600));
        let large = card((2000, 2000));
        assert!(!large.enlarges(2000));
        // A little larger than its pixels does not show.
        let close = card((760, 760));
        assert!(close.card.width > 760 && !close.enlarges(760));
    }
}
