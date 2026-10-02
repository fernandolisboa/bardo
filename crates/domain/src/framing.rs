//! Framing (PRD story 67): a video project is cut for one frame shape, 16:9
//! or 9:16 (`crate::AspectRatio`), and each clip of its video track says how
//! its picture fills a frame of another shape: the largest window of the
//! frame's shape, placed where the user puts it, or the whole picture with
//! bars around it. The same assets then fit a long video and a Short.
//!
//! Scene pictures are made 16:9, so in a 16:9 cut every clip fits the frame
//! as it is and the framing only applies to 9:16. A position is kept in
//! steps of its range ([`CROP_STEPS`]), not in pixels, so it holds for the
//! source and for its smaller proxy alike.

use crate::AspectRatio;

/// How many steps a crop position has across its range: 0 puts the window
/// at the left (or top) edge of the picture, `CROP_STEPS` at the right (or
/// bottom).
pub const CROP_STEPS: u16 = 1_000;

/// Where a crop window sits in its picture, across (`x`) and down (`y`).
/// Exact, so a framing compares and saves as it was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CropPosition {
    x: u16,
    y: u16,
}

impl CropPosition {
    /// The middle of the picture.
    pub const CENTER: CropPosition = CropPosition {
        x: CROP_STEPS / 2,
        y: CROP_STEPS / 2,
    };

    /// A position, each axis kept within [`CROP_STEPS`].
    pub fn new(x: u16, y: u16) -> Self {
        CropPosition {
            x: x.min(CROP_STEPS),
            y: y.min(CROP_STEPS),
        }
    }

    pub fn x(self) -> u16 {
        self.x
    }

    pub fn y(self) -> u16 {
        self.y
    }

    /// Each axis as a fraction, from 0.0 to 1.0.
    pub fn fractions(self) -> (f32, f32) {
        let steps = f32::from(CROP_STEPS);
        (f32::from(self.x) / steps, f32::from(self.y) / steps)
    }

    /// This position moved across by `by` steps, within the range.
    pub fn nudged(self, by: i32) -> Self {
        let x = (i32::from(self.x) + by).clamp(0, i32::from(CROP_STEPS));
        CropPosition::new(x as u16, self.y)
    }

    /// The position after dragging the window of `target`'s shape over
    /// `source` by `dx`, `dy` pixels of the source, the window stopping at
    /// the picture's edges. An axis the window fills, or that is not
    /// dragged, keeps its step.
    pub fn dragged(
        self,
        source: PictureSize,
        target: AspectRatio,
        dx: f64,
        dy: f64,
    ) -> CropPosition {
        let window = crop_window(source, target, self);
        let axis = |step: u16, offset: u32, slack: u32, by: f64| -> u16 {
            if slack == 0 || by == 0.0 {
                return step;
            }
            let moved = (f64::from(offset) + by).clamp(0.0, f64::from(slack));
            (moved / f64::from(slack) * f64::from(CROP_STEPS)).round() as u16
        };
        CropPosition::new(
            axis(self.x, window.x, source.width - window.width, dx),
            axis(self.y, window.y, source.height - window.height, dy),
        )
    }
}

impl Default for CropPosition {
    fn default() -> Self {
        CropPosition::CENTER
    }
}

/// How a clip's picture fills a frame of another shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Framing {
    /// The largest window of the frame's shape, at this position.
    Crop(CropPosition),
    /// The whole picture, with bars where it does not reach the frame.
    Fit,
}

impl Framing {
    /// A centered window: what a clip starts with.
    pub const FILL: Framing = Framing::Crop(CropPosition::CENTER);
}

impl Default for Framing {
    fn default() -> Self {
        Framing::FILL
    }
}

impl AspectRatio {
    /// Width and height of the shape, in lowest terms.
    pub fn ratio(self) -> (u32, u32) {
        match self {
            AspectRatio::Vertical => (9, 16),
            AspectRatio::Landscape => (16, 9),
        }
    }
}

/// A picture's size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PictureSize {
    pub width: u32,
    pub height: u32,
}

impl PictureSize {
    pub const fn new(width: u32, height: u32) -> Self {
        PictureSize { width, height }
    }
}

/// A rectangle of a picture, in its pixels from the top left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The window a crop at `position` takes from a `source` picture for a
/// frame of `target`'s shape: as large as the picture allows (its sides
/// even, as 4:2:0 video needs), slid across the room left over by
/// `position`. The renderer's crop (`bardo_media::ffmpeg::Framing::Crop`)
/// places its window the same way from whatever size of the picture it
/// reads, sized to the output's exact shape, so it can differ from this one
/// by a pixel or two.
pub fn crop_window(source: PictureSize, target: AspectRatio, position: CropPosition) -> CropRect {
    let (across, down) = target.ratio();
    let (source_width, source_height) = (u64::from(source.width), u64::from(source.height));
    let (width, height) = if source_width * u64::from(down) >= source_height * u64::from(across) {
        // Wider than the frame: full height.
        (
            source_height * u64::from(across) / u64::from(down),
            source_height,
        )
    } else {
        (
            source_width,
            source_width * u64::from(down) / u64::from(across),
        )
    };
    let even = |side: u64| if side >= 2 { side & !1 } else { side };
    let (width, height) = (even(width) as u32, even(height) as u32);
    let place = |slack: u32, step: u16| -> u32 {
        let steps = u64::from(CROP_STEPS);
        ((u64::from(slack) * u64::from(step) + steps / 2) / steps) as u32
    };
    CropRect {
        x: place(source.width - width, position.x),
        y: place(source.height - height, position.y),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: PictureSize = PictureSize::new(1920, 1080);

    #[test]
    fn a_vertical_window_takes_the_full_height_of_a_landscape_picture() {
        let center = crop_window(HD, AspectRatio::Vertical, CropPosition::CENTER);
        // 1080 × 9 / 16 = 607.5, down to an even 606.
        assert_eq!(
            center,
            CropRect {
                x: 657,
                y: 0,
                width: 606,
                height: 1080
            }
        );
        let left = crop_window(HD, AspectRatio::Vertical, CropPosition::new(0, 500));
        assert_eq!((left.x, left.width), (0, 606));
        let right = crop_window(HD, AspectRatio::Vertical, CropPosition::new(1_000, 500));
        assert_eq!(right.x + right.width, 1920);
        let quarter = crop_window(HD, AspectRatio::Vertical, CropPosition::new(250, 500));
        assert_eq!(quarter.x, 329);
    }

    #[test]
    fn the_vertical_position_does_not_move_a_window_of_full_height() {
        let top = crop_window(HD, AspectRatio::Vertical, CropPosition::new(500, 0));
        let bottom = crop_window(HD, AspectRatio::Vertical, CropPosition::new(500, 1_000));
        assert_eq!(top, bottom);
    }

    #[test]
    fn a_landscape_window_takes_the_full_width_of_a_tall_picture() {
        let tall = PictureSize::new(1080, 1920);
        let window = crop_window(tall, AspectRatio::Landscape, CropPosition::new(0, 1_000));
        assert_eq!(
            window,
            CropRect {
                x: 0,
                y: 1314,
                width: 1080,
                height: 606
            }
        );
    }

    #[test]
    fn a_picture_of_the_frames_shape_is_taken_whole() {
        for position in [CropPosition::new(0, 0), CropPosition::new(1_000, 1_000)] {
            assert_eq!(
                crop_window(HD, AspectRatio::Landscape, position),
                CropRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                }
            );
        }
    }

    #[test]
    fn odd_sizes_give_even_windows_inside_the_picture() {
        // A 16:9 image as image models make it, a little off 16:9.
        let image = PictureSize::new(1376, 768);
        for x in [0, 333, 500, 1_000] {
            let window = crop_window(image, AspectRatio::Vertical, CropPosition::new(x, 500));
            assert_eq!(window.width % 2, 0);
            assert_eq!(window.height, 768);
            assert!(window.x + window.width <= image.width, "{window:?}");
        }
    }

    #[test]
    fn positions_stay_within_their_range() {
        assert_eq!(
            CropPosition::new(4_000, 1_001),
            CropPosition::new(1_000, 1_000)
        );
        assert_eq!(CropPosition::CENTER.nudged(-900), CropPosition::new(0, 500));
        assert_eq!(CropPosition::CENTER.nudged(50), CropPosition::new(550, 500));
        assert_eq!(CropPosition::new(1_000, 0).fractions(), (1.0, 0.0));
    }

    #[test]
    fn dragging_moves_the_window_by_the_pixels_dragged_up_to_the_edges() {
        let source = PictureSize::new(960, 540);
        // Window 302 wide, 658 pixels of room.
        let dragged = CropPosition::CENTER.dragged(source, AspectRatio::Vertical, 164.5, 80.0);
        assert_eq!(dragged, CropPosition::new(750, 500));
        let window = crop_window(source, AspectRatio::Vertical, dragged);
        assert_eq!(window.x, 494);
        let far = CropPosition::CENTER.dragged(source, AspectRatio::Vertical, -5_000.0, 0.0);
        assert_eq!(far, CropPosition::new(0, 500));
        // Nothing to move in a picture of the frame's shape.
        let same = CropPosition::new(200, 700).dragged(source, AspectRatio::Landscape, 90.0, 90.0);
        assert_eq!(same, CropPosition::new(200, 700));
        // A step between two pixels stays where it is when not dragged.
        let still = CropPosition::new(250, 500).dragged(source, AspectRatio::Vertical, 0.0, 3.0);
        assert_eq!(still, CropPosition::new(250, 500));
    }

    #[test]
    fn clips_start_filling_the_frame_from_the_middle() {
        assert_eq!(Framing::default(), Framing::Crop(CropPosition::CENTER));
        assert_eq!(AspectRatio::Vertical.ratio(), (9, 16));
    }
}
