//! Distance-field engine: exact Euclidean distance transform and the shaping functions behind
//! feather (and later outline, bevel and shadows).
//!
//! Feathering here is a function of distance to the selection edge, not a blur: the falloff
//! has the same width all the way round, corners stay true, nothing bleeds outside the ramp,
//! and because the distance field doesn't depend on the radius, changing the radius only
//! re-shapes an existing field (cheap enough for a live slider).
//!
//! Operates on plain buffers so it has no dependency on the document model.

use rayon::prelude::*;

/// Shape of the falloff across the feather width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Profile {
    /// Smoothstep: soft at both ends (default).
    #[default]
    Smooth,
    Linear,
    /// Error-function curve, like a Gaussian-blurred edge but confined to the width.
    Gaussian,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Self::Smooth, Self::Linear, Self::Gaussian];
    pub fn name(self) -> &'static str {
        match self {
            Self::Smooth => "Smooth",
            Self::Linear => "Linear",
            Self::Gaussian => "Gaussian",
        }
    }

    /// Map `t` in 0..=1 (0 = outer end of the ramp, 1 = fully selected) to coverage.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Smooth => t * t * (3.0 - 2.0 * t),
            Self::Gaussian => {
                const K: f32 = 4.0;
                let e = erf((t - 0.5) * K);
                let max = erf(0.5 * K);
                (e / max + 1.0) / 2.0
            }
        }
    }
}

/// Where the soft edge sits relative to the selection outline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Position {
    /// The softening eats inward; nothing outside the outline is selected (default).
    #[default]
    Inside,
    /// Half in, half out.
    Centered,
    /// The softening grows outward; everything inside stays fully selected.
    Outside,
}

impl Position {
    pub const ALL: [Position; 3] = [Self::Inside, Self::Centered, Self::Outside];
    pub fn name(self) -> &'static str {
        match self {
            Self::Inside => "Inside",
            Self::Centered => "Centered",
            Self::Outside => "Outside",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Feather {
    /// Width of the soft edge in pixels (0 = hard edge).
    pub radius: f32,
    pub profile: Profile,
    pub position: Position,
}

impl Feather {
    pub const NONE: Feather = Feather {
        radius: 0.0,
        profile: Profile::Smooth,
        position: Position::Inside,
    };

    pub fn is_none(&self) -> bool {
        self.radius <= 0.0
    }

    /// How far beyond the outline coverage can reach (pixels).
    pub fn outward_extent(&self) -> f32 {
        match self.position {
            Position::Inside => 0.0,
            Position::Centered => self.radius / 2.0,
            Position::Outside => self.radius,
        }
    }

    /// Signed distance of the 50% contour from the outline (negative = inside). The
    /// marching ants are drawn here.
    pub fn half_contour_offset(&self) -> f32 {
        match self.position {
            Position::Inside => -self.radius / 2.0,
            Position::Centered => 0.0,
            Position::Outside => self.radius / 2.0,
        }
    }

    /// Coverage (0..=1) at signed distance `sd` from the outline (positive = outside).
    pub fn coverage(&self, sd: f32) -> f32 {
        let r = self.radius.max(1e-6);
        let depth = -sd + self.outward_extent();
        self.profile.apply(depth / r)
    }
}

/// Abramowitz–Stegun 7.1.26 (max error 1.5e-7).
fn erf(x: f32) -> f32 {
    let s = x.signum();
    let x = x.abs() as f64;
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-x * x).exp();
    s * y as f32
}

const INF: f32 = 1e20;

/// Felzenszwalb–Huttenlocher 1D squared distance transform of `f` (in place).
fn edt_1d(f: &mut [f32], v: &mut [usize], z: &mut [f32], d: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        let mut s;
        loop {
            let p = v[k];
            s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32))
                / (2.0 * q as f32 - 2.0 * p as f32);
            if s <= z[k] {
                if k == 0 {
                    break;
                }
                k -= 1;
            } else {
                break;
            }
        }
        if s <= z[k] {
            // k == 0 and the new parabola dominates everything so far.
            v[0] = q;
            z[0] = -INF;
            z[1] = INF;
            k = 0;
            continue;
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = INF;
    }
    k = 0;
    for (q, dst) in d[..n].iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f32 - p as f32;
        *dst = dq * dq + f[p];
    }
    f.copy_from_slice(&d[..n]);
}

/// Exact squared Euclidean distance from each pixel centre to the nearest `seed` pixel centre
/// (0 on seeds, a large value if there are none).
pub fn squared_distance(seed: &[bool], width: usize, height: usize) -> Vec<f32> {
    let mut g: Vec<f32> = seed.iter().map(|&s| if s { 0.0 } else { INF }).collect();
    // Columns, then rows.
    let mut cols: Vec<Vec<f32>> = (0..width)
        .into_par_iter()
        .map(|x| {
            let mut f: Vec<f32> = (0..height).map(|y| g[y * width + x]).collect();
            let (mut v, mut z, mut d) = (
                vec![0usize; height],
                vec![0f32; height + 1],
                vec![0f32; height],
            );
            edt_1d(&mut f, &mut v, &mut z, &mut d);
            f
        })
        .collect();
    for (x, col) in cols.iter_mut().enumerate() {
        for (y, &val) in col.iter().enumerate() {
            g[y * width + x] = val;
        }
    }
    g.par_chunks_mut(width).for_each(|row| {
        let (mut v, mut z, mut d) = (
            vec![0usize; width],
            vec![0f32; width + 1],
            vec![0f32; width],
        );
        edt_1d(row, &mut v, &mut z, &mut d);
    });
    g
}

/// Signed distance (pixels) from each pixel centre to the edge of a coverage mask:
/// negative inside, positive outside. Partially covered (antialiased) pixels place the edge
/// at sub-pixel precision.
#[derive(Clone, Debug)]
pub struct SignedField {
    pub width: usize,
    pub height: usize,
    pub sd: Vec<f32>,
}

impl SignedField {
    /// `outside_border`: treat everything beyond the buffer as outside (the edge of the
    /// buffer is an edge). Otherwise the border is ignored.
    pub fn from_coverage(
        coverage: &[u8],
        width: usize,
        height: usize,
        outside_border: bool,
    ) -> Self {
        let (w, h) = if outside_border {
            (width + 2, height + 2)
        } else {
            (width, height)
        };
        let pad = usize::from(outside_border);
        let at = |x: usize, y: usize| -> u8 {
            if outside_border {
                if x == 0 || y == 0 || x > width || y > height {
                    0
                } else {
                    coverage[(y - 1) * width + (x - 1)]
                }
            } else {
                coverage[y * width + x]
            }
        };
        let inside: Vec<bool> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| at(x, y) >= 128)
            .collect();
        let outside: Vec<bool> = inside.iter().map(|b| !b).collect();
        let d_in = squared_distance(&inside, w, h);
        let d_out = squared_distance(&outside, w, h);
        let mut sd = vec![0f32; width * height];
        sd.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
            for (x, s) in row.iter_mut().enumerate() {
                let (gx, gy) = (x + pad, y + pad);
                let i = gy * w + gx;
                let c = at(gx, gy);
                *s = if c > 0 && c < 255 {
                    // Edge pixel: coverage says where the edge crosses it.
                    0.5 - c as f32 / 255.0
                } else if inside[i] {
                    -(d_out[i].sqrt() - 0.5)
                } else {
                    d_in[i].sqrt() - 0.5
                };
            }
        });
        Self { width, height, sd }
    }

    /// Coverage (0..=255) for a feather. Radius 0 is not handled here (callers keep the
    /// original hard mask so feather 0 is pixel-identical).
    pub fn shape(&self, feather: &Feather) -> Vec<u8> {
        self.sd
            .par_iter()
            .map(|&s| (feather.coverage(s) * 255.0 + 0.5) as u8)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: usize, inset: usize) -> Vec<u8> {
        let mut c = vec![0u8; size * size];
        for y in inset..size - inset {
            for x in inset..size - inset {
                c[y * size + x] = 255;
            }
        }
        c
    }

    #[test]
    fn edt_is_exact_euclidean() {
        let (w, h) = (9, 7);
        let mut seed = vec![false; w * h];
        seed[3 * w + 4] = true;
        let d = squared_distance(&seed, w, h);
        for y in 0..h {
            for x in 0..w {
                let expect = (x as f32 - 4.0).powi(2) + (y as f32 - 3.0).powi(2);
                assert_eq!(d[y * w + x], expect, "({x},{y})");
            }
        }
    }

    #[test]
    fn signed_field_sign_and_magnitude() {
        let f = SignedField::from_coverage(&square(20, 5), 20, 20, false);
        // Pixel (5,10) is the first inside column: centre 0.5 px inside.
        assert_eq!(f.sd[10 * 20 + 5], -0.5);
        assert_eq!(f.sd[10 * 20 + 4], 0.5);
        assert_eq!(f.sd[10 * 20 + 8], -3.5);
        assert_eq!(f.sd[10 * 20], 4.5);
    }

    #[test]
    fn golden_square_linear_inside() {
        // Linear inside feather r=8: coverage = depth / 8, where depth = distance of the pixel
        // centre inside the edge.
        let f = SignedField::from_coverage(&square(64, 16), 64, 64, false);
        let c = f.shape(&Feather {
            radius: 8.0,
            profile: Profile::Linear,
            position: Position::Inside,
        });
        for (x, depth) in [
            (16usize, 0.5f32),
            (17, 1.5),
            (19, 3.5),
            (23, 7.5),
            (24, 8.5),
            (32, 16.5),
        ] {
            let expect = ((depth / 8.0).min(1.0) * 255.0 + 0.5) as u8;
            assert_eq!(c[32 * 64 + x], expect, "x={x}");
        }
        assert_eq!(c[32 * 64 + 15], 0, "nothing outside with Inside");
    }

    #[test]
    fn golden_positions() {
        let f = SignedField::from_coverage(&square(64, 16), 64, 64, false);
        let mk = |position| Feather {
            radius: 8.0,
            profile: Profile::Linear,
            position,
        };
        let outside = f.shape(&mk(Position::Outside));
        assert_eq!(outside[32 * 64 + 16], 255, "inside stays full");
        assert!(outside[32 * 64 + 12] > 0, "grows outward");
        assert_eq!(outside[32 * 64 + 5], 0, "beyond the radius");
        let centered = f.shape(&mk(Position::Centered));
        // Edge pixel centre is 0.5 inside: depth 4.5 of 8 → just over half.
        assert_eq!(centered[32 * 64 + 16], ((4.5 / 8.0) * 255.0 + 0.5) as u8);
    }

    #[test]
    fn golden_ring_and_thin_line() {
        // Ring: 40×40 square minus a 10×10 hole; the hole's edge feathers too.
        let mut c = square(60, 10);
        for y in 25..35 {
            for x in 25..35 {
                c[y * 60 + x] = 0;
            }
        }
        for r in [1.0f32, 8.0, 64.0] {
            let f = SignedField::from_coverage(&c, 60, 60, false);
            let s = f.shape(&Feather {
                radius: r,
                profile: Profile::Smooth,
                position: Position::Inside,
            });
            assert_eq!(s[30 * 60 + 30], 0, "r={r}: hole stays empty");
            assert_eq!(s[30 * 60 + 5], 0, "r={r}: outside stays empty");
            // Symmetric: same depth from outer and hole edges gives the same coverage.
            assert_eq!(s[30 * 60 + 12], s[30 * 60 + 22], "r={r}");
        }
        // A 1-px line with an 8-px inside feather is barely there.
        let mut line = vec![0u8; 20 * 20];
        for x in 2..18 {
            line[10 * 20 + x] = 255;
        }
        let f = SignedField::from_coverage(&line, 20, 20, false);
        let s = f.shape(&Feather {
            radius: 8.0,
            profile: Profile::Linear,
            position: Position::Inside,
        });
        assert_eq!(s[10 * 20 + 10], ((0.5 / 8.0) * 255.0 + 0.5) as u8);
    }

    #[test]
    fn antialiased_edges_are_continuous() {
        let mut c = square(20, 5);
        c[10 * 20 + 4] = 128; // half-covered pixel just outside
        let f = SignedField::from_coverage(&c, 20, 20, false);
        assert!((f.sd[10 * 20 + 4] - (0.5 - 128.0 / 255.0)).abs() < 1e-6);
    }

    #[test]
    fn border_counts_as_outside_when_asked() {
        let full = vec![255u8; 10 * 10];
        let f = SignedField::from_coverage(&full, 10, 10, true);
        assert_eq!(f.sd[5 * 10], -0.5, "left edge pixel is 0.5 from the border");
        let g = SignedField::from_coverage(&full, 10, 10, false);
        assert!(g.sd[5 * 10] < -1000.0, "no edge anywhere");
    }

    #[test]
    fn profiles_are_monotonic_and_bounded() {
        for p in Profile::ALL {
            let mut last = 0.0;
            for i in 0..=100 {
                let v = p.apply(i as f32 / 100.0);
                assert!((0.0..=1.0).contains(&v) && v + 1e-6 >= last, "{p:?}");
                last = v;
            }
            assert!(p.apply(0.0) < 1e-3 && p.apply(1.0) > 0.999, "{p:?} ends");
        }
    }
}
