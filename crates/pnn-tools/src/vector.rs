//! Vector drawing for Line/Curve and Shapes: paths, outlines and their rasterisation into a
//! "shape layer" that is then blended onto the image layer.
//!
//! Behaviour from paint.net 5's docs (`LineCurveTool.html`, `ShapeTools.html`): four-nub
//! curves (Straight / Cubic Spline / Bézier), start and end caps, dash styles, fill styles
//! using both colours, antialiasing, and blend modes applied "as if the pixels were on their
//! own layer". From paint.net 3.36 (MIT, see NOTICE): the cap set (`LineCap2`), arrow caps as
//! GDI+ `AdjustableArrowCap(5, 5)` in pen widths (`PenInfo.cs`), GDI+'s dash patterns, and the
//! cardinal spline GDI+ `DrawCurve` draws (tension 0.5).
//!
//! Rasterisation is tiny-skia's analytic antialiasing; paint.net 5 supersamples (2×/4×), so
//! edge values differ slightly.

use pnn_core::{BlendMode, ColorBgra, Mask, Rect, Surface};
use tiny_skia::{
    FillRule, LineCap as SkCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, StrokeDash,
    Transform,
};

use crate::brush::{self, for_each_tile_row};
use crate::{CurveType, DashStyle, FillStyle, LineCap};

/// GDI+ `DrawCurve`'s default tension.
const TENSION: f32 = 0.5;

/// Points along the curve through (or controlled by) the four nubs, for tangents and hit
/// tests.
pub fn curve_points(p: &[[f32; 2]; 4], curve: CurveType, per_segment: usize) -> Vec<[f32; 2]> {
    let n = per_segment.max(1);
    let mut out = vec![p[0]];
    let mut cubic = |a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]| {
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let f = |a: f32, b: f32, c: f32, d: f32| {
                u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
            };
            out.push([f(a[0], b[0], c[0], d[0]), f(a[1], b[1], c[1], d[1])]);
        }
    };
    match curve {
        CurveType::Straight => {
            for w in p.windows(2) {
                cubic(w[0], w[0], w[1], w[1]);
            }
        }
        CurveType::Bezier => cubic(p[0], p[1], p[2], p[3]),
        CurveType::Spline => {
            for [a, b, c1, c2] in spline_segments(p) {
                cubic(a, c1, c2, b);
            }
        }
    }
    out
}

/// Cardinal spline segments [start, end, control 1, control 2], as GDI+ `DrawCurve`.
fn spline_segments(p: &[[f32; 2]; 4]) -> Vec<[[f32; 2]; 4]> {
    let at = |i: isize| p[i.clamp(0, 3) as usize];
    (0..3isize)
        .map(|i| {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let k = TENSION / 3.0;
            let c1 = [p1[0] + (p2[0] - p0[0]) * k, p1[1] + (p2[1] - p0[1]) * k];
            let c2 = [p2[0] - (p3[0] - p1[0]) * k, p2[1] - (p3[1] - p1[1]) * k];
            [p1, p2, c1, c2]
        })
        .collect()
}

/// The path through the four nubs. `None` when every nub is at the same point.
pub fn line_path(p: &[[f32; 2]; 4], curve: CurveType) -> Option<Path> {
    if p.iter().all(|q| q == &p[0]) {
        return None;
    }
    let mut b = PathBuilder::new();
    b.move_to(p[0][0], p[0][1]);
    match curve {
        CurveType::Straight => {
            for q in &p[1..] {
                b.line_to(q[0], q[1]);
            }
        }
        CurveType::Bezier => b.cubic_to(p[1][0], p[1][1], p[2][0], p[2][1], p[3][0], p[3][1]),
        CurveType::Spline => {
            for [_, e, c1, c2] in spline_segments(p) {
                b.cubic_to(c1[0], c1[1], c2[0], c2[1], e[0], e[1]);
            }
        }
    }
    b.finish()
}

/// Pen settings for an outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub width: f32,
    pub dash: DashStyle,
    pub start_cap: LineCap,
    pub end_cap: LineCap,
}

/// How a part gets its colour: solid `fore`, or a hatch of `fore` on `back`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub fore: ColorBgra,
    pub back: ColorBgra,
    pub fill: FillStyle,
}

impl Ink {
    fn at(&self, x: u32, y: u32) -> ColorBgra {
        match self.fill {
            FillStyle::Hatch(i) if !brush::hatch_on(i, x, y) => self.back,
            _ => self.fore,
        }
    }
}

/// Arrow-cap tangents of an open path: `[tip, from]` at the start and at the end.
pub type Ends = ([[f32; 2]; 2], [[f32; 2]; 2]);

/// One piece of a drawing, painted in order (fill before outline).
#[derive(Clone, Debug)]
pub enum Part {
    Fill(Path, Ink),
    /// An open or closed path drawn with a pen. `ends` gives the start and end tangent
    /// points `(tip, from)` used for arrow caps on open paths.
    Outline {
        path: Path,
        pen: Pen,
        ink: Ink,
        ends: Option<Ends>,
    },
}

impl Part {
    fn reach(&self) -> f32 {
        match self {
            Part::Fill(..) => 1.0,
            Part::Outline { pen, .. } => pen.width * 4.0 + 2.0,
        }
    }
    fn path(&self) -> &Path {
        match self {
            Part::Fill(p, _) | Part::Outline { path: p, .. } => p,
        }
    }
}

/// A rendered drawing over `bounds`: per pixel the composited paint (straight alpha, with
/// coverage), the paint at full coverage (for Overwrite), and the total coverage.
pub struct Drawn {
    pub bounds: Rect,
    pub color: Vec<ColorBgra>,
    pub solid: Vec<ColorBgra>,
    pub coverage: Vec<u8>,
}

/// Canvas pixels the parts can touch, clipped to the canvas.
pub fn parts_bounds(parts: &[Part], width: u32, height: u32) -> Option<Rect> {
    let mut acc: Option<[f32; 4]> = None;
    for p in parts {
        let b = p.path().bounds();
        let r = p.reach();
        let e = [b.left() - r, b.top() - r, b.right() + r, b.bottom() + r];
        acc = Some(match acc {
            None => e,
            Some(a) => [
                a[0].min(e[0]),
                a[1].min(e[1]),
                a[2].max(e[2]),
                a[3].max(e[3]),
            ],
        });
    }
    let [x0, y0, x1, y1] = acc?;
    let x0 = (x0.floor().max(0.0) as u32).min(width);
    let y0 = (y0.floor().max(0.0) as u32).min(height);
    let x1 = (x1.ceil().max(0.0) as u32).min(width);
    let y1 = (y1.ceil().max(0.0) as u32).min(height);
    (x1 > x0 && y1 > y0).then(|| Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// Coverage of one part over `r` (row-major).
fn part_coverage(part: &Part, r: Rect, antialias: bool) -> Vec<u8> {
    let mut pm = Pixmap::new(r.width, r.height).expect("non-empty bounds");
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = antialias;
    let t = Transform::from_translate(-(r.x as f32), -(r.y as f32));
    match part {
        // Even-odd, so inner contours (the Gear's hole) are holes.
        Part::Fill(path, _) => pm.fill_path(path, &paint, FillRule::EvenOdd, t, None),
        Part::Outline {
            path, pen, ends, ..
        } => {
            let w = pen.width.max(0.01);
            let mut stroke = Stroke {
                width: w,
                line_cap: SkCap::Butt,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            let pattern: Vec<f32> = pen.dash.pattern().iter().map(|v| v * w).collect();
            if !pattern.is_empty() {
                stroke.dash = StrokeDash::new(pattern, 0.0);
            }
            pm.stroke_path(path, &paint, &stroke, t, None);
            if let Some((start, end)) = ends {
                for (cap, [tip, from]) in [(pen.start_cap, *start), (pen.end_cap, *end)] {
                    draw_cap(&mut pm, &paint, cap, tip, from, w, t);
                }
            }
        }
    }
    pm.pixels().iter().map(|p| p.alpha()).collect()
}

/// A cap at `tip`, the line arriving from `from`.
fn draw_cap(
    pm: &mut Pixmap,
    paint: &Paint,
    cap: LineCap,
    tip: [f32; 2],
    from: [f32; 2],
    w: f32,
    t: Transform,
) {
    let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy, ux);
    match cap {
        LineCap::Flat => {}
        LineCap::Rounded => {
            if let Some(c) = PathBuilder::from_circle(tip[0], tip[1], w / 2.0) {
                pm.fill_path(&c, paint, FillRule::Winding, t, None);
            }
        }
        LineCap::Arrow | LineCap::ArrowFilled => {
            // GDI+ `AdjustableArrowCap(5, 5)`: 5 pen widths wide and long.
            let (len, half) = (5.0 * w, 2.5 * w);
            let base = [tip[0] - ux * len, tip[1] - uy * len];
            let a = [base[0] + nx * half, base[1] + ny * half];
            let b = [base[0] - nx * half, base[1] - ny * half];
            let mut pb = PathBuilder::new();
            pb.move_to(a[0], a[1]);
            pb.line_to(tip[0], tip[1]);
            pb.line_to(b[0], b[1]);
            if cap == LineCap::ArrowFilled {
                pb.close();
            }
            let Some(path) = pb.finish() else {
                return;
            };
            if cap == LineCap::ArrowFilled {
                pm.fill_path(&path, paint, FillRule::Winding, t, None);
            } else {
                let stroke = Stroke {
                    width: w,
                    line_join: LineJoin::Miter,
                    ..Stroke::default()
                };
                pm.stroke_path(&path, paint, &stroke, t, None);
            }
        }
    }
}

/// Render `parts` onto a transparent shape layer covering `bounds`.
pub fn draw(parts: &[Part], bounds: Rect, antialias: bool) -> Drawn {
    let n = (bounds.width * bounds.height) as usize;
    let mut color = vec![ColorBgra::TRANSPARENT; n];
    let mut solid = vec![ColorBgra::TRANSPARENT; n];
    let mut coverage = vec![0u8; n];
    for part in parts {
        let k = part_coverage(part, bounds, antialias);
        let ink = match part {
            Part::Fill(_, ink) | Part::Outline { ink, .. } => *ink,
        };
        for (i, &c) in k.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let x = bounds.x + i as u32 % bounds.width;
            let y = bounds.y + i as u32 / bounds.width;
            let p = ink.at(x, y);
            color[i] = BlendMode::Normal.apply(color[i], p.with_alpha(scale(p.a, c)), 255);
            solid[i] = if coverage[i] == 0 {
                p
            } else {
                pnn_core::color::lerp_premultiplied(solid[i], p, c)
            };
            coverage[i] = coverage[i].max(c);
        }
    }
    Drawn {
        bounds,
        color,
        solid,
        coverage,
    }
}

fn scale(a: u8, b: u8) -> u8 {
    ((a as u32 * b as u32 + 127) / 255) as u8
}

/// How the drawing lands on the layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawOp {
    Blend(BlendMode),
    Overwrite,
}

/// Restore `r` of `layer` from `before`, then blend the drawing over it, clipped to the
/// (soft) selection.
pub fn composite(
    layer: &mut Surface,
    before: &Surface,
    selection: Option<&Mask>,
    drawn: Option<&Drawn>,
    r: Rect,
    op: DrawOp,
) {
    for_each_tile_row(layer, r, |y, x0, row| {
        for (i, out) in row.iter_mut().enumerate() {
            let x = x0 + i as u32;
            let old = before.get(x, y);
            *out = old;
            let Some(d) = drawn else {
                continue;
            };
            let b = d.bounds;
            if x < b.x || y < b.y || x >= b.x + b.width || y >= b.y + b.height {
                continue;
            }
            let j = ((y - b.y) * b.width + (x - b.x)) as usize;
            let mut k = d.coverage[j];
            if let Some(m) = selection {
                k = scale(k, m.get(x, y));
            }
            if k == 0 {
                continue;
            }
            *out = match op {
                DrawOp::Overwrite => pnn_core::color::lerp_premultiplied(old, d.solid[j], k),
                DrawOp::Blend(mode) => {
                    let mut c = d.color[j];
                    if selection.is_some() {
                        c = c.with_alpha(scale(c.a, selection.map_or(255, |m| m.get(x, y))));
                    }
                    mode.apply(old, c, 255)
                }
            };
        }
    });
}

/// Distance from `p` to the polyline `pts`.
pub fn distance_to_polyline(p: [f32; 2], pts: &[[f32; 2]]) -> f32 {
    let mut best = f32::INFINITY;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
        let l2 = ex * ex + ey * ey;
        let t = if l2 > 0.0 {
            (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (dx, dy) = (p[0] - a[0] - t * ex, p[1] - a[1] - t * ey);
        best = best.min((dx * dx + dy * dy).sqrt());
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Ink = Ink {
        fore: ColorBgra::BLACK,
        back: ColorBgra::WHITE,
        fill: FillStyle::Solid,
    };

    fn straight(a: [f32; 2], b: [f32; 2]) -> [[f32; 2]; 4] {
        let l = |t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        [a, l(1.0 / 3.0), l(2.0 / 3.0), b]
    }

    fn outline(p: [[f32; 2]; 4], curve: CurveType, pen: Pen) -> Part {
        let pts = curve_points(&p, curve, 16);
        let n = pts.len();
        Part::Outline {
            path: line_path(&p, curve).unwrap(),
            pen,
            ink: BLACK,
            ends: Some(([pts[0], pts[1]], [pts[n - 1], pts[n - 2]])),
        }
    }

    fn pen(width: f32) -> Pen {
        Pen {
            width,
            dash: DashStyle::Solid,
            start_cap: LineCap::Flat,
            end_cap: LineCap::Flat,
        }
    }

    fn cov_at(d: &Drawn, x: u32, y: u32) -> u8 {
        d.coverage[((y - d.bounds.y) * d.bounds.width + (x - d.bounds.x)) as usize]
    }

    #[test]
    fn all_curve_types_pass_through_their_ends() {
        let p = [[10.0, 10.0], [30.0, 60.0], [70.0, 0.0], [90.0, 50.0]];
        for curve in [CurveType::Straight, CurveType::Spline, CurveType::Bezier] {
            let pts = curve_points(&p, curve, 8);
            assert_eq!(pts[0], p[0]);
            let last = *pts.last().unwrap();
            assert!((last[0] - 90.0).abs() < 1e-4 && (last[1] - 50.0).abs() < 1e-4);
        }
        // Spline goes through the middle nubs; Bézier generally doesn't.
        let spline = curve_points(&p, CurveType::Spline, 32);
        assert!(distance_to_polyline(p[1], &spline) < 0.01);
        let bezier = curve_points(&p, CurveType::Bezier, 64);
        assert!(distance_to_polyline(p[1], &bezier) > 5.0);
    }

    #[test]
    fn line_width_and_flat_ends() {
        let part = outline(
            straight([10.0, 20.0], [50.0, 20.0]),
            CurveType::Straight,
            pen(4.0),
        );
        let b = parts_bounds(std::slice::from_ref(&part), 100, 100).unwrap();
        let d = draw(&[part], b, true);
        assert_eq!(cov_at(&d, 30, 18), 255);
        assert_eq!(cov_at(&d, 30, 21), 255);
        assert_eq!(cov_at(&d, 30, 23), 0);
        assert_eq!(cov_at(&d, 8, 20), 0, "flat start");
        assert_eq!(cov_at(&d, 51, 20), 0, "flat end");
    }

    #[test]
    fn caps_extend_the_ends() {
        let mut p = pen(4.0);
        p.start_cap = LineCap::Rounded;
        p.end_cap = LineCap::ArrowFilled;
        let part = outline(straight([20.0, 50.0], [60.0, 50.0]), CurveType::Straight, p);
        let b = parts_bounds(std::slice::from_ref(&part), 100, 100).unwrap();
        let d = draw(&[part], b, true);
        assert!(cov_at(&d, 18, 50) > 0, "round start bulges past the end");
        // Arrow head: 20 px long, 10 px wide at its base (x = 40).
        assert!(cov_at(&d, 42, 45) > 200, "arrow wing");
        assert_eq!(cov_at(&d, 30, 45), 0, "no wing before the head");
    }

    #[test]
    fn dashes_leave_gaps() {
        let mut p = pen(2.0);
        p.dash = DashStyle::Dash; // 6 on, 2 off
        let part = outline(straight([0.0, 10.0], [80.0, 10.0]), CurveType::Straight, p);
        let b = parts_bounds(std::slice::from_ref(&part), 100, 20).unwrap();
        let d = draw(&[part], b, true);
        let row: Vec<u8> = (0..80).map(|x| cov_at(&d, x, 10)).collect();
        assert!(
            row[2] == 255 && row[6] == 0 && row[7] == 0 && row[9] == 255,
            "{row:?}"
        );
    }

    #[test]
    fn overwrite_and_blend() {
        let part = outline(
            straight([0.0, 2.0], [10.0, 2.0]),
            CurveType::Straight,
            pen(2.0),
        );
        let mut red = BLACK;
        red.fore = ColorBgra::from_rgba(255, 0, 0, 0);
        let part = match part {
            Part::Outline {
                path, pen, ends, ..
            } => Part::Outline {
                path,
                pen,
                ink: red,
                ends,
            },
            p => p,
        };
        let b = parts_bounds(std::slice::from_ref(&part), 10, 4).unwrap();
        let d = draw(&[part], b, true);
        let mut before = Surface::new_transparent(10, 4);
        for y in 0..4 {
            for x in 0..10 {
                before.set(x, y, ColorBgra::WHITE);
            }
        }
        let all = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 4,
        };
        let mut layer = before.clone();
        composite(
            &mut layer,
            &before,
            None,
            Some(&d),
            all,
            DrawOp::Blend(BlendMode::Normal),
        );
        assert_eq!(
            layer.get(5, 1),
            ColorBgra::WHITE,
            "transparent paint blends to nothing"
        );
        composite(&mut layer, &before, None, Some(&d), all, DrawOp::Overwrite);
        assert_eq!(
            layer.get(5, 1).a,
            0,
            "overwrite with transparent paint erases"
        );
        assert_eq!(layer.get(5, 3), ColorBgra::WHITE);
    }
}
