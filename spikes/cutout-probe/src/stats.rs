//! What a mask says about itself, with no photo and no ground truth: the
//! numbers the spike tests as a confidence signal (can Bardo tell a good
//! cutout from a bad one and fall back to the card layout on its own?).

use serde::Serialize;

/// Alpha at or above this is foreground.
pub const IN: f32 = 0.5;
/// Between these an alpha is neither clearly in nor clearly out.
pub const UNSURE: (f32, f32) = (0.1, 0.9);
/// Foreground pieces smaller than this share of the picture are specks,
/// not pieces.
pub const SPECK: f32 = 0.0005;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MaskStats {
    /// The model's smallest and largest output, before any stretch. A
    /// model that found nothing has a low `raw_max`, which the stretch
    /// then hides.
    pub raw_min: f32,
    pub raw_max: f32,
    /// Share of the picture that is foreground.
    pub area: f32,
    /// Share of the picture that is unsure.
    pub unsure: f32,
    /// `unsure` over `area`: how much of the cutout the model did not
    /// decide. A crisp object has a thin unsure rim; glass, a shadow taken
    /// for the object or a half-found object have wide unsure regions.
    pub unsure_to_area: f32,
    /// Share of the picture's outermost ring of pixels that is foreground:
    /// an object cut by the frame, or the background taken for the object.
    pub border: f32,
    /// Foreground pieces (8-connected) that are not specks.
    pub pieces: usize,
    /// The largest piece's share of the foreground.
    pub largest_piece: f32,
    /// Mean alpha inside the foreground: 1.0 for a solid body.
    pub body: f32,
}

/// The statistics of a `width` × `height` alpha in 0..=1, with the raw
/// range the model gave.
pub fn mask_stats(alpha: &[f32], width: usize, height: usize, raw: (f32, f32)) -> MaskStats {
    assert_eq!(alpha.len(), width * height);
    let n = alpha.len() as f32;
    let inside = |i: usize| alpha[i] >= IN;
    let fg = (0..alpha.len()).filter(|&i| inside(i)).count();
    let unsure = alpha
        .iter()
        .filter(|&&a| a > UNSURE.0 && a < UNSURE.1)
        .count();
    let body = if fg == 0 {
        0.0
    } else {
        alpha.iter().filter(|&&a| a >= IN).sum::<f32>() / fg as f32
    };

    let mut ring = 0usize;
    let mut ring_fg = 0usize;
    for y in 0..height {
        for x in 0..width {
            if y == 0 || x == 0 || y + 1 == height || x + 1 == width {
                ring += 1;
                ring_fg += usize::from(inside(y * width + x));
            }
        }
    }

    let sizes = piece_sizes(alpha, width, height);
    let speck = (SPECK * n).ceil() as usize;
    let pieces = sizes.iter().filter(|&&s| s >= speck).count();
    let largest = sizes.iter().copied().max().unwrap_or(0);

    MaskStats {
        raw_min: raw.0,
        raw_max: raw.1,
        area: fg as f32 / n,
        unsure: unsure as f32 / n,
        unsure_to_area: if fg == 0 {
            0.0
        } else {
            unsure as f32 / fg as f32
        },
        border: if ring == 0 {
            0.0
        } else {
            ring_fg as f32 / ring as f32
        },
        pieces,
        largest_piece: if fg == 0 {
            0.0
        } else {
            largest as f32 / fg as f32
        },
        body,
    }
}

/// The sizes of the 8-connected foreground pieces.
fn piece_sizes(alpha: &[f32], width: usize, height: usize) -> Vec<usize> {
    let mut seen = vec![false; alpha.len()];
    let mut sizes = Vec::new();
    let mut stack = Vec::new();
    for start in 0..alpha.len() {
        if seen[start] || alpha[start] < IN {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut size = 0usize;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = ((i % width) as isize, (i / width) as isize);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                        continue;
                    }
                    let j = ny as usize * width + nx as usize;
                    if !seen[j] && alpha[j] >= IN {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        sizes.push(size);
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(width: usize, height: usize) -> Vec<f32> {
        vec![0.0; width * height]
    }

    fn fill(a: &mut [f32], width: usize, (x0, y0, x1, y1): (usize, usize, usize, usize), v: f32) {
        for y in y0..y1 {
            for x in x0..x1 {
                a[y * width + x] = v;
            }
        }
    }

    #[test]
    fn a_solid_object_in_the_middle() {
        let (w, h) = (100, 100);
        let mut a = canvas(w, h);
        fill(&mut a, w, (25, 25, 75, 75), 1.0);
        let s = mask_stats(&a, w, h, (0.0, 1.0));
        assert!((s.area - 0.25).abs() < 1e-6);
        assert_eq!(s.unsure, 0.0);
        assert_eq!(s.border, 0.0);
        assert_eq!(s.pieces, 1);
        assert_eq!(s.largest_piece, 1.0);
        assert_eq!(s.body, 1.0);
    }

    #[test]
    fn an_empty_mask_divides_nothing_by_zero() {
        let s = mask_stats(&canvas(10, 10), 10, 10, (0.0, 0.02));
        assert_eq!(s.area, 0.0);
        assert_eq!(s.unsure_to_area, 0.0);
        assert_eq!(s.largest_piece, 0.0);
        assert_eq!(s.pieces, 0);
        assert_eq!(s.raw_max, 0.02);
    }

    #[test]
    fn an_undecided_region_counts_as_unsure_and_thins_the_body() {
        let (w, h) = (100, 100);
        let mut a = canvas(w, h);
        fill(&mut a, w, (20, 20, 60, 60), 1.0);
        fill(&mut a, w, (60, 20, 80, 60), 0.6);
        let s = mask_stats(&a, w, h, (0.0, 1.0));
        assert!((s.area - 0.24).abs() < 1e-6);
        assert!((s.unsure - 0.08).abs() < 1e-6);
        assert!((s.unsure_to_area - 1.0 / 3.0).abs() < 1e-6);
        assert!(s.body < 0.9);
    }

    #[test]
    fn an_object_cut_by_the_frame_touches_the_border() {
        let (w, h) = (100, 100);
        let mut a = canvas(w, h);
        fill(&mut a, w, (0, 40, 50, 60), 1.0);
        let s = mask_stats(&a, w, h, (0.0, 1.0));
        // 20 of the 396 ring pixels.
        assert!((s.border - 20.0 / 396.0).abs() < 1e-6);
    }

    #[test]
    fn pieces_join_across_a_diagonal_and_specks_do_not_count() {
        let (w, h) = (100, 100);
        let mut a = canvas(w, h);
        fill(&mut a, w, (10, 10, 30, 30), 1.0);
        // Touches the first only at a corner: the same piece.
        fill(&mut a, w, (30, 30, 50, 50), 1.0);
        // Apart: a second piece.
        fill(&mut a, w, (70, 70, 90, 90), 1.0);
        // Four pixels: under 0.05 % of 10,000 rounds up to 5, a speck.
        fill(&mut a, w, (0, 98, 2, 100), 1.0);
        let s = mask_stats(&a, w, h, (0.0, 1.0));
        assert_eq!(s.pieces, 2);
        assert!((s.largest_piece - 800.0 / 1204.0).abs() < 1e-6);
    }
}
