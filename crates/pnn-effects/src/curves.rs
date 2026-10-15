//! Curves and Levels settings and their renderers.
//!
//! Ported from paint.net 3.36 (MIT, see NOTICE): `Core/SplineInterpolator.cs` (Numerical
//! Recipes natural cubic spline), `Effects/CurvesEffectConfigToken.cs` (tables from integer
//! control points, default (0, 0)–(255, 255)), `UnaryPixelOps.LuminosityCurve` /
//! `ChannelCurve`, `Core/CurveControl.cs` (points keyed by input value, grab radius √30 in
//! curve units), and `UnaryPixelOps.Level` for Levels. Differences: Curves' end points move
//! only up and down here (3.36 let a drag sideways from an end point add a point instead).

use std::collections::BTreeMap;

use pnn_core::{ColorBgra, Rect, Surface};

use crate::adjustments::level_curve;
use crate::framework::{ChannelCurve, Renderer, intensity};

/// Control points: input → output, both 0..=255.
pub type Points = BTreeMap<u8, u8>;

fn identity() -> Points {
    Points::from([(0, 0), (255, 255)])
}

/// 3.36 `SplineInterpolator`: natural cubic spline through `points`, sampled at 0..=255 and
/// clamped to a byte.
pub fn spline_table(points: &Points) -> [u8; 256] {
    let xs: Vec<f64> = points.keys().map(|&x| x as f64).collect();
    let ys: Vec<f64> = points.values().map(|&y| y as f64).collect();
    let n = xs.len();
    if n == 0 {
        return std::array::from_fn(|i| i as u8);
    }
    if n == 1 {
        return [ys[0] as u8; 256];
    }
    // PreCompute.
    let mut y2 = vec![0.0; n];
    let mut u = vec![0.0; n];
    for i in 1..n - 1 {
        let wx = xs[i + 1] - xs[i - 1];
        let sig = (xs[i] - xs[i - 1]) / wx;
        let p = sig * y2[i - 1] + 2.0;
        y2[i] = (sig - 1.0) / p;
        let ddydx =
            (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]) - (ys[i] - ys[i - 1]) / (xs[i] - xs[i - 1]);
        u[i] = (6.0 * ddydx / wx - sig * u[i - 1]) / p;
    }
    y2[n - 1] = 0.0;
    for i in (0..n - 1).rev() {
        y2[i] = y2[i] * y2[i + 1] + u[i];
    }
    // Interpolate (bisection), then `Utility.ClampToByte` (truncating).
    std::array::from_fn(|j| {
        let x = j as f64;
        let (mut lo, mut hi) = (0usize, n - 1);
        while hi - lo > 1 {
            let k = (hi + lo) >> 1;
            if xs[k] > x {
                hi = k;
            } else {
                lo = k;
            }
        }
        let h = xs[hi] - xs[lo];
        let a = (xs[hi] - x) / h;
        let b = (x - xs[lo]) / h;
        let v = a * ys[lo]
            + b * ys[hi]
            + ((a * a * a - a) * y2[lo] + (b * b * b - b) * y2[hi]) * (h * h) / 6.0;
        v.clamp(0.0, 255.0) as u8
    })
}

/// Curves settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Curves {
    /// Luminosity mode (the default) or RGB.
    pub luminosity: bool,
    pub lum: Points,
    /// Red, green, blue.
    pub rgb: [Points; 3],
    /// Which RGB channels a point edit affects.
    pub mask: [bool; 3],
}

impl Default for Curves {
    fn default() -> Self {
        Self {
            luminosity: true,
            lum: identity(),
            rgb: [identity(), identity(), identity()],
            mask: [true; 3],
        }
    }
}

/// 3.36 `UnaryPixelOps.LuminosityCurve`: shift RGB by curve[intensity] − intensity.
struct LuminosityCurve([u8; 256]);

impl Renderer for LuminosityCurve {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            let c = src.get(x0 + i as u32, y);
            let l = intensity(c);
            let d = self.0[l as usize] as i32 - l as i32;
            let ch = |v: u8| (v as i32 + d).clamp(0, 255) as u8;
            *o = ColorBgra::from_bgra(ch(c.b), ch(c.g), ch(c.r), c.a);
        }
    }
}

impl Curves {
    pub fn renderer(&self) -> Box<dyn Renderer> {
        if self.luminosity {
            Box::new(LuminosityCurve(spline_table(&self.lum)))
        } else {
            Box::new(ChannelCurve {
                r: spline_table(&self.rgb[0]),
                g: spline_table(&self.rgb[1]),
                b: spline_table(&self.rgb[2]),
            })
        }
    }

    /// The point lists edited by the current mode and channel mask.
    pub fn edited(&mut self) -> Vec<&mut Points> {
        if self.luminosity {
            vec![&mut self.lum]
        } else {
            let mask = self.mask;
            self.rgb
                .iter_mut()
                .zip(mask)
                .filter(|(_, m)| *m)
                .map(|(p, _)| p)
                .collect()
        }
    }
}

/// 3.36 `CurveControl`: the point of `points` within √30 curve units of (x, y), if any.
pub fn point_near(points: &Points, x: i32, y: i32) -> Option<u8> {
    points
        .iter()
        .map(|(&px, &py)| (px, (px as i32 - x).pow(2) + (py as i32 - y).pow(2)))
        .filter(|&(_, d)| d < 30)
        .min_by_key(|&(_, d)| d)
        .map(|(px, _)| px)
}

/// Move the point keyed `from` to (x, y), keeping the end points' inputs fixed. Returns the
/// point's new key.
pub fn move_point(points: &mut Points, from: u8, x: u8, y: u8) -> u8 {
    if from == 0 || from == 255 {
        points.insert(from, y);
        return from;
    }
    let x = x.clamp(1, 254);
    points.remove(&from);
    points.insert(x, y);
    x
}

/// Levels settings, per channel in blue, green, red order (as `ColorBgra`'s indexer and
/// 3.36's `Level`).
#[derive(Clone, Debug, PartialEq)]
pub struct Levels {
    pub in_lo: [u8; 3],
    pub in_hi: [u8; 3],
    pub gamma: [f32; 3],
    pub out_lo: [u8; 3],
    pub out_hi: [u8; 3],
    /// Which channels an edit affects (B, G, R).
    pub mask: [bool; 3],
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            in_lo: [0; 3],
            in_hi: [255; 3],
            gamma: [1.0; 3],
            out_lo: [0; 3],
            out_hi: [255; 3],
            mask: [true; 3],
        }
    }
}

impl Levels {
    pub fn curve(&self) -> ChannelCurve {
        level_curve(self.in_lo, self.in_hi, self.gamma, self.out_lo, self.out_hi)
    }

    pub fn renderer(&self) -> Box<dyn Renderer> {
        Box::new(self.curve())
    }

    /// The Auto button: Auto-Level's settings for `bounds` of `src` (3.36
    /// `HistogramRgb.MakeLevelsAuto`).
    pub fn auto(src: &Surface, bounds: Rect) -> Self {
        let h = Histogram::of(src, bounds);
        let lo = [0, 1, 2].map(|i| h.percentile(i, 0.005));
        let md = [0, 1, 2].map(|i| h.mean(i));
        let hi = [0, 1, 2].map(|i| h.percentile(i, 0.995));
        let gamma = [0, 1, 2].map(|i| {
            if lo[i] < md[i] && md[i] < hi[i] {
                let base = (md[i] - lo[i]) as f64 / (hi[i] - lo[i]) as f64;
                (0.5f64.ln() / base.ln()).clamp(0.1, 10.0) as f32
            } else {
                1.0
            }
        });
        Self {
            in_lo: lo,
            in_hi: hi,
            gamma,
            ..Self::default()
        }
    }
}

/// Per-channel (B, G, R) counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram(pub [[u64; 256]; 3]);

impl Histogram {
    /// Rows in parallel (Levels opens with it; one pass over a 24 MP photo took ~40 ms).
    pub fn of(src: &Surface, r: Rect) -> Self {
        use rayon::prelude::*;
        let h = (r.y..r.y + r.height)
            .into_par_iter()
            .fold(
                || Box::new([[0u64; 256]; 3]),
                |mut h, y| {
                    for x in r.x..r.x + r.width {
                        let c = src.get(x, y);
                        h[0][c.b as usize] += 1;
                        h[1][c.g as usize] += 1;
                        h[2][c.r as usize] += 1;
                    }
                    h
                },
            )
            .reduce(
                || Box::new([[0u64; 256]; 3]),
                |mut a, b| {
                    for (ac, bc) in a.iter_mut().zip(b.iter()) {
                        for (x, y) in ac.iter_mut().zip(bc) {
                            *x += y;
                        }
                    }
                    a
                },
            );
        Self(*h)
    }

    /// The histogram after a per-channel table.
    pub fn mapped(&self, t: &ChannelCurve) -> Self {
        let mut h = [[0u64; 256]; 3];
        for (c, table) in [&t.b, &t.g, &t.r].into_iter().enumerate() {
            for v in 0..256 {
                h[c][table[v] as usize] += self.0[c][v];
            }
        }
        Self(h)
    }

    /// 3.36 `Histogram.GetPercentile`.
    pub fn percentile(&self, c: usize, f: f32) -> u8 {
        let sum: u64 = self.0[c].iter().sum();
        let mut integral = 0u64;
        for (j, &n) in self.0[c].iter().enumerate() {
            integral += n;
            if integral as f32 > sum as f32 * f {
                return j as u8;
            }
        }
        0
    }

    /// 3.36 `HistogramRgb.GetMeanColor`, one channel.
    pub fn mean(&self, c: usize) -> u8 {
        let sum: u64 = self.0[c].iter().sum();
        let avg: u64 = self.0[c]
            .iter()
            .enumerate()
            .map(|(j, &n)| j as u64 * n)
            .sum();
        let m = if sum == 0 {
            0.0
        } else {
            avg as f32 / sum as f32
        };
        (m + 0.5) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_spline_through_points() {
        let t = spline_table(&identity());
        assert!(t.iter().enumerate().all(|(i, &v)| v == i as u8));
        let p = Points::from([(0, 0), (128, 200), (255, 255)]);
        let t = spline_table(&p);
        assert_eq!(t[128], 200, "passes through its point");
        assert!(t[64] > 64 && t[200] > 200, "lifted around it");
        let inv = spline_table(&Points::from([(0, 255), (255, 0)]));
        assert_eq!((inv[0], inv[255], inv[100]), (255, 0, 155));
    }

    #[test]
    fn point_editing() {
        let mut p = identity();
        assert_eq!(point_near(&p, 2, 3), Some(0));
        assert_eq!(point_near(&p, 128, 128), None);
        p.insert(100, 150);
        let k = move_point(&mut p, 100, 120, 160);
        assert_eq!((k, p.get(&120)), (120, Some(&160)));
        assert!(!p.contains_key(&100));
        assert_eq!(
            move_point(&mut p, 255, 200, 230),
            255,
            "end point keeps its input"
        );
        assert_eq!(p.get(&255), Some(&230));
    }

    #[test]
    fn luminosity_and_rgb_modes() {
        let src = {
            let mut s = Surface::new_transparent(1, 1);
            s.set(0, 0, ColorBgra::from_rgba(100, 100, 100, 255));
            s
        };
        let mut c = Curves {
            lum: Points::from([(0, 0), (100, 150), (255, 255)]),
            ..Curves::default()
        };
        let mut out = [ColorBgra::TRANSPARENT];
        c.renderer().render_row(&src, 0, 0, &mut out);
        assert_eq!(out[0].r, 150);
        c.luminosity = false;
        c.mask = [true, false, false];
        for p in c.edited() {
            p.insert(100, 30);
        }
        c.renderer().render_row(&src, 0, 0, &mut out);
        assert_eq!((out[0].r, out[0].g, out[0].b), (30, 100, 100), "red only");
    }

    #[test]
    fn levels_and_auto() {
        let mut s = Surface::new_transparent(100, 1);
        for x in 0..100 {
            let v = 60 + x as u8; // 60..=159
            s.set(x, 0, ColorBgra::from_rgba(v, v, v, 255));
        }
        let r = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 1,
        };
        let l = Levels::auto(&s, r);
        assert_eq!((l.in_lo[0], l.in_hi[0]), (60, 159));
        let curve = l.curve();
        assert_eq!((curve.r[60], curve.r[159]), (0, 255));
        let h = Histogram::of(&s, r).mapped(&curve);
        assert_eq!(h.0[2][0], 1, "output histogram spreads to 0");
        let m = Levels {
            gamma: [2.0; 3],
            ..Levels::default()
        };
        assert!(
            m.curve().g[128] < 128,
            "gamma 2 darkens mid-tones (3.36 pow(v, γ))"
        );
    }
}
