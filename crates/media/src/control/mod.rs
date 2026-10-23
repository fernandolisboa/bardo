//! Drawing the frames of a control video (#191) on the processor, with
//! tiny-skia: the face covered by its drawn mesh (Mesh mode), and the
//! performer as a colored depth figure over the set (Depth mode). Frames
//! are RGBA, rows top to bottom, no padding, as ffmpeg reads them from a
//! pipe.
//!
//! Every color drawn inside the face oval has more blue than red, so no
//! mix of them, anti-aliased or not, passes for skin: in Mesh mode no
//! frame shows the performer's face.

pub mod edges;

use std::sync::OnceLock;

use bardo_domain::{FaceMesh, RgbFrame};
use tiny_skia::{FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// How much larger than the face's outline the covering is, around its
/// centre, so the cheeks' edge and the jaw stay covered as the head moves.
pub const OVAL_GROWTH: f32 = 0.15;
/// How far above the outline's top the covering reaches, as a share of the
/// face's height: the forehead up to the hairline.
pub const OVAL_RAISE: f32 = 0.30;

/// The colors of the drawn face: RGB.
pub const FACE_FILL: [u8; 3] = [34, 40, 52];
pub const MESH_LINES: [u8; 3] = [150, 190, 235];
pub const LIPS_LINES: [u8; 3] = [170, 80, 235];
pub const EYES_LINES: [u8; 3] = [60, 215, 255];
pub const BROWS_LINES: [u8; 3] = [90, 235, 150];

/// The landmarks of the top of the forehead and the chin.
const FOREHEAD: usize = 10;
const CHIN: usize = 152;

/// Where a person mask starts and stops counting as the performer: a
/// smooth step between these, so the figure's edge is soft.
const MASK_FROM: f32 = 0.35;
const MASK_TO: f32 = 0.65;

/// A frame being drawn: RGBA, every pixel opaque.
pub struct Canvas {
    pixmap: Pixmap,
}

impl Canvas {
    /// A black canvas. `None` for a zero side.
    pub fn new(width: u32, height: u32) -> Option<Canvas> {
        let mut pixmap = Pixmap::new(width, height)?;
        pixmap.fill(tiny_skia::Color::BLACK);
        Some(Canvas { pixmap })
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Copies `frame` in; it must be the canvas's size.
    pub fn put_rgb(&mut self, frame: &RgbFrame) {
        debug_assert_eq!((frame.width, frame.height), (self.width(), self.height()));
        for (pixel, rgb) in self
            .pixmap
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(frame.rgb.as_chunks::<3>().0)
        {
            *pixel = [rgb[0], rgb[1], rgb[2], 255];
        }
    }

    /// The frame as RGBA. Opaque, so premultiplied is the same.
    pub fn rgba(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// The frame as RGB.
    pub fn rgb(&self) -> RgbFrame {
        RgbFrame {
            width: self.width(),
            height: self.height(),
            rgb: self
                .pixmap
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect(),
        }
    }

    /// `mesh`'s landmark `index` in pixels.
    fn point(&self, mesh: &FaceMesh, index: usize) -> (f32, f32) {
        let p = mesh.landmarks.get(index).copied().unwrap_or_default();
        (p[0] * self.width() as f32, p[1] * self.height() as f32)
    }

    /// The face's outline in pixels, in order.
    pub fn face_outline(&self, mesh: &FaceMesh) -> Vec<(f32, f32)> {
        edges::FACE_OVAL
            .iter()
            .map(|edge| self.point(mesh, usize::from(edge[0])))
            .collect()
    }

    /// The outline grown by [`OVAL_GROWTH`] around its centre and raised
    /// toward the hairline by [`OVAL_RAISE`] of the face's height: what
    /// covers the face.
    pub fn face_cover(&self, mesh: &FaceMesh) -> Vec<(f32, f32)> {
        let outline = self.face_outline(mesh);
        let n = outline.len().max(1) as f32;
        let centre = outline
            .iter()
            .fold((0.0, 0.0), |(x, y), p| (x + p.0 / n, y + p.1 / n));
        let (top, chin) = (self.point(mesh, FOREHEAD), self.point(mesh, CHIN));
        let (ux, uy) = (top.0 - chin.0, top.1 - chin.1);
        let height = (ux * ux + uy * uy).sqrt();
        if height < 1e-3 {
            return outline;
        }
        let (ux, uy) = (ux / height, uy / height);
        let above = |p: &(f32, f32)| (p.0 - centre.0) * ux + (p.1 - centre.1) * uy;
        let highest = outline.iter().map(above).fold(0.0_f32, f32::max).max(1e-3);
        outline
            .iter()
            .map(|p| {
                let lift = above(p).max(0.0) / highest * OVAL_RAISE * height;
                (
                    centre.0 + (p.0 - centre.0) * (1.0 + OVAL_GROWTH) + ux * lift,
                    centre.1 + (p.1 - centre.1) * (1.0 + OVAL_GROWTH) + uy * lift,
                )
            })
            .collect()
    }

    /// Mesh mode: fills the face's covering with [`FACE_FILL`]. Draw the
    /// mesh over it with [`Canvas::draw_mesh`].
    pub fn cover_face(&mut self, mesh: &FaceMesh) {
        if let Some(path) = polygon(&self.face_cover(mesh)) {
            self.pixmap.fill_path(
                &path,
                &paint(FACE_FILL),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Draws `mesh`: the tessellation, then the lips, eyes, irises and
    /// brows in their own colors, as thick as the face's height allows.
    pub fn draw_mesh(&mut self, mesh: &FaceMesh) {
        let (top, chin) = (self.point(mesh, FOREHEAD), self.point(mesh, CHIN));
        let height = ((top.0 - chin.0).powi(2) + (top.1 - chin.1).powi(2)).sqrt();
        let width = (height / 320.0).clamp(1.0, 3.0);
        let groups: [LineGroup; 8] = [
            (tessellation(), MESH_LINES, width),
            (edges::LIPS, LIPS_LINES, width * 1.5),
            (edges::LEFT_EYE, EYES_LINES, width * 1.5),
            (edges::RIGHT_EYE, EYES_LINES, width * 1.5),
            (&edges::LEFT_IRIS, EYES_LINES, width * 1.5),
            (&edges::RIGHT_IRIS, EYES_LINES, width * 1.5),
            (edges::LEFT_EYEBROW, BROWS_LINES, width * 1.5),
            (edges::RIGHT_EYEBROW, BROWS_LINES, width * 1.5),
        ];
        for (lines, color, width) in groups {
            let mut builder = PathBuilder::new();
            for edge in lines {
                let (a, b) = (
                    self.point(mesh, usize::from(edge[0])),
                    self.point(mesh, usize::from(edge[1])),
                );
                builder.move_to(a.0, a.1);
                builder.line_to(b.0, b.1);
            }
            if let Some(path) = builder.finish() {
                let stroke = Stroke {
                    width,
                    line_cap: tiny_skia::LineCap::Round,
                    ..Stroke::default()
                };
                self.pixmap
                    .stroke_path(&path, &paint(color), &stroke, Transform::identity(), None);
            }
        }
    }

    /// Depth mode: puts the performer over what the canvas holds (the set)
    /// as a figure colored by `depth` between `far` and `near`, where
    /// `mask` says they are, with a soft edge. Inside `face`'s covering the
    /// figure is opaque whatever the mask says, so the set never shows the
    /// face. `depth` (16 bits, larger is nearer) and `mask` (0 to 255) are
    /// `size`, smaller than the canvas, and stretched over it.
    pub fn depth_figure(
        &mut self,
        depth: &[u16],
        mask: &[u8],
        size: (u32, u32),
        (far, near): (u16, u16),
        face: Option<&FaceMesh>,
    ) {
        let (w, h) = (self.width() as usize, self.height() as usize);
        let (dw, dh) = (size.0 as usize, size.1 as usize);
        if dw == 0 || dh == 0 || depth.len() < dw * dh || mask.len() < dw * dh {
            return;
        }
        let oval = face.and_then(|face| {
            let path = polygon(&self.face_cover(face))?;
            let mut oval = Mask::new(self.width(), self.height())?;
            oval.fill_path(&path, FillRule::Winding, true, Transform::identity());
            Some(oval)
        });
        let columns = samples(w, dw);
        let rows = samples(h, dh);
        let span = f32::from(near.saturating_sub(far)).max(1.0);
        let colors = turbo_table();
        let data = self.pixmap.data_mut();
        for (y, &(y0, y1, fy)) in rows.iter().enumerate() {
            for (x, &(x0, x1, fx)) in columns.iter().enumerate() {
                let at = |plane: &dyn Fn(usize) -> f32| {
                    let top = plane(y0 * dw + x0) * (1.0 - fx) + plane(y0 * dw + x1) * fx;
                    let bottom = plane(y1 * dw + x0) * (1.0 - fx) + plane(y1 * dw + x1) * fx;
                    top * (1.0 - fy) + bottom * fy
                };
                let person = at(&|i| f32::from(mask[i]) / 255.0);
                let mut alpha = smoothstep(MASK_FROM, MASK_TO, person);
                if let Some(oval) = &oval {
                    alpha = alpha.max(f32::from(oval.data()[y * w + x]) / 255.0);
                }
                if alpha <= 0.0 {
                    continue;
                }
                let value = at(&|i| f32::from(depth[i]));
                let share = ((value - f32::from(far)) / span).clamp(0.0, 1.0);
                let color = colors[(share * 255.0).round() as usize];
                let pixel = &mut data[(y * w + x) * 4..][..3];
                for (channel, figure) in pixel.iter_mut().zip(color) {
                    let mixed = f32::from(*channel) * (1.0 - alpha) + f32::from(figure) * alpha;
                    *channel = mixed.round() as u8;
                }
            }
        }
    }
}

/// Edges drawn in one color and width.
type LineGroup = (&'static [[u16; 2]], [u8; 3], f32);

fn paint(color: [u8; 3]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = true;
    paint
}

/// A closed path through `points`.
fn polygon(points: &[(f32, f32)]) -> Option<Path> {
    let (first, rest) = points.split_first()?;
    let mut builder = PathBuilder::new();
    builder.move_to(first.0, first.1);
    for point in rest {
        builder.line_to(point.0, point.1);
    }
    builder.close();
    builder.finish()
}

/// The tessellation's edges, each once (a triangle shares its edges with
/// its neighbours).
fn tessellation() -> &'static [[u16; 2]] {
    static EDGES: OnceLock<Vec<[u16; 2]>> = OnceLock::new();
    EDGES.get_or_init(|| {
        let mut edges: Vec<[u16; 2]> = edges::TRIANGLES
            .iter()
            .flat_map(|t| [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]])
            .map(|[a, b]| [a.min(b), a.max(b)])
            .collect();
        edges.sort_unstable();
        edges.dedup();
        edges
    })
}

/// For each of `out` pixels across, the two cells of `cells` it falls
/// between and how far toward the second, cell centres aligned.
fn samples(out: usize, cells: usize) -> Vec<(usize, usize, f32)> {
    (0..out)
        .map(|i| {
            let f =
                ((i as f32 + 0.5) * cells as f32 / out as f32 - 0.5).clamp(0.0, (cells - 1) as f32);
            let low = f as usize;
            (low, (low + 1).min(cells - 1), f - low as f32)
        })
        .collect()
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Google's Turbo colormap at `t` from 0 (far: dark blue) to 1 (near:
/// dark red), from its polynomial fit.
pub fn turbo(t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let poly = |c: [f32; 6]| {
        let v = c[0] + t * (c[1] + t * (c[2] + t * (c[3] + t * (c[4] + t * c[5]))));
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    [
        poly([
            0.135_721_38,
            4.615_392_6,
            -42.660_323,
            132.131_08,
            -152.942_39,
            59.286_38,
        ]),
        poly([
            0.091_402_61,
            2.194_188_4,
            4.842_966_6,
            -14.185_033,
            4.277_298_6,
            2.829_566,
        ]),
        poly([
            0.106_673_3,
            12.641_946,
            -60.582_05,
            110.362_77,
            -89.903_11,
            27.348_25,
        ]),
    ]
}

fn turbo_table() -> &'static [[u8; 3]; 256] {
    static TABLE: OnceLock<[[u8; 3]; 256]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| turbo(i as f32 / 255.0)))
}

/// Whether a pixel passes for skin by the usual RGB rule (Kovač et al.,
/// daylight): what the Mesh mode must never leave inside the face's
/// outline.
pub fn looks_like_skin(rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb.map(i32::from);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    r > 95 && g > 40 && b > 20 && max - min > 15 && (r - g).abs() > 15 && r > g && r > b
}

/// Whether `point` is inside the closed `polygon`.
pub fn inside(polygon: &[(f32, f32)], point: (f32, f32)) -> bool {
    let mut inside = false;
    let mut j = polygon.len().wrapping_sub(1);
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a.1 > point.1) != (b.1 > point.1)
            && point.0 < (b.0 - a.0) * (point.1 - a.1) / (b.1 - a.1) + a.0
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use bardo_domain::FACE_LANDMARKS;

    use super::*;

    const SKIN: [u8; 3] = [205, 150, 120];

    /// A face in the middle of a `width` × `height` frame: its outline an
    /// ellipse `rx` × `ry` pixels, every other point spread inside it.
    fn face(width: u32, height: u32, rx: f32, ry: f32) -> FaceMesh {
        let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
        let mut landmarks = vec![[0.5, 0.5, 0.0]; FACE_LANDMARKS];
        // Inside points on a small spiral, so lines go everywhere.
        for (index, point) in landmarks.iter_mut().enumerate() {
            let angle = index as f32 * 2.4;
            let reach = (index as f32 / FACE_LANDMARKS as f32).sqrt() * 0.85;
            point[0] = (cx + angle.cos() * rx * reach) / width as f32;
            point[1] = (cy + angle.sin() * ry * reach) / height as f32;
        }
        let oval = edges::FACE_OVAL;
        for (i, edge) in oval.iter().enumerate() {
            // From the top, clockwise on screen.
            let angle =
                -std::f32::consts::FRAC_PI_2 + i as f32 / oval.len() as f32 * std::f32::consts::TAU;
            landmarks[usize::from(edge[0])] = [
                (cx + angle.cos() * rx) / width as f32,
                (cy + angle.sin() * ry) / height as f32,
                0.0,
            ];
        }
        landmarks[FOREHEAD] = [cx / width as f32, (cy - ry) / height as f32, 0.0];
        landmarks[CHIN] = [cx / width as f32, (cy + ry) / height as f32, 0.0];
        FaceMesh {
            landmarks,
            blendshapes: [0.0; 52],
        }
    }

    fn canvas_of(width: u32, height: u32, color: [u8; 3]) -> Canvas {
        let mut canvas = Canvas::new(width, height).unwrap();
        canvas.put_rgb(&RgbFrame {
            width,
            height,
            rgb: color.repeat((width * height) as usize),
        });
        canvas
    }

    fn pixel(canvas: &Canvas, x: u32, y: u32) -> [u8; 3] {
        let at = ((y * canvas.width() + x) * 4) as usize;
        let data = canvas.rgba();
        [data[at], data[at + 1], data[at + 2]]
    }

    #[test]
    fn the_rule_tells_skin_from_the_drawn_face() {
        assert!(looks_like_skin(SKIN));
        assert!(looks_like_skin([230, 180, 160]));
        for color in [FACE_FILL, MESH_LINES, LIPS_LINES, EYES_LINES, BROWS_LINES] {
            assert!(!looks_like_skin(color), "{color:?}");
            assert!(color[2] > color[0], "more blue than red: {color:?}");
        }
    }

    #[test]
    fn the_oval_and_raised_covering_hide_every_skin_pixel_of_the_face() {
        let (width, height) = (320, 240);
        let mesh = face(width, height, 50.0, 70.0);
        let mut canvas = canvas_of(width, height, SKIN);
        canvas.cover_face(&mesh);
        canvas.draw_mesh(&mesh);

        let outline = canvas.face_outline(&mesh);
        let cover = canvas.face_cover(&mesh);
        let mut inside_count = 0;
        for y in 0..height {
            for x in 0..width {
                let at = (x as f32 + 0.5, y as f32 + 0.5);
                if inside(&outline, at) {
                    inside_count += 1;
                    assert!(
                        !looks_like_skin(pixel(&canvas, x, y)),
                        "skin at {x},{y}: {:?}",
                        pixel(&canvas, x, y)
                    );
                }
            }
        }
        assert!(inside_count > 9_000, "{inside_count}");
        // The covering reaches past the outline: wider at the cheeks and
        // well above the top, toward the hairline.
        let top = cover.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        assert!(top < 120.0 - 70.0 * (1.0 + OVAL_GROWTH) - 15.0, "{top}");
        assert!(!looks_like_skin(pixel(&canvas, 160, 52)), "forehead");
        assert!(!looks_like_skin(pixel(&canvas, 160 + 54, 120)), "cheek");
        // Away from the face, the frame is as it was.
        assert_eq!(pixel(&canvas, 10, 10), SKIN);
        assert_eq!(pixel(&canvas, 300, 230), SKIN);
    }

    #[test]
    fn the_mesh_is_drawn_in_its_colors() {
        let (width, height) = (320, 240);
        let mesh = face(width, height, 50.0, 70.0);
        let mut canvas = canvas_of(width, height, [0, 0, 0]);
        canvas.draw_mesh(&mesh);
        let colors: std::collections::HashSet<[u8; 3]> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| pixel(&canvas, x, y))
            .collect();
        for color in [MESH_LINES, LIPS_LINES, EYES_LINES, BROWS_LINES] {
            assert!(colors.contains(&color), "{color:?}");
        }
    }

    #[test]
    fn the_depth_figure_is_colored_within_its_range_over_the_set() {
        let (width, height) = (64, 48);
        let set = [10, 200, 10];
        let mut canvas = canvas_of(width, height, set);
        // Depth 8 × 6: the left half far, the right half near; the person
        // on the middle two rows.
        let size = (8, 6);
        let depth: Vec<u16> = (0..48)
            .map(|i| if i % 8 < 4 { 1_000 } else { 9_000 })
            .collect();
        let mask: Vec<u8> = (0..48)
            .map(|i| if (16..32).contains(&i) { 255 } else { 0 })
            .collect();
        canvas.depth_figure(&depth, &mask, size, (1_000, 9_000), None);
        // Off the person: the set.
        assert_eq!(pixel(&canvas, 2, 2), set);
        assert_eq!(pixel(&canvas, 60, 45), set);
        // On the person: far is the colormap's start, near its end.
        assert_eq!(pixel(&canvas, 2, 22), turbo(0.0));
        assert_eq!(pixel(&canvas, 60, 22), turbo(1.0));
        // A range wider than the person's makes them one color.
        let mut flat = canvas_of(width, height, set);
        flat.depth_figure(&depth, &mask, size, (0, 60_000), None);
        assert_ne!(pixel(&flat, 60, 22), turbo(1.0));
    }

    #[test]
    fn inside_the_face_covering_the_figure_is_opaque_whatever_the_mask() {
        let (width, height) = (320, 240);
        let mesh = face(width, height, 50.0, 70.0);
        let room = [205, 150, 120];
        let mut canvas = canvas_of(width, height, room);
        // The mask misses everything; the depth is one value.
        let size = (16, 12);
        let depth = vec![5_000u16; 16 * 12];
        let mask = vec![0u8; 16 * 12];
        canvas.depth_figure(&depth, &mask, size, (0, 10_000), Some(&mesh));
        assert_eq!(pixel(&canvas, 160, 120), turbo(128.0 / 255.0));
        assert_eq!(pixel(&canvas, 5, 5), room);
    }

    #[test]
    fn turbo_runs_from_blue_to_red() {
        let far = turbo(0.1);
        let near = turbo(1.0);
        assert!(far[2] > far[0], "{far:?}");
        assert!(near[0] > near[2], "{near:?}");
        let middle = turbo(0.5);
        assert!(middle[1] > 200, "{middle:?}");
    }

    #[test]
    fn edges_split_into_their_contours() {
        assert_eq!(edges::LIPS.len(), 40);
        assert_eq!(edges::LIPS[0], [61, 146]);
        assert_eq!(edges::LEFT_EYE[0], [263, 249]);
        assert_eq!(edges::LEFT_EYEBROW[0], [276, 283]);
        assert_eq!(edges::RIGHT_EYE[0], [33, 7]);
        assert_eq!(edges::RIGHT_EYEBROW[0], [46, 53]);
        assert_eq!(edges::FACE_OVAL.len(), 36);
        assert_eq!(edges::FACE_OVAL[0], [10, 338]);
        // A closed loop, each edge starting where the last ended.
        for pair in edges::FACE_OVAL.windows(2) {
            assert_eq!(pair[0][1], pair[1][0]);
        }
        assert_eq!(edges::FACE_OVAL[35][1], edges::FACE_OVAL[0][0]);
        assert!(tessellation().len() > 1_200);
    }
}
