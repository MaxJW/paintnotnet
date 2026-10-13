//! Geometry of the Shapes tool's 29 presets.
//!
//! paint.net 5 lists the presets (`ShapeTools.html`) but its shape geometry isn't published;
//! these outlines are paintnotnet's own drawings of each named shape. Every shape is built in
//! the unit square (u right, v down) and mapped through `map`, so the caller can stretch,
//! flip and rotate it while outlines keep their pen width. Only the Rounded Rectangle's
//! corners are in pixels (Corner Size), which is why `size` is passed in.

use tiny_skia::{Path, PathBuilder};

use crate::ShapeType;

/// Bézier circle constant.
const KAPPA: f64 = 0.552_284_75;

/// Builds a path in unit-square coordinates, emitting mapped points.
struct Pen<'a> {
    b: PathBuilder,
    map: &'a dyn Fn([f64; 2]) -> [f64; 2],
}

impl Pen<'_> {
    fn p(&self, u: f64, v: f64) -> (f32, f32) {
        let q = (self.map)([u, v]);
        (q[0] as f32, q[1] as f32)
    }
    fn move_to(&mut self, u: f64, v: f64) {
        let (x, y) = self.p(u, v);
        self.b.move_to(x, y);
    }
    fn line_to(&mut self, u: f64, v: f64) {
        let (x, y) = self.p(u, v);
        self.b.line_to(x, y);
    }
    fn cubic_to(&mut self, c: [f64; 6]) {
        let (x1, y1) = self.p(c[0], c[1]);
        let (x2, y2) = self.p(c[2], c[3]);
        let (x, y) = self.p(c[4], c[5]);
        self.b.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn polygon(&mut self, pts: &[[f64; 2]]) {
        self.move_to(pts[0][0], pts[0][1]);
        for q in &pts[1..] {
            self.line_to(q[0], q[1]);
        }
        self.b.close();
    }
    /// Ellipse inscribed in [u0, u1] × [v0, v1].
    fn ellipse(&mut self, u0: f64, v0: f64, u1: f64, v1: f64) {
        let (cx, cy) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let (rx, ry) = ((u1 - u0) / 2.0, (v1 - v0) / 2.0);
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        self.move_to(cx + rx, cy);
        self.cubic_to([cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry]);
        self.cubic_to([cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy]);
        self.cubic_to([cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry]);
        self.cubic_to([cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy]);
        self.b.close();
    }
    /// Rounded rectangle [0, 1] × [0, bottom] with corner radii (rx, ry) in unit space, and
    /// optionally a callout tail on the bottom edge.
    fn rounded_rect(&mut self, rx: f64, ry: f64, bottom: f64, tail: Option<[[f64; 2]; 3]>) {
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        self.move_to(rx, 0.0);
        self.line_to(1.0 - rx, 0.0);
        self.cubic_to([1.0 - rx + kx, 0.0, 1.0, ry - ky, 1.0, ry]);
        self.line_to(1.0, bottom - ry);
        self.cubic_to([
            1.0,
            bottom - ry + ky,
            1.0 - rx + kx,
            bottom,
            1.0 - rx,
            bottom,
        ]);
        if let Some(t) = tail {
            for q in t {
                self.line_to(q[0], q[1]);
            }
        }
        self.line_to(rx, bottom);
        self.cubic_to([rx - kx, bottom, 0.0, bottom - ry + ky, 0.0, bottom - ry]);
        self.line_to(0.0, ry);
        self.cubic_to([0.0, ry - ky, rx - kx, 0.0, rx, 0.0]);
        self.b.close();
    }
}

/// Points of a regular polygon (`inner` = None) or star, stretched to fill the unit square.
fn radial(points: usize, inner: Option<f64>) -> Vec<[f64; 2]> {
    let n = if inner.is_some() { points * 2 } else { points };
    let raw: Vec<[f64; 2]> = (0..n)
        .map(|k| {
            let a = -std::f64::consts::FRAC_PI_2 + k as f64 * std::f64::consts::TAU / n as f64;
            let r = match inner {
                Some(r) if k % 2 == 1 => r,
                _ => 1.0,
            };
            [r * a.cos(), r * a.sin()]
        })
        .collect();
    fit(&raw)
}

/// Scale and shift points to span the unit square.
fn fit(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in pts {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    pts.iter()
        .map(|p| {
            [
                (p[0] - lo[0]) / (hi[0] - lo[0]),
                (p[1] - lo[1]) / (hi[1] - lo[1]),
            ]
        })
        .collect()
}

/// The shape's outline. `size` is the box in pixels (for the Rounded Rectangle's corner
/// radius); `map` takes unit-square points to image coordinates.
pub fn shape_path(
    shape: ShapeType,
    size: [f64; 2],
    corner_radius: f64,
    map: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Option<Path> {
    let mut pen = Pen {
        b: PathBuilder::new(),
        map,
    };
    let (w, h) = (size[0].abs().max(1e-6), size[1].abs().max(1e-6));
    let radii = |bottom: f64| {
        (
            (corner_radius / w).clamp(0.0, 0.5),
            (corner_radius / h).clamp(0.0, 0.5 * bottom),
        )
    };
    use ShapeType as S;
    match shape {
        S::Rectangle => pen.polygon(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        S::RoundedRectangle => {
            let (rx, ry) = radii(1.0);
            pen.rounded_rect(rx, ry, 1.0, None);
        }
        S::Ellipse => pen.ellipse(0.0, 0.0, 1.0, 1.0),
        S::Diamond => pen.polygon(&[[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]]),
        S::Trapezoid => pen.polygon(&[[0.25, 0.0], [0.75, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        S::Parallelogram => pen.polygon(&[[0.25, 0.0], [1.0, 0.0], [0.75, 1.0], [0.0, 1.0]]),
        S::Triangle => pen.polygon(&[[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        S::RightTriangle => pen.polygon(&[[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        S::Pentagon => pen.polygon(&radial(5, None)),
        S::Hexagon => pen.polygon(&radial(6, None)),
        S::Heptagon => pen.polygon(&radial(7, None)),
        S::Octagon => pen.polygon(&radial(8, None)),
        S::Star3 => pen.polygon(&radial(3, Some(0.3))),
        S::Star4 => pen.polygon(&radial(4, Some(0.4))),
        S::Star5 => pen.polygon(&radial(5, Some(0.382))),
        S::Star6 => pen.polygon(&radial(6, Some(0.5))),
        S::Arrow => pen.polygon(&[
            [0.0, 0.3],
            [0.6, 0.3],
            [0.6, 0.0],
            [1.0, 0.5],
            [0.6, 1.0],
            [0.6, 0.7],
            [0.0, 0.7],
        ]),
        S::NotchedArrow => pen.polygon(&[
            [0.0, 0.3],
            [0.6, 0.3],
            [0.6, 0.0],
            [1.0, 0.5],
            [0.6, 1.0],
            [0.6, 0.7],
            [0.0, 0.7],
            [0.12, 0.5],
        ]),
        S::PentagonArrow => {
            pen.polygon(&[[0.0, 0.0], [0.75, 0.0], [1.0, 0.5], [0.75, 1.0], [0.0, 1.0]])
        }
        S::ChevronArrow => pen.polygon(&[
            [0.0, 0.0],
            [0.75, 0.0],
            [1.0, 0.5],
            [0.75, 1.0],
            [0.0, 1.0],
            [0.25, 0.5],
        ]),
        S::RectangularCallout => pen.polygon(&[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 0.8],
            [0.45, 0.8],
            [0.2, 1.0],
            [0.25, 0.8],
            [0.0, 0.8],
        ]),
        S::RoundedRectangleCallout => {
            let (rx, ry) = radii(0.8);
            pen.rounded_rect(rx, ry, 0.8, Some([[0.45, 0.8], [0.2, 1.0], [0.25, 0.8]]));
        }
        S::EllipseCallout => {
            // Body ellipse in the top 80%, with the tail leaving it between two angles.
            let (cx, cy, rx, ry) = (0.5, 0.4, 0.5, 0.4);
            let (a0, a1) = (100f64.to_radians(), 125f64.to_radians());
            let n = 96;
            let pts: Vec<[f64; 2]> = (0..=n)
                .map(|i| a1 + (a0 + std::f64::consts::TAU - a1) * i as f64 / n as f64)
                .map(|a| [cx + rx * a.cos(), cy + ry * a.sin()])
                .chain(std::iter::once([0.15, 1.0]))
                .collect();
            pen.polygon(&pts);
        }
        S::CloudCallout => {
            // Scalloped ellipse: bulges between points on an ellipse, plus two thought
            // bubbles towards the bottom left.
            let (cx, cy, rx, ry) = (0.5, 0.38, 0.32, 0.24);
            let n = 10;
            let at = |k: usize| {
                let a = k as f64 * std::f64::consts::TAU / n as f64;
                [cx + rx * a.cos(), cy + ry * a.sin()]
            };
            let start = at(0);
            pen.move_to(start[0], start[1]);
            for k in 0..n {
                let (p, q) = (at(k), at(k + 1));
                let mid = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
                let out = [(mid[0] - cx) * 0.35, (mid[1] - cy) * 0.35];
                let c = [mid[0] + out[0] * 1.6, mid[1] + out[1] * 1.6];
                // A quadratic bulge as a cubic.
                let c1 = [
                    p[0] + (c[0] - p[0]) * 2.0 / 3.0,
                    p[1] + (c[1] - p[1]) * 2.0 / 3.0,
                ];
                let c2 = [
                    q[0] + (c[0] - q[0]) * 2.0 / 3.0,
                    q[1] + (c[1] - q[1]) * 2.0 / 3.0,
                ];
                pen.cubic_to([c1[0], c1[1], c2[0], c2[1], q[0], q[1]]);
            }
            pen.b.close();
            pen.ellipse(0.16, 0.8, 0.28, 0.9);
            pen.ellipse(0.04, 0.92, 0.12, 1.0);
        }
        S::LightningBolt => pen.polygon(&fit(&[
            [0.55, 0.0],
            [0.2, 0.55],
            [0.45, 0.55],
            [0.3, 1.0],
            [0.85, 0.4],
            [0.58, 0.4],
            [0.78, 0.0],
        ])),
        S::CheckMark => pen.polygon(&[
            [0.0, 0.55],
            [0.15, 0.4],
            [0.38, 0.63],
            [0.85, 0.0],
            [1.0, 0.13],
            [0.38, 0.95],
        ]),
        S::Multiply => {
            let t = 0.14;
            pen.polygon(&[
                [t, 0.0],
                [0.5, 0.5 - t],
                [1.0 - t, 0.0],
                [1.0, t],
                [0.5 + t, 0.5],
                [1.0, 1.0 - t],
                [1.0 - t, 1.0],
                [0.5, 0.5 + t],
                [t, 1.0],
                [0.0, 1.0 - t],
                [0.5 - t, 0.5],
                [0.0, t],
            ]);
        }
        S::Gear => {
            let teeth = 8;
            let pts: Vec<[f64; 2]> = (0..teeth * 4)
                .map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / (teeth * 4) as f64;
                    let r = if k % 4 < 2 { 0.5 } else { 0.38 };
                    [0.5 + r * a.cos(), 0.5 + r * a.sin()]
                })
                .collect();
            pen.polygon(&pts);
            pen.ellipse(0.35, 0.35, 0.65, 0.65);
        }
        S::Heart => {
            pen.move_to(0.5, 0.25);
            pen.cubic_to([0.5, 0.0, 0.0, 0.0, 0.0, 0.32]);
            pen.cubic_to([0.0, 0.62, 0.35, 0.78, 0.5, 1.0]);
            pen.cubic_to([0.65, 0.78, 1.0, 0.62, 1.0, 0.32]);
            pen.cubic_to([1.0, 0.0, 0.5, 0.0, 0.5, 0.25]);
            pen.b.close();
        }
    }
    pen.b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_builds_and_fits_its_box() {
        let map = |p: [f64; 2]| [10.0 + p[0] * 200.0, 20.0 + p[1] * 100.0];
        for s in ShapeType::ALL {
            let path = shape_path(s, [200.0, 100.0], 20.0, &map).unwrap_or_else(|| panic!("{s:?}"));
            let b = path.bounds();
            assert!(b.left() >= 9.9 && b.right() <= 210.1, "{s:?} {b:?}");
            assert!(b.top() >= 19.0 && b.bottom() <= 120.1, "{s:?} {b:?}");
            assert!(
                b.width() > 150.0 && b.height() > 80.0,
                "{s:?} fills its box: {b:?}"
            );
        }
    }

    #[test]
    fn cycling_wraps_both_ways() {
        assert_eq!(ShapeType::Rectangle.cycle(false), ShapeType::Heart);
        assert_eq!(ShapeType::Heart.cycle(true), ShapeType::Rectangle);
        assert_eq!(
            ShapeType::Rectangle.cycle(true),
            ShapeType::RoundedRectangle
        );
    }
}
