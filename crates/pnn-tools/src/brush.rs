//! Stroke engine for the Paintbrush, Eraser and Clone Stamp.
//!
//! paint.net 3.36 (`tools/PaintBrushTool.cs`, MIT, see NOTICE) swept GDI+ polygons straight
//! into the layer, so a semi-transparent stroke darkened where it overlapped itself. paint.net
//! 5 stamps round dabs with Hardness and Spacing (docs: `Paintbrush.html`) and a stroke does
//! not build up over itself. Its dab falloff, spacing floor and smoothing curve are not
//! published, so those here are paintnotnet's own:
//! - A stroke is a coverage mask: each dab writes `max(mask, dab)`, then the layer is
//!   recomposited from its pre-stroke copy, so a stroke never blends over itself.
//! - Dab coverage ramps (smoothstep) from full at `r·h − ½` to zero at `r + ½`, where `r` is
//!   the radius and `h` the hardness; at 100% that's a one-pixel antialiased edge. With
//!   antialiasing off the dab is a hard disc and hardness is ignored, as the docs say.
//! - Dabs are `max(spacing · width, ½ px)` apart along the path. Up to 50% spacing, where
//!   dabs overlap, the profile is swept from each dab to the next (distance to the segment),
//!   so soft strokes are smooth rather than scalloped; above that the dabs stand apart.
//! - Smoothing draws a Catmull-Rom curve through the pointer samples instead of straight
//!   segments, one sample behind the pointer.

use pnn_core::{BlendMode, ColorBgra, Mask, Rect, Surface, TILE_SIZE, TiledBuffer};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushShape {
    /// Diameter in px (fractional allowed).
    pub width: f32,
    /// 0..=1.
    pub hardness: f32,
    /// Dab distance as a fraction of the width.
    pub spacing: f32,
    pub antialias: bool,
}

impl BrushShape {
    pub fn from_settings(s: &crate::ToolSettings) -> Self {
        Self {
            width: s.brush_width,
            hardness: s.hardness as f32 / 100.0,
            spacing: s.spacing as f32 / 100.0,
            antialias: s.antialiasing,
        }
    }

    fn step(&self) -> f32 {
        (self.spacing * self.width).max(0.5)
    }

    /// Coverage (0..=1) of a pixel whose centre is `d` px from the dab centre.
    pub fn coverage(&self, d: f32) -> f32 {
        let r = self.width / 2.0;
        if !self.antialias {
            return if d <= r.max(0.5) { 1.0 } else { 0.0 };
        }
        let inner = r * self.hardness - 0.5;
        let outer = r + 0.5;
        if d <= inner {
            return 1.0;
        }
        if d >= outer {
            return 0.0;
        }
        let t = (outer - d) / (outer - inner);
        t * t * (3.0 - 2.0 * t)
    }

    /// Dab centre for a pointer position: with antialiasing off, odd widths centre on a pixel
    /// and even widths on a pixel corner, so hard dabs are symmetric.
    fn dab_centre(&self, p: [f32; 2]) -> [f32; 2] {
        if self.antialias {
            return p;
        }
        if (self.width.round() as i64) % 2 == 1 {
            [p[0].floor() + 0.5, p[1].floor() + 0.5]
        } else {
            [p[0].round(), p[1].round()]
        }
    }
}

/// The coverage a stroke has laid down so far.
pub struct StrokeMask {
    pub mask: Mask,
    shape: BrushShape,
    smoothing: bool,
    /// Pointer samples not yet fully drawn (smoothing keeps the last three).
    points: Vec<[f32; 2]>,
    /// Path length still to travel before the next dab.
    carry: f32,
    /// Where the drawn path currently ends.
    pen: Option<[f32; 2]>,
    /// Centre of the last dab, swept from in continuous mode.
    last_dab: Option<[f32; 2]>,
}

impl StrokeMask {
    pub fn new(width: u32, height: u32, shape: BrushShape, smoothing: bool) -> Self {
        Self {
            mask: TiledBuffer::new(width, height, 0),
            shape,
            smoothing,
            points: Vec::new(),
            carry: 0.0,
            pen: None,
            last_dab: None,
        }
    }

    pub fn shape(&self) -> BrushShape {
        self.shape
    }

    /// Add a pointer sample; returns the pixels whose coverage may have changed.
    pub fn add_point(&mut self, p: [f32; 2]) -> Option<Rect> {
        if self.points.last() == Some(&p) {
            return None;
        }
        self.points.push(p);
        let mut dirty = None;
        if self.points.len() == 1 {
            self.pen = Some(p);
            dirty = union(dirty, self.dab(p));
            self.carry = self.shape.step();
            return dirty;
        }
        if !self.smoothing {
            let a = self.points[self.points.len() - 2];
            dirty = union(dirty, self.walk(&[a, p]));
            self.points.drain(..self.points.len() - 1);
            return dirty;
        }
        // Draw the segment before the newest sample, whose tangent is now known.
        let n = self.points.len();
        if n >= 3 {
            let (p1, p2, p3) = (self.points[n - 3], self.points[n - 2], self.points[n - 1]);
            let p0 = if n >= 4 { self.points[n - 4] } else { p1 };
            dirty = union(dirty, self.walk(&catmull_rom(p0, p1, p2, p3)));
        }
        if self.points.len() > 4 {
            self.points.drain(..self.points.len() - 4);
        }
        dirty
    }

    /// Draw what smoothing still holds back. Call on pointer up.
    pub fn finish(&mut self) -> Option<Rect> {
        let n = self.points.len();
        if !self.smoothing || n < 2 {
            return self.close_tail();
        }
        let p0 = if n >= 3 {
            self.points[n - 3]
        } else {
            self.points[n - 2]
        };
        let (p1, p2) = (self.points[n - 2], self.points[n - 1]);
        let path = catmull_rom(p0, p1, p2, p2);
        self.points.clear();
        let dirty = self.walk(&path);
        union(dirty, self.close_tail())
    }

    /// In continuous mode, sweep from the last dab to where the path ends.
    fn close_tail(&mut self) -> Option<Rect> {
        match (self.continuous(), self.pen) {
            (true, Some(p)) if self.last_dab != Some(self.shape.dab_centre(p)) => self.dab(p),
            _ => None,
        }
    }

    fn continuous(&self) -> bool {
        self.shape.spacing <= 0.5
    }

    /// Stamp dabs every `step` px along a polyline, continuing the spacing from the previous
    /// segment.
    fn walk(&mut self, path: &[[f32; 2]]) -> Option<Rect> {
        let step = self.shape.step();
        let mut dirty = None;
        for seg in path.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let mut t = self.carry;
            while t <= len {
                let k = if len > 0.0 { t / len } else { 0.0 };
                let p = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
                dirty = union(dirty, self.dab(p));
                t += step;
            }
            self.carry = t - len;
            self.pen = Some(b);
        }
        dirty
    }

    /// Stamp a dab at `p`; in continuous mode, sweep it from the previous dab.
    fn dab(&mut self, p: [f32; 2]) -> Option<Rect> {
        let c = self.shape.dab_centre(p);
        let a = match self.last_dab {
            Some(a) if self.continuous() => a,
            _ => c,
        };
        self.last_dab = Some(c);
        let reach = self.shape.width / 2.0 + 1.0;
        let (w, h) = (self.mask.width() as i64, self.mask.height() as i64);
        let x0 = ((a[0].min(c[0]) - reach).floor() as i64).max(0);
        let y0 = ((a[1].min(c[1]) - reach).floor() as i64).max(0);
        let x1 = ((a[0].max(c[0]) + reach).ceil() as i64).min(w);
        let y1 = ((a[1].max(c[1]) + reach).ceil() as i64).min(h);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let r = Rect {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        };
        let shape = self.shape;
        // Distance from a pixel centre to the segment a→c (a point when they coincide).
        let (ex, ey) = (c[0] - a[0], c[1] - a[1]);
        let len2 = ex * ex + ey * ey;
        for_each_tile_row(&mut self.mask, r, |y, x_start, row| {
            let py = y as f32 + 0.5 - a[1];
            for (i, v) in row.iter_mut().enumerate() {
                let px = (x_start + i as u32) as f32 + 0.5 - a[0];
                let t = if len2 > 0.0 {
                    ((px * ex + py * ey) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (dx, dy) = (px - t * ex, py - t * ey);
                let k = (shape.coverage((dx * dx + dy * dy).sqrt()) * 255.0 + 0.5) as u8;
                if k > *v {
                    *v = k;
                }
            }
        });
        Some(r)
    }
}

/// Points along a Catmull-Rom segment from `p1` to `p2`, about a pixel apart.
fn catmull_rom(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) -> Vec<[f32; 2]> {
    let len = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
    let n = (len.ceil() as usize).clamp(1, 512);
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * ((2.0 * b)
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            [f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]
        })
        .collect()
}

pub fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (None, r) | (r, None) => r,
        (Some(a), Some(b)) => {
            let x0 = a.x.min(b.x);
            let y0 = a.y.min(b.y);
            let x1 = (a.x + a.width).max(b.x + b.width);
            let y1 = (a.y + a.height).max(b.y + b.height);
            Some(Rect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            })
        }
    }
}

/// Visit the rows of `r` tile by tile, tiles in parallel: `f(y, x of the row's first pixel,
/// row)`.
pub fn for_each_tile_row<T: Copy + PartialEq + Send + Sync>(
    buf: &mut TiledBuffer<T>,
    r: Rect,
    f: impl Fn(u32, u32, &mut [T]) + Sync,
) {
    use rayon::prelude::*;
    let absent = buf.absent();
    // Slots, made writable (allocated or un-shared) inside the workers, so the copies
    // happen in parallel.
    let tiles = buf.slots_in(r);
    let visit = |(c, slot): (
        pnn_core::TileCoord,
        &mut Option<std::sync::Arc<pnn_core::Tile<T>>>,
    )| {
        let tile = std::sync::Arc::make_mut(
            slot.get_or_insert_with(|| std::sync::Arc::new(pnn_core::Tile::filled(absent))),
        );
        let (ox, oy) = (c.tx * TILE_SIZE, c.ty * TILE_SIZE);
        let x0 = r.x.max(ox);
        let x1 = (r.x + r.width).min(ox + TILE_SIZE);
        let y0 = r.y.max(oy);
        let y1 = (r.y + r.height).min(oy + TILE_SIZE);
        for y in y0..y1 {
            let row = tile.row_mut(y - oy);
            f(y, x0, &mut row[(x0 - ox) as usize..(x1 - ox) as usize]);
        }
    };
    // Small brushes touch a tile or two: not worth waking the thread pool.
    if tiles.len() <= 2 {
        tiles.into_iter().for_each(visit);
    } else {
        tiles.into_par_iter().for_each(visit);
    }
}

/// How a stroke's coverage changes the layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrokeOp {
    /// Paint blended with a layer blend mode.
    Blend(BlendMode),
    /// Paint replaces the pixel (coverage-weighted at soft edges).
    Overwrite,
    /// Alpha scaled by `1 − coverage`, colour kept (paint.net 3.36 `EraserTool`).
    Erase,
    /// paint.net 3.36 `RecolorTool`: pixels within `tolerance` of `from` (its
    /// `ColorDifference`) have their RGB shifted by `to − from`; alpha is kept.
    Recolor {
        from: ColorBgra,
        to: ColorBgra,
        tolerance: i32,
    },
}

/// 3.36 `Utility.ColorDifference`: ⌈√((ΔR² + ΔG² + ΔB²) / 3)⌉.
pub fn color_difference(a: ColorBgra, b: ColorBgra) -> i32 {
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
    let sq = (d(a.r, b.r) + d(a.g, b.g) + d(a.b, b.b)) / 3;
    (sq as f64).sqrt().ceil() as i32
}

/// The tolerance 3.36's Recolor uses: `Tolerance × 256`, but no more than the difference
/// between the two colours (`RestrictTolerance`).
pub fn recolor_tolerance(tolerance_pct: u8, from: ColorBgra, to: ColorBgra) -> i32 {
    ((tolerance_pct as f32 / 100.0 * 256.0) as i32).min(color_difference(to, from))
}

/// Recomposite `r` of `layer` from its pre-stroke copy `before`: coverage is the stroke's
/// times the selection's; `paint(x, y)` gives the paint colour (solid or hatched).
pub fn composite(
    layer: &mut Surface,
    before: &Surface,
    stroke: &Mask,
    selection: Option<&Mask>,
    r: Rect,
    op: StrokeOp,
    paint: impl Fn(u32, u32) -> ColorBgra + Sync,
) {
    for_each_tile_row(layer, r, |y, x0, row| {
        for (i, out) in row.iter_mut().enumerate() {
            let x = x0 + i as u32;
            let old = before.get(x, y);
            let mut c = stroke.get(x, y) as u32;
            if let Some(m) = selection {
                c = c * m.get(x, y) as u32 / 255;
            }
            *out = if c == 0 {
                old
            } else {
                match op {
                    StrokeOp::Erase => old.with_alpha((old.a as u32 * (255 - c) / 255) as u8),
                    StrokeOp::Overwrite => {
                        pnn_core::color::lerp_premultiplied(old, paint(x, y), c as u8)
                    }
                    StrokeOp::Blend(mode) => {
                        let p = paint(x, y);
                        mode.apply(old, p.with_alpha((p.a as u32 * c / 255) as u8), 255)
                    }
                    StrokeOp::Recolor {
                        from,
                        to,
                        tolerance,
                    } => {
                        if color_difference(old, from) > tolerance {
                            old
                        } else {
                            // Shifted colour, mixed in by coverage at soft edges.
                            let ch = |o: u8, t: u8, f: u8| {
                                let adj = (o as i32 + t as i32 - f as i32).clamp(0, 255);
                                (o as i32 + (adj - o as i32) * c as i32 / 255) as u8
                            };
                            ColorBgra::from_bgra(
                                ch(old.b, to.b, from.b),
                                ch(old.g, to.g, from.g),
                                ch(old.r, to.r, from.r),
                                old.a,
                            )
                        }
                    }
                }
            };
        }
    });
}

/// paint.net's hatch fill styles that paintnotnet draws (GDI+ `HatchStyle` names). The 8×8
/// cell phase is a guess; 3.36 drew them with GDI+ and 5.x's are unpublished.
pub const HATCHES: [&str; 6] = [
    "Horizontal",
    "Vertical",
    "Forward Diagonal",
    "Backward Diagonal",
    "Cross",
    "Diagonal Cross",
];

/// Whether the hatch's foreground (the drawing colour) covers pixel (x, y).
pub fn hatch_on(index: u8, x: u32, y: u32) -> bool {
    let (x, y) = (x % 8, y % 8);
    let h = y == 0;
    let v = x == 0;
    let f = x == y;
    let b = x + y == 7;
    match index {
        0 => h,
        1 => v,
        2 => f,
        3 => b,
        4 => h || v,
        _ => f || b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(width: f32, hardness: f32, antialias: bool) -> BrushShape {
        BrushShape {
            width,
            hardness,
            spacing: 0.15,
            antialias,
        }
    }

    #[test]
    fn hard_dab_has_one_pixel_edge() {
        let s = shape(10.0, 1.0, true);
        assert_eq!(s.coverage(0.0), 1.0);
        assert_eq!(s.coverage(4.5), 1.0);
        assert!((s.coverage(5.0) - 0.5).abs() < 1e-6);
        assert_eq!(s.coverage(5.5), 0.0);
    }

    #[test]
    fn soft_dab_falls_off_from_the_centre() {
        let s = shape(20.0, 0.0, true);
        assert!(s.coverage(0.0) > 0.99);
        assert!(s.coverage(5.0) > s.coverage(8.0));
        assert_eq!(s.coverage(10.5), 0.0);
    }

    #[test]
    fn aliased_dab_ignores_hardness_and_is_symmetric() {
        let mut m = StrokeMask::new(20, 20, shape(3.0, 0.0, false), false);
        m.add_point([10.2, 10.7]);
        let on: Vec<(u32, u32)> = (0..20)
            .flat_map(|y| (0..20).map(move |x| (x, y)))
            .filter(|&(x, y)| m.mask.get(x, y) > 0)
            .collect();
        assert!(on.iter().all(|&(x, y)| m.mask.get(x, y) == 255));
        // Width 3 centred on pixel (10, 10): a plus-shaped disc of 9 pixels at most.
        assert!(on.contains(&(10, 10)) && on.contains(&(9, 10)) && on.contains(&(11, 10)));
        assert!(!on.contains(&(12, 10)) && !on.contains(&(8, 10)));
    }

    #[test]
    fn stroke_is_continuous_and_does_not_build_up() {
        let mut m = StrokeMask::new(100, 40, shape(6.0, 1.0, true), false);
        m.add_point([10.0, 20.0]);
        m.add_point([90.0, 20.0]);
        m.add_point([10.0, 20.0]); // back over itself
        for x in 10..90 {
            assert_eq!(m.mask.get(x, 20), 255, "gap at {x}");
        }
        let mut layer = Surface::new_transparent(100, 40);
        let before = layer.clone();
        let paint = ColorBgra::from_rgba(255, 0, 0, 128);
        composite(
            &mut layer,
            &before,
            &m.mask,
            None,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 40,
            },
            StrokeOp::Blend(BlendMode::Normal),
            |_, _| paint,
        );
        assert_eq!(
            layer.get(50, 20).a,
            128,
            "one layer of paint despite the overlap"
        );
    }

    #[test]
    fn spacing_carries_across_segments() {
        let mut s = shape(2.0, 1.0, true);
        s.spacing = 2.0; // dabs 4 px apart
        let mut m = StrokeMask::new(40, 10, s, false);
        for x in [2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0] {
            m.add_point([x, 5.0]);
        }
        // Dabs at x = 2, 6, 10; pixels 3 and 4 are more than r + ½ from all of them.
        assert!(m.mask.get(1, 4) > 0 && m.mask.get(5, 4) > 0 && m.mask.get(9, 4) > 0);
        assert_eq!(
            m.mask.get(3, 4) & m.mask.get(4, 4),
            0,
            "no dab between 2 and 6"
        );
    }

    #[test]
    fn smoothing_draws_through_every_sample() {
        let mut m = StrokeMask::new(100, 100, shape(2.0, 1.0, true), true);
        let pts = [[10.0, 10.0], [50.0, 15.0], [80.0, 50.0], [60.0, 90.0]];
        for p in pts {
            m.add_point(p);
        }
        m.finish();
        for p in pts {
            assert!(m.mask.get(p[0] as u32, p[1] as u32) > 0, "{p:?}");
        }
    }

    #[test]
    fn erase_and_overwrite() {
        let mut layer = Surface::new_transparent(4, 1);
        for x in 0..4 {
            layer.set(x, 0, ColorBgra::from_rgba(0, 0, 255, 200));
        }
        let before = layer.clone();
        let mut stroke: Mask = TiledBuffer::new(4, 1, 0);
        stroke.set(1, 0, 255);
        stroke.set(2, 0, 128);
        let all = Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
        };
        composite(
            &mut layer,
            &before,
            &stroke,
            None,
            all,
            StrokeOp::Erase,
            |_, _| ColorBgra::BLACK,
        );
        assert_eq!(layer.get(0, 0).a, 200);
        assert_eq!(layer.get(1, 0).a, 0);
        assert_eq!(layer.get(2, 0).a, 99);
        assert_eq!(layer.get(2, 0).b, 255, "colour kept");
        let red = ColorBgra::from_rgba(255, 0, 0, 0);
        composite(
            &mut layer,
            &before,
            &stroke,
            None,
            all,
            StrokeOp::Overwrite,
            |_, _| red,
        );
        assert_eq!(
            layer.get(1, 0).a,
            0,
            "overwrite replaces, even with transparent paint"
        );
        assert_eq!(layer.get(0, 0).a, 200);
    }

    #[test]
    fn hatch_patterns() {
        assert!(hatch_on(0, 3, 8) && !hatch_on(0, 3, 9));
        assert!(hatch_on(4, 0, 5) && hatch_on(4, 5, 0) && !hatch_on(4, 5, 5));
        assert!(hatch_on(2, 3, 3) && hatch_on(3, 3, 4));
    }

    #[test]
    #[ignore = "timing; run with --release --ignored --nocapture"]
    fn big_brush_4k() {
        let (w, h) = (3840, 2160);
        let mut layer = Surface::new_transparent(w, h);
        let before = layer.clone();
        for width in [20.0, 200.0, 800.0] {
            let mut m = StrokeMask::new(w, h, shape(width, 0.75, true), true);
            let mut times = Vec::new();
            for i in 0..60 {
                let p = [
                    200.0 + i as f32 * 50.0,
                    1000.0 + (i as f32 * 0.3).sin() * 300.0,
                ];
                let t = std::time::Instant::now();
                if let Some(r) = m.add_point(p) {
                    composite(
                        &mut layer,
                        &before,
                        &m.mask,
                        None,
                        r,
                        StrokeOp::Blend(BlendMode::Normal),
                        |_, _| ColorBgra::BLACK,
                    );
                }
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            println!(
                "width {width}: per pointer move median {:.2} ms, max {:.2} ms",
                times[30], times[59]
            );
        }
    }

    #[test]
    fn recolor_shifts_matching_pixels_only() {
        let mut layer = Surface::new_transparent(3, 1);
        layer.set(0, 0, ColorBgra::from_rgba(200, 10, 10, 255)); // near red
        layer.set(1, 0, ColorBgra::from_rgba(10, 200, 10, 128)); // green: no match
        layer.set(2, 0, ColorBgra::from_rgba(250, 0, 0, 255)); // red, half coverage
        let before = layer.clone();
        let mut stroke: Mask = TiledBuffer::new(3, 1, 255);
        stroke.set(2, 0, 128);
        let (from, to) = (
            ColorBgra::from_rgba(255, 0, 0, 255),
            ColorBgra::from_rgba(0, 0, 255, 255),
        );
        let tolerance = recolor_tolerance(50, from, to);
        assert_eq!(tolerance, 128, "50% × 256");
        let all = Rect {
            x: 0,
            y: 0,
            width: 3,
            height: 1,
        };
        let op = StrokeOp::Recolor {
            from,
            to,
            tolerance,
        };
        composite(&mut layer, &before, &stroke, None, all, op, |_, _| {
            ColorBgra::BLACK
        });
        // (200,10,10) + (0,0,255) − (255,0,0) = (0, 10, 255) after clamping.
        assert_eq!(layer.get(0, 0), ColorBgra::from_rgba(0, 10, 255, 255));
        assert_eq!(layer.get(1, 0), before.get(1, 0));
        let half = layer.get(2, 0);
        assert!(
            half.r > 100 && half.r < 150 && half.b > 100 && half.b < 150,
            "{half:?}"
        );
        assert_eq!(color_difference(from, to), 209); // ⌈√(2·255²/3)⌉
    }
}
