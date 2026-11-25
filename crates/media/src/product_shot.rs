//! Product shots (#278, ADR-0040): a scene's picture composed from one
//! of the product's own images, with no model and no ffmpeg. The image is
//! read, scaled to the card `bardo_domain::ShotLayout` gives it and set,
//! with rounded corners, over a plain backdrop in a color taken from the
//! image itself. Its pixels are changed by the scaling and by nothing
//! else.

use std::io::Cursor;

use bardo_domain::{AspectRatio, ShotComposer, ShotError, ShotLayout};
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::{
    DynamicImage, ExtendedColorType, ImageBuffer, ImageDecoder, ImageEncoder, ImageReader, Limits,
    Rgb, RgbImage, Rgba, RgbaImage,
};

/// Composes product shots on the processor.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProductShots;

/// How the image is scaled to its card.
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

impl ShotComposer for ProductShots {
    fn compose(&self, image: &[u8], aspect: AspectRatio) -> Result<Vec<u8>, ShotError> {
        let picture = read(image)?;
        let layout = ShotLayout::of(aspect, (picture.width(), picture.height()));
        let card = scaled(picture, layout.card.width, layout.card.height);
        let backdrop = backdrop_of(&card);
        let mut frame = RgbImage::from_pixel(layout.width, layout.height, Rgb(backdrop));
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
        Ok(png)
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
    use bardo_domain::{CAPTION_BAND, CaptionStyle, shot_frame};
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

    fn composed(bytes: &[u8], aspect: AspectRatio) -> RgbImage {
        let shot = ProductShots.compose(bytes, aspect).unwrap();
        assert!(shot.starts_with(b"\x89PNG\r\n\x1a\n"));
        image::load_from_memory(&shot).unwrap().into_rgb8()
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
                let layout = ShotLayout::of(aspect, (width, height));
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
        let layout = ShotLayout::of(AspectRatio::Vertical, (800, 800));
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
        let card = ShotLayout::of(AspectRatio::Landscape, (1000, 1000)).card;
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
        assert_eq!(
            ProductShots
                .compose(&png(&dark), AspectRatio::Vertical)
                .unwrap(),
            ProductShots
                .compose(&png(&dark), AspectRatio::Vertical)
                .unwrap()
        );
    }

    #[test]
    fn a_file_that_is_no_picture_is_refused() {
        for bytes in [&b""[..], b"not an image", b"\x89PNG\r\n\x1a\ncut short"] {
            assert!(matches!(
                ProductShots.compose(bytes, AspectRatio::Vertical),
                Err(ShotError::Unreadable(_))
            ));
        }
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
