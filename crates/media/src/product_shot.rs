//! Product shots (#278, ADR-0040; layouts and backdrops #285, ADR-0041):
//! a scene's picture composed from one of the product's own images, with
//! no model and no ffmpeg. The image is read, scaled to the place
//! `bardo_domain::ShotLayout` gives it and set over its backdrop: as a
//! card with rounded corners, standing cut out with a soft shadow, or as
//! the screen of a plain device outline. Its pixels are changed by the
//! scaling and by nothing else.

use std::io::Cursor;

use bardo_domain::{
    BackdropDirection, BackdropPaint, ComposedPicture, DeviceKind, DeviceOutline, ProductLayout,
    SHADOW_BLUR, SHADOW_DROP, SHOT_SHORT_SIDE, ShotArea, ShotColor, ShotComposer, ShotError,
    ShotLayout, ShotRecipe,
};
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::{
    DynamicImage, ExtendedColorType, GrayImage, ImageBuffer, ImageDecoder, ImageEncoder,
    ImageReader, Limits, Luma, Rgb, RgbImage, Rgba, RgbaImage,
};

/// Composes product shots on the processor.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProductShots;

/// How the image is scaled to its place.
const SCALING: FilterType = FilterType::Lanczos3;

/// The widest and tallest picture read, in pixels: past the largest a
/// product image may be, whatever its file says.
const MAX_SIDE: u32 = 8_192;

/// The most memory reading one picture may take.
const MAX_READ: u64 = 512 * 1024 * 1024;

/// How light an image is, on average, from which its backdrop is a deep
/// tone of its color; under it, a pale one.
const LIGHT_FROM: f32 = 0.45;

/// A deep tone keeps this much of the image's average color.
const DEEP: f32 = 0.28;

/// A pale tone goes this far from the image's average color to white.
const PALE: f32 = 0.82;

/// How dark a cut-out product's shadow is where it is darkest.
const SHADOW: f32 = 0.42;

/// A device outline: its body, the thin rim around it, a browser's bar,
/// the bar's three dots and its address field. Plain greys, no marks.
const OUTLINE: [u8; 3] = [26, 26, 30];
const RIM: [u8; 3] = [72, 72, 80];
const BAR: [u8; 3] = [44, 44, 50];
const DOT: [u8; 3] = [98, 98, 106];
const FIELD: [u8; 3] = [62, 62, 70];

/// The most tones offered from one image.
const MOST_TONES: usize = 8;

impl ShotComposer for ProductShots {
    fn compose(&self, image: &[u8], recipe: &ShotRecipe<'_>) -> Result<ComposedPicture, ShotError> {
        let picture = read(image)?;
        // A photo with nothing see-through has nothing to stand cut out.
        let layout = match recipe.layout {
            ProductLayout::Cutout if picture.pixels().all(|pixel| pixel[3] == u8::MAX) => {
                ProductLayout::Card
            }
            layout => layout,
        };
        let layout = ShotLayout::of(
            recipe.aspect,
            (picture.width(), picture.height()),
            layout,
            recipe.placement,
        );
        let card = scaled(picture, layout.card.width, layout.card.height);
        let tones = tones_of(&card);
        let mut frame = match recipe.backdrop {
            BackdropPaint::Auto => {
                RgbImage::from_pixel(layout.width, layout.height, Rgb(backdrop_of(&card)))
            }
            BackdropPaint::Plain {
                from,
                to,
                direction,
            } => gradient(layout.width, layout.height, from, to, direction),
            BackdropPaint::Picture(bytes) => covering(bytes, layout.width, layout.height)?,
        };
        if layout.shadow {
            cast_shadow(&mut frame, &card, layout.card);
        }
        if let Some(device) = layout.device {
            draw_outline(&mut frame, device);
        }
        for (x, y, pixel) in card.enumerate_pixels() {
            let cover = corner_cover(x, y, card.width(), card.height(), layout.radius)
                * (f32::from(pixel[3]) / 255.0);
            if cover <= 0.0 {
                continue;
            }
            let shown = frame.get_pixel_mut(layout.card.x + x, layout.card.y + y);
            for channel in 0..3 {
                shown[channel] = if cover >= 1.0 {
                    pixel[channel]
                } else {
                    let under = f32::from(shown[channel]);
                    (under + (f32::from(pixel[channel]) - under) * cover).round() as u8
                };
            }
        }
        let mut png = Vec::new();
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, PngFilter::Adaptive)
            .write_image(
                frame.as_raw(),
                layout.width,
                layout.height,
                ExtendedColorType::Rgb8,
            )
            .map_err(|error| ShotError::Failed(error.to_string()))?;
        Ok(ComposedPicture { png, tones })
    }
}

/// The picture in a product image's file, upright as a camera's note of
/// its turn says.
fn read(bytes: &[u8]) -> Result<RgbaImage, ShotError> {
    let unreadable = |error: image::ImageError| ShotError::Unreadable(error.to_string());
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_READ);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| ShotError::Unreadable(error.to_string()))?;
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(unreadable)?;
    let orientation = decoder.orientation().map_err(unreadable)?;
    let mut picture = DynamicImage::from_decoder(decoder).map_err(unreadable)?;
    picture.apply_orientation(orientation);
    let picture = picture.into_rgba8();
    if picture.width() == 0 || picture.height() == 0 {
        return Err(ShotError::Unreadable("the picture is empty".into()));
    }
    Ok(picture)
}

/// `picture` at `width` × `height`. One with see-through parts is scaled
/// with each color weighed by how solid it is, so no edge takes the color
/// hidden under a clear pixel; a solid one is scaled as it is. The weighed
/// colors are kept in 16 bits, exact for 8-bit colors and half the memory
/// of floating point for a large picture.
fn scaled(picture: RgbaImage, width: u32, height: u32) -> RgbaImage {
    if picture.pixels().all(|pixel| pixel[3] == u8::MAX) {
        return imageops::resize(&picture, width, height, SCALING);
    }
    // Each color times its solidity (at most 255 × 255), and the
    // solidity on the same 16-bit scale.
    let weighed: ImageBuffer<Rgba<u16>, Vec<u16>> =
        ImageBuffer::from_fn(picture.width(), picture.height(), |x, y| {
            let [r, g, b, a] = picture.get_pixel(x, y).0;
            let solid = u16::from(a);
            Rgba([
                u16::from(r) * solid,
                u16::from(g) * solid,
                u16::from(b) * solid,
                solid * 257,
            ])
        });
    drop(picture);
    let scaled = imageops::resize(&weighed, width, height, SCALING);
    RgbaImage::from_fn(width, height, |x, y| {
        let [r, g, b, a] = scaled.get_pixel(x, y).0;
        if a == 0 {
            return Rgba([0, 0, 0, 0]);
        }
        let solid = f32::from(a);
        let color = |weighed: u16| {
            (f32::from(weighed) * 257.0 / solid)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Rgba([
            color(r),
            color(g),
            color(b),
            (solid / 257.0).round().clamp(0.0, 255.0) as u8,
        ])
    })
}

/// The backdrop's color for `card`: a tone of the image's own average
/// color, deep under a light image and pale under a dark one, so the card
/// stands out from it. An image with nothing solid gets a neutral dark.
fn backdrop_of(card: &RgbaImage) -> [u8; 3] {
    let mut sum = [0.0f64; 3];
    let mut solid = 0.0f64;
    for pixel in card.pixels() {
        let weight = f64::from(pixel[3]) / 255.0;
        for channel in 0..3 {
            sum[channel] += f64::from(pixel[channel]) / 255.0 * weight;
        }
        solid += weight;
    }
    if solid <= 0.0 {
        return [24, 24, 28];
    }
    let average = sum.map(|channel| (channel / solid) as f32);
    let light = 0.2126 * average[0] + 0.7152 * average[1] + 0.0722 * average[2];
    average.map(|channel| {
        let toned = if light >= LIGHT_FROM {
            channel * DEEP
        } else {
            channel + (1.0 - channel) * PALE
        };
        (toned * 255.0).round().clamp(0.0, 255.0) as u8
    })
}

/// Tones of `card` to build a backdrop from: the tone its own backdrop
/// takes, then a deep and a pale tone of its average color and of each of
/// its main colors. Always the same for the same image.
fn tones_of(card: &RgbaImage) -> Vec<ShotColor> {
    // Solid pixels counted in boxes of similar colors.
    let mut boxes = vec![(0.0f64, [0.0f64; 3]); 512];
    for pixel in card.pixels() {
        let weight = f64::from(pixel[3]) / 255.0;
        if weight <= 0.0 {
            continue;
        }
        let at = (usize::from(pixel[0] >> 5) << 6)
            | (usize::from(pixel[1] >> 5) << 3)
            | usize::from(pixel[2] >> 5);
        boxes[at].0 += weight;
        for channel in 0..3 {
            boxes[at].1[channel] += f64::from(pixel[channel]) * weight;
        }
    }
    let mut main: Vec<(f64, [f32; 3])> = boxes
        .into_iter()
        .filter(|(weight, _)| *weight > 0.0)
        .map(|(weight, sum)| (weight, sum.map(|channel| (channel / weight / 255.0) as f32)))
        .collect();
    main.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut colors: Vec<[f32; 3]> = Vec::new();
    for (_, color) in main {
        let apart = colors.iter().all(|kept| {
            kept.iter()
                .zip(color)
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                > 0.35
        });
        if apart {
            colors.push(color);
        }
        if colors.len() == 3 {
            break;
        }
    }
    let mut tones = vec![ShotColor(backdrop_of(card))];
    for color in colors {
        for tone in [deep(color), pale(color)] {
            if !tones.contains(&tone) && tones.len() < MOST_TONES {
                tones.push(tone);
            }
        }
    }
    tones
}

fn deep(color: [f32; 3]) -> ShotColor {
    ShotColor(color.map(|channel| (channel * DEEP * 255.0).round().clamp(0.0, 255.0) as u8))
}

fn pale(color: [f32; 3]) -> ShotColor {
    ShotColor(color.map(|channel| {
        ((channel + (1.0 - channel) * PALE) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    }))
}

/// A `width` × `height` backdrop going from `from` to `to` as
/// `direction` says, with a faint even grain so no band shows.
fn gradient(
    width: u32,
    height: u32,
    from: ShotColor,
    to: ShotColor,
    direction: BackdropDirection,
) -> RgbImage {
    if from == to {
        return RgbImage::from_pixel(width, height, Rgb(from.0));
    }
    const GRAIN: [[f32; 4]; 4] = [
        [0.0, 8.0, 2.0, 10.0],
        [12.0, 4.0, 14.0, 6.0],
        [3.0, 11.0, 1.0, 9.0],
        [15.0, 7.0, 13.0, 5.0],
    ];
    let (w, h) = ((width.max(2) - 1) as f32, (height.max(2) - 1) as f32);
    RgbImage::from_fn(width, height, |x, y| {
        let along = match direction {
            BackdropDirection::Down => y as f32 / h,
            BackdropDirection::Across => x as f32 / w,
            BackdropDirection::Diagonal => (x as f32 / w + y as f32 / h) / 2.0,
        };
        let grain = GRAIN[(y % 4) as usize][(x % 4) as usize] / 16.0 - 0.47;
        Rgb(std::array::from_fn(|channel| {
            let (a, b) = (f32::from(from.0[channel]), f32::from(to.0[channel]));
            (a + (b - a) * along + grain).round().clamp(0.0, 255.0) as u8
        }))
    })
}

/// A drawn backdrop covering a `width` × `height` frame: scaled to fill
/// it and cut to its shape, kept in the middle.
fn covering(bytes: &[u8], width: u32, height: u32) -> Result<RgbImage, ShotError> {
    let backdrop = read(bytes).map_err(|error| match error {
        ShotError::Unreadable(why) => ShotError::Unreadable(format!("the backdrop: {why}")),
        other => other,
    })?;
    Ok(DynamicImage::ImageRgba8(backdrop)
        .resize_to_fill(width, height, FilterType::Triangle)
        .into_rgb8())
}

/// Darkens `frame` under a cut-out product set at `at`: its own outline,
/// softened and dropped a little below it.
fn cast_shadow(frame: &mut RgbImage, card: &RgbaImage, at: ShotArea) {
    // Made at a quarter of the size: a soft shadow needs no detail, and
    // the blur is sixteen times cheaper.
    const QUARTER: u32 = 4;
    let short = f64::from(SHOT_SHORT_SIDE);
    let blur = (SHADOW_BLUR * short / f64::from(QUARTER)) as f32;
    let drop = (SHADOW_DROP * short).round() as i64;
    let pad = (blur * 3.0).ceil() as u32;
    let (small_width, small_height) = (
        (card.width() / QUARTER).max(1),
        (card.height() / QUARTER).max(1),
    );
    let alpha = GrayImage::from_fn(card.width(), card.height(), |x, y| {
        Luma([card.get_pixel(x, y)[3]])
    });
    let small = imageops::resize(&alpha, small_width, small_height, FilterType::Triangle);
    let mut padded = GrayImage::new(small_width + 2 * pad, small_height + 2 * pad);
    imageops::replace(&mut padded, &small, i64::from(pad), i64::from(pad));
    let soft = imageops::fast_blur(&padded, blur);
    let full = imageops::resize(
        &soft,
        soft.width() * QUARTER,
        soft.height() * QUARTER,
        FilterType::Triangle,
    );
    // The shadow's top left corner on the frame.
    let left = i64::from(at.x) - i64::from(pad * QUARTER);
    let top = i64::from(at.y) - i64::from(pad * QUARTER) + drop;
    for (x, y, shade) in full.enumerate_pixels() {
        let (fx, fy) = (left + i64::from(x), top + i64::from(y));
        if shade[0] == 0
            || fx < 0
            || fy < 0
            || fx >= i64::from(frame.width())
            || fy >= i64::from(frame.height())
        {
            continue;
        }
        let keep = 1.0 - SHADOW * f32::from(shade[0]) / 255.0;
        let pixel = frame.get_pixel_mut(fx as u32, fy as u32);
        for channel in 0..3 {
            pixel[channel] = (f32::from(pixel[channel]) * keep).round() as u8;
        }
    }
}

/// Draws a plain device outline: a body with a thin rim and, for a
/// browser window, its bar with three dots and an address field. The
/// screen is drawn over it.
fn draw_outline(frame: &mut RgbImage, device: DeviceOutline) {
    let area = device.area;
    let rim = (area.width.min(area.height) / 220).max(2);
    fill_rounded(frame, area, device.radius, RIM);
    let body = ShotArea {
        x: area.x + rim,
        y: area.y + rim,
        width: area.width.saturating_sub(2 * rim).max(1),
        height: area.height.saturating_sub(2 * rim).max(1),
    };
    fill_rounded(frame, body, device.radius.saturating_sub(rim), OUTLINE);
    if device.kind != DeviceKind::Browser || device.bar == 0 {
        return;
    }
    let side = rim * 2;
    let bar = ShotArea {
        x: body.x + side,
        y: body.y + side,
        width: body.width.saturating_sub(2 * side).max(1),
        height: device.bar.saturating_sub(side).max(1),
    };
    fill_rounded(frame, bar, bar.height / 4, BAR);
    let dot = (bar.height / 7).max(2);
    let middle = bar.y + bar.height / 2;
    for n in 0..3 {
        let x = bar.x + bar.height / 2 + n * dot * 3;
        fill_rounded(
            frame,
            ShotArea {
                x: x.saturating_sub(dot),
                y: middle.saturating_sub(dot),
                width: dot * 2,
                height: dot * 2,
            },
            dot,
            DOT,
        );
    }
    let field_left = bar.x + bar.height / 2 + dot * 9 + bar.height / 3;
    let field_right = bar.right().saturating_sub(bar.height / 2);
    if field_right > field_left {
        let field_height = (bar.height / 2).max(2);
        fill_rounded(
            frame,
            ShotArea {
                x: field_left,
                y: middle.saturating_sub(field_height / 2),
                width: field_right - field_left,
                height: field_height,
            },
            field_height / 2,
            FIELD,
        );
    }
}

/// Fills `area` of `frame` with `color`, its corners rounded by `radius`
/// and smoothed.
fn fill_rounded(frame: &mut RgbImage, area: ShotArea, radius: u32, color: [u8; 3]) {
    let radius = radius.min(area.width.min(area.height) / 2);
    for y in 0..area.height {
        for x in 0..area.width {
            let (fx, fy) = (area.x + x, area.y + y);
            if fx >= frame.width() || fy >= frame.height() {
                continue;
            }
            let cover = corner_cover(x, y, area.width, area.height, radius);
            if cover <= 0.0 {
                continue;
            }
            let pixel = frame.get_pixel_mut(fx, fy);
            for channel in 0..3 {
                let under = f32::from(pixel[channel]);
                pixel[channel] =
                    (under + (f32::from(color[channel]) - under) * cover).round() as u8;
            }
        }
    }
}

/// How much of the pixel at (`x`, `y`) of a `width` × `height` card with
/// corners of `radius` the card covers: all of it, but for the pixels the
/// rounded corners cut, in part or whole.
fn corner_cover(x: u32, y: u32, width: u32, height: u32, radius: u32) -> f32 {
    let radius = radius as f32;
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    let (width, height) = (width as f32, height as f32);
    let past_x = (radius - x).max(x - (width - radius));
    let past_y = (radius - y).max(y - (height - radius));
    if past_x <= 0.0 || past_y <= 0.0 {
        return 1.0;
    }
    (radius - past_x.hypot(past_y) + 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use bardo_domain::{AspectRatio, CAPTION_BAND, CaptionStyle, ShotPlacement, shot_frame};
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::webp::WebPEncoder;
    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::ffmpeg::caption_look;

    /// A picture with no two neighbors alike: bands, a slope and noise.
    fn pattern(width: u32, height: u32) -> RgbImage {
        let mut seed = 0x2545_F491u32;
        RgbImage::from_fn(width, height, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            Rgb([
                ((x * 255) / width) as u8,
                (((y / 40) % 2) * 180 + (seed % 60)) as u8,
                ((x + y) % 251) as u8,
            ])
        })
    }

    fn png(picture: &RgbImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                picture.as_raw(),
                picture.width(),
                picture.height(),
                ExtendedColorType::Rgb8,
            )
            .unwrap();
        bytes
    }

    fn png_rgba(picture: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                picture.as_raw(),
                picture.width(),
                picture.height(),
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        bytes
    }

    fn recipe(aspect: AspectRatio, layout: ProductLayout) -> ShotRecipe<'static> {
        ShotRecipe {
            aspect,
            layout,
            placement: ShotPlacement::Center,
            backdrop: BackdropPaint::Auto,
        }
    }

    fn composed_as(bytes: &[u8], recipe: &ShotRecipe<'_>) -> RgbImage {
        let shot = ProductShots.compose(bytes, recipe).unwrap();
        assert!(shot.png.starts_with(b"\x89PNG\r\n\x1a\n"));
        image::load_from_memory(&shot.png).unwrap().into_rgb8()
    }

    fn composed(bytes: &[u8], aspect: AspectRatio) -> RgbImage {
        composed_as(bytes, &recipe(aspect, ProductLayout::Card))
    }

    fn card_of(aspect: AspectRatio, picture: (u32, u32)) -> ShotLayout {
        ShotLayout::of(aspect, picture, ProductLayout::Card, ShotPlacement::Center)
    }

    /// Whether (`x`, `y`) of a card is in one of the squares its rounded
    /// corners are cut from.
    fn in_a_corner(x: u32, y: u32, layout: &ShotLayout) -> bool {
        let (width, height, radius) = (layout.card.width, layout.card.height, layout.radius);
        (x < radius || x >= width - radius) && (y < radius || y >= height - radius)
    }

    #[test]
    fn a_shot_holds_the_images_pixels_scaled_and_a_plain_backdrop() {
        for aspect in AspectRatio::ALL {
            for (width, height) in [(1200, 800), (900, 1500)] {
                let source = pattern(width, height);
                let shot = composed(&png(&source), aspect);
                let layout = card_of(aspect, (width, height));
                assert_eq!(shot.dimensions(), shot_frame(aspect));
                assert_eq!(shot.dimensions(), (layout.width, layout.height));

                // Inside the card: the source, scaled and nothing else.
                let card = layout.card;
                let scaled = imageops::resize(&source, card.width, card.height, SCALING);
                let mut compared = 0u32;
                for (x, y, pixel) in scaled.enumerate_pixels() {
                    if in_a_corner(x, y, &layout) {
                        continue;
                    }
                    assert_eq!(
                        shot.get_pixel(card.x + x, card.y + y),
                        pixel,
                        "{aspect} {width}x{height}: ({x}, {y})"
                    );
                    compared += 1;
                }
                assert!(compared > card.width * card.height * 9 / 10);

                // Outside it: one color.
                let backdrop = *shot.get_pixel(0, 0);
                for (x, y, pixel) in shot.enumerate_pixels() {
                    let inside =
                        (card.x..card.right()).contains(&x) && (card.y..card.bottom()).contains(&y);
                    if !inside {
                        assert_eq!(*pixel, backdrop, "{aspect}: ({x}, {y})");
                    }
                }
                // The corners are rounded: the card's very corner is
                // backdrop, and its edge's middle is the image.
                for (x, y) in [
                    (card.x, card.y),
                    (card.right() - 1, card.y),
                    (card.x, card.bottom() - 1),
                    (card.right() - 1, card.bottom() - 1),
                ] {
                    assert_eq!(*shot.get_pixel(x, y), backdrop);
                }
                assert!(layout.radius > 0);
            }
        }
    }

    #[test]
    fn jpeg_and_webp_images_are_composed_too() {
        let source = pattern(800, 800);
        let from_png = composed(&png(&source), AspectRatio::Vertical);

        let mut webp = Vec::new();
        WebPEncoder::new_lossless(&mut webp)
            .write_image(source.as_raw(), 800, 800, ExtendedColorType::Rgb8)
            .unwrap();
        assert!(webp.starts_with(b"RIFF"));
        assert_eq!(composed(&webp, AspectRatio::Vertical), from_png);

        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 95)
            .write_image(source.as_raw(), 800, 800, ExtendedColorType::Rgb8)
            .unwrap();
        let from_jpeg = composed(&jpeg, AspectRatio::Vertical);
        assert_eq!(from_jpeg.dimensions(), from_png.dimensions());
        // The same picture, but for what JPEG itself loses.
        let layout = card_of(AspectRatio::Vertical, (800, 800));
        let middle = (
            layout.card.x + layout.card.width / 2,
            layout.card.y + layout.card.height / 2,
        );
        let (a, b) = (
            from_jpeg.get_pixel(middle.0, middle.1),
            from_png.get_pixel(middle.0, middle.1),
        );
        assert!((0..3).all(|c| a[c].abs_diff(b[c]) < 40), "{a:?} {b:?}");
    }

    #[test]
    fn a_cut_out_image_lets_the_backdrop_through() {
        // Solid red on the left half, clear on the right.
        let source = RgbaImage::from_fn(1000, 1000, |x, _| match x < 500 {
            true => Rgba([200, 30, 30, 255]),
            false => Rgba([0, 255, 0, 0]),
        });
        let shot = composed(&png_rgba(&source), AspectRatio::Landscape);
        let card = card_of(AspectRatio::Landscape, (1000, 1000)).card;
        let backdrop = *shot.get_pixel(0, 0);
        let row = card.y + card.height / 2;
        assert_eq!(
            *shot.get_pixel(card.x + card.width / 4, row),
            Rgb([200, 30, 30])
        );
        assert_eq!(*shot.get_pixel(card.x + card.width * 3 / 4, row), backdrop);
        // The edge between them goes from red to the backdrop, never
        // through the green hidden under the clear half.
        for x in card.x..card.right() {
            let pixel = shot.get_pixel(x, row);
            assert!(
                pixel[1] <= backdrop[1].max(30) + 2,
                "green shows at {x}: {pixel:?}"
            );
        }
    }

    #[test]
    fn the_backdrop_is_a_tone_of_the_image_that_sets_it_apart() {
        let light = |pixel: Rgb<u8>| {
            0.2126 * f32::from(pixel[0])
                + 0.7152 * f32::from(pixel[1])
                + 0.0722 * f32::from(pixel[2])
        };
        // A light image, bluish: a deep blue backdrop.
        let pale = RgbImage::from_pixel(700, 700, Rgb([200, 220, 250]));
        let backdrop = *composed(&png(&pale), AspectRatio::Vertical).get_pixel(0, 0);
        assert!(light(backdrop) < 80.0, "{backdrop:?}");
        assert!(backdrop[2] > backdrop[0], "it keeps the image's color");
        // A dark image: a pale backdrop.
        let dark = RgbImage::from_pixel(700, 700, Rgb([20, 24, 40]));
        let backdrop = *composed(&png(&dark), AspectRatio::Vertical).get_pixel(0, 0);
        assert!(light(backdrop) > 200.0, "{backdrop:?}");
        // The same image always gets the same picture.
        let card = recipe(AspectRatio::Vertical, ProductLayout::Card);
        assert_eq!(
            ProductShots.compose(&png(&dark), &card).unwrap(),
            ProductShots.compose(&png(&dark), &card).unwrap()
        );
    }

    #[test]
    fn a_file_that_is_no_picture_is_refused() {
        for bytes in [&b""[..], b"not an image", b"\x89PNG\r\n\x1a\ncut short"] {
            assert!(matches!(
                ProductShots.compose(bytes, &recipe(AspectRatio::Vertical, ProductLayout::Card)),
                Err(ShotError::Unreadable(_))
            ));
        }
    }

    /// `source` with a see-through frame around a solid middle.
    fn cut_out(width: u32, height: u32) -> RgbaImage {
        let solid = pattern(width, height);
        RgbaImage::from_fn(width, height, |x, y| {
            let inside = x > width / 5 && x < width * 4 / 5 && y > height / 6 && y < height * 5 / 6;
            let [r, g, b] = solid.get_pixel(x, y).0;
            Rgba([r, g, b, if inside { 255 } else { 0 }])
        })
    }

    #[test]
    fn every_layout_shows_the_images_pixels_only_scaled() {
        let pictures: [(ProductLayout, RgbaImage); 4] = [
            (ProductLayout::Card, opaque(1200, 800)),
            (ProductLayout::Cutout, cut_out(900, 1200)),
            (ProductLayout::Device, opaque(1080, 2340)),
            (ProductLayout::Device, opaque(1920, 1080)),
        ];
        for aspect in AspectRatio::ALL {
            for (layout, source) in &pictures {
                for placement in ShotPlacement::ALL {
                    let recipe = ShotRecipe {
                        placement,
                        ..recipe(aspect, *layout)
                    };
                    let shot = composed_as(&png_rgba(source), &recipe);
                    let at = ShotLayout::of(
                        aspect,
                        (source.width(), source.height()),
                        *layout,
                        placement,
                    );
                    let card = at.card;
                    let scaled = scaled(source.clone(), card.width, card.height);
                    let mut compared = 0u32;
                    for (x, y, pixel) in scaled.enumerate_pixels() {
                        if pixel[3] != 255 || (at.radius > 0 && in_a_corner(x, y, &at)) {
                            continue;
                        }
                        assert_eq!(
                            shot.get_pixel(card.x + x, card.y + y).0,
                            [pixel[0], pixel[1], pixel[2]],
                            "{aspect} {layout:?} {placement:?}: ({x}, {y})"
                        );
                        compared += 1;
                    }
                    assert!(compared > card.width * card.height / 3, "{layout:?}");
                }
            }
        }
    }

    fn opaque(width: u32, height: u32) -> RgbaImage {
        let solid = pattern(width, height);
        RgbaImage::from_fn(width, height, |x, y| {
            let [r, g, b] = solid.get_pixel(x, y).0;
            Rgba([r, g, b, 255])
        })
    }

    #[test]
    fn a_cut_out_photo_stands_on_the_backdrop_with_a_soft_shadow() {
        let aspect = AspectRatio::Vertical;
        let source = cut_out(1000, 1000);
        let shot = composed_as(&png_rgba(&source), &recipe(aspect, ProductLayout::Cutout));
        let at = ShotLayout::of(
            aspect,
            (1000, 1000),
            ProductLayout::Cutout,
            ShotPlacement::Center,
        );
        let backdrop = *shot.get_pixel(0, 0);
        // Just under the solid part: shade. Far from it: the backdrop.
        let below = at.card.y + at.card.height * 5 / 6 + 12;
        let under = *shot.get_pixel(at.card.x + at.card.width / 2, below);
        assert!(
            (0..3).all(|c| under[c] < backdrop[c] || backdrop[c] == 0),
            "{under:?} under {backdrop:?}"
        );
        assert_eq!(*shot.get_pixel(5, shot.height() - 5), backdrop);
        // The see-through corner of the photo is the backdrop: no card.
        assert_eq!(*shot.get_pixel(at.card.x + 2, at.card.y + 2), backdrop);

        // A photo with nothing see-through is shown as a card.
        let solid = opaque(1000, 1000);
        assert_eq!(
            composed_as(&png_rgba(&solid), &recipe(aspect, ProductLayout::Cutout)),
            composed_as(&png_rgba(&solid), &recipe(aspect, ProductLayout::Card))
        );
    }

    #[test]
    fn a_screen_sits_in_a_plain_outline() {
        for (picture, kind) in [
            ((1080, 2340), DeviceKind::Phone),
            ((1920, 1080), DeviceKind::Browser),
        ] {
            let source = opaque(picture.0, picture.1);
            let aspect = AspectRatio::Vertical;
            let shot = composed_as(&png_rgba(&source), &recipe(aspect, ProductLayout::Device));
            let at = ShotLayout::of(
                aspect,
                picture,
                ProductLayout::Device,
                ShotPlacement::Center,
            );
            let outline = at.device.unwrap();
            assert_eq!(outline.kind, kind);
            // Between the outline's edge and the screen: the body's grey.
            let side = outline.area.x + (at.card.x - outline.area.x) / 2 + 1;
            let row = at.card.y + at.card.height / 2;
            assert_eq!(shot.get_pixel(side, row).0, OUTLINE, "{kind:?}");
            // Outside the outline: the backdrop.
            assert_eq!(*shot.get_pixel(0, row), *shot.get_pixel(0, 0));
        }
    }

    #[test]
    fn a_plain_backdrop_runs_from_its_first_color_to_its_second() {
        let source = opaque(600, 600);
        let (from, to) = (ShotColor([200, 40, 40]), ShotColor([20, 40, 200]));
        for direction in BackdropDirection::ALL {
            let recipe = ShotRecipe {
                backdrop: BackdropPaint::Plain {
                    from,
                    to,
                    direction,
                },
                ..recipe(AspectRatio::Landscape, ProductLayout::Card)
            };
            let shot = composed_as(&png_rgba(&source), &recipe);
            let (w, h) = shot.dimensions();
            let near = |a: Rgb<u8>, b: ShotColor| (0..3).all(|c| a[c].abs_diff(b.0[c]) <= 2);
            assert!(near(*shot.get_pixel(0, 0), from), "{direction:?}");
            let end = match direction {
                BackdropDirection::Down => (0, h - 1),
                BackdropDirection::Across => (w - 1, 0),
                BackdropDirection::Diagonal => (w - 1, h - 1),
            };
            assert!(
                near(*shot.get_pixel(end.0, end.1), to),
                "{direction:?}: {:?}",
                shot.get_pixel(end.0, end.1)
            );
        }
        // One color is one color.
        let recipe = ShotRecipe {
            backdrop: BackdropPaint::Plain {
                from,
                to: from,
                direction: BackdropDirection::Down,
            },
            ..recipe(AspectRatio::Vertical, ProductLayout::Card)
        };
        let shot = composed_as(&png_rgba(&source), &recipe);
        assert_eq!(shot.get_pixel(0, 0).0, from.0);
        assert_eq!(shot.get_pixel(0, shot.height() - 1).0, from.0);
    }

    #[test]
    fn a_drawn_backdrop_covers_the_frame_whatever_its_shape() {
        let backdrop = png(&RgbImage::from_pixel(1920, 1080, Rgb([90, 140, 60])));
        let source = opaque(600, 600);
        for aspect in AspectRatio::ALL {
            let recipe = ShotRecipe {
                backdrop: BackdropPaint::Picture(&backdrop),
                ..recipe(aspect, ProductLayout::Card)
            };
            let shot = composed_as(&png_rgba(&source), &recipe);
            assert_eq!(shot.dimensions(), shot_frame(aspect));
            let (w, h) = shot.dimensions();
            for (x, y) in [(0, 0), (w - 1, h - 1), (w / 2, h - 1)] {
                assert_eq!(shot.get_pixel(x, y).0, [90, 140, 60], "{aspect}");
            }
        }
        let recipe = ShotRecipe {
            backdrop: BackdropPaint::Picture(b"nothing"),
            ..recipe(AspectRatio::Vertical, ProductLayout::Card)
        };
        assert!(matches!(
            ProductShots.compose(&png_rgba(&source), &recipe),
            Err(ShotError::Unreadable(why)) if why.contains("backdrop")
        ));
    }

    #[test]
    fn tones_are_taken_from_the_image_and_start_with_its_own_backdrop() {
        let source = RgbImage::from_fn(800, 800, |x, _| match x < 400 {
            true => Rgb([220, 30, 30]),
            false => Rgb([30, 30, 220]),
        });
        let shot = ProductShots
            .compose(
                &png(&source),
                &recipe(AspectRatio::Vertical, ProductLayout::Card),
            )
            .unwrap();
        let picture = image::load_from_memory(&shot.png).unwrap().into_rgb8();
        assert_eq!(shot.tones[0].0, picture.get_pixel(0, 0).0);
        assert!(shot.tones.len() >= 5 && shot.tones.len() <= MOST_TONES);
        // A deep red and a deep blue are among them.
        let [red, blue] = [0, 2].map(|at| {
            shot.tones
                .iter()
                .any(|tone| u32::from(tone.0[at]) > 3 * u32::from(tone.0[2 - at]).max(1))
        });
        assert!(red && blue, "{:?}", shot.tones);
    }

    /// The band composed pictures keep clear ([`CAPTION_BAND`]) is what
    /// the caption styles take: from each style's lift, two lines of its
    /// letters in a vertical frame and one in a wide one, with its outline
    /// or band around them.
    #[test]
    fn every_caption_style_fits_the_band_a_shot_keeps_clear() {
        for aspect in AspectRatio::ALL {
            let (width, height) = shot_frame(aspect);
            let short = width.min(height) as f32;
            let lines = match aspect {
                AspectRatio::Vertical => 2.0,
                AspectRatio::Landscape => 1.0,
            };
            for style in CaptionStyle::ALL {
                let look = caption_look(style);
                let around = look
                    .outline
                    .map(|(_, width)| width)
                    .or(look.band.map(|(_, _, padding)| padding))
                    .unwrap_or(0.0);
                let top = look.lift + (lines * look.size + 2.0 * around) * short / height as f32;
                assert!(
                    f64::from(top) <= CAPTION_BAND,
                    "{style:?} in {aspect}: {top}"
                );
            }
        }
    }
}
