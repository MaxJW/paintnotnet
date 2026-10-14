//! Whole-surface transforms: flip, rotate, canvas resize, resample, merge down.
//!
//! These rewrite every pixel, so they work on a flat buffer and re-tile afterwards
//! (transparent tiles are dropped again by `compact`).

use rayon::prelude::*;

use crate::{ColorBgra, Layer, Surface};

fn rebuild(width: u32, height: u32, pixels: &[ColorBgra]) -> Surface {
    let mut s = Surface::new_transparent(width, height);
    s.write_pixels(pixels);
    s.compact();
    s
}

pub fn flip_horizontal(s: &Surface) -> Surface {
    let (w, h) = (s.width() as usize, s.height() as usize);
    let mut px = s.to_pixels();
    px.par_chunks_mut(w).for_each(|row| row.reverse());
    rebuild(w as u32, h as u32, &px)
}

pub fn flip_vertical(s: &Surface) -> Surface {
    let (w, h) = (s.width() as usize, s.height() as usize);
    let src = s.to_pixels();
    let mut px = vec![ColorBgra::TRANSPARENT; w * h];
    px.par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| row.copy_from_slice(&src[(h - 1 - y) * w..(h - y) * w]));
    rebuild(w as u32, h as u32, &px)
}

/// Rotate by quarter turns clockwise (1 = 90° CW, 2 = 180°, 3 = 90° CCW).
pub fn rotate_quarter(s: &Surface, turns: u8) -> Surface {
    let (w, h) = (s.width() as usize, s.height() as usize);
    let src = s.to_pixels();
    match turns % 4 {
        0 => s.clone(),
        2 => {
            let mut px = src;
            px.reverse();
            rebuild(w as u32, h as u32, &px)
        }
        t => {
            // New image is h × w.
            let (nw, nh) = (h, w);
            let mut px = vec![ColorBgra::TRANSPARENT; nw * nh];
            px.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    // CW: dst(x, y) = src(y, h-1-x); CCW: dst(x, y) = src(w-1-y, x)
                    *p = if t == 1 {
                        src[(h - 1 - x) * w + y]
                    } else {
                        src[x * w + (w - 1 - y)]
                    };
                }
            });
            rebuild(nw as u32, nh as u32, &px)
        }
    }
}

/// Where existing pixels sit when the canvas grows or shrinks (3×3 anchor grid).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Anchor; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    /// (0, 1 or 2) on each axis.
    fn factors(self) -> (i64, i64) {
        let i = Self::ALL.iter().position(|&a| a == self).unwrap_or(4) as i64;
        (i % 3, i / 3)
    }

    /// Top-left of the old image inside the new canvas.
    pub fn offset(self, old: (u32, u32), new: (u32, u32)) -> (i64, i64) {
        let (fx, fy) = self.factors();
        let dx = new.0 as i64 - old.0 as i64;
        let dy = new.1 as i64 - old.1 as i64;
        (dx * fx / 2, dy * fy / 2)
    }
}

/// Canvas size: new area filled with `fill`.
pub fn canvas_resize(
    s: &Surface,
    width: u32,
    height: u32,
    anchor: Anchor,
    fill: ColorBgra,
) -> Surface {
    let (ow, oh) = (s.width(), s.height());
    let (ox, oy) = anchor.offset((ow, oh), (width, height));
    let src = s.to_pixels();
    let mut px = vec![fill; width as usize * height as usize];
    px.par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let sy = y as i64 - oy;
            if sy < 0 || sy >= oh as i64 {
                return;
            }
            for (x, p) in row.iter_mut().enumerate() {
                let sx = x as i64 - ox;
                if sx >= 0 && sx < ow as i64 {
                    *p = src[sy as usize * ow as usize + sx as usize];
                }
            }
        });
    rebuild(width, height, &px)
}

/// paint.net 5's Resize resampling modes (`ImageMenu.html`). paint.net doesn't document its
/// kernels; these map each mode onto a standard filter (see `filter`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Resampling {
    #[default]
    Bicubic,
    BicubicSmooth,
    Bilinear,
    BilinearLowQuality,
    AdaptiveSharp,
    Lanczos,
    Fant,
    NearestNeighbor,
}

impl Resampling {
    pub const ALL: [Resampling; 8] = [
        Self::Bicubic,
        Self::BicubicSmooth,
        Self::Bilinear,
        Self::BilinearLowQuality,
        Self::AdaptiveSharp,
        Self::Lanczos,
        Self::Fant,
        Self::NearestNeighbor,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Bicubic => "Bicubic",
            Self::BicubicSmooth => "Bicubic (Smooth)",
            Self::Bilinear => "Bilinear",
            Self::BilinearLowQuality => "Bilinear (Low Quality)",
            Self::AdaptiveSharp => "Adaptive (Sharp)",
            Self::Lanczos => "Lanczos",
            Self::Fant => "Fant",
            Self::NearestNeighbor => "Nearest Neighbor",
        }
    }
}

#[derive(Clone, Copy)]
enum Filter {
    Box,
    Triangle,
    CatmullRom,
    BSpline,
    Lanczos3,
}

impl Filter {
    fn support(self) -> f64 {
        match self {
            Self::Box => 0.5,
            Self::Triangle => 1.0,
            Self::CatmullRom | Self::BSpline => 2.0,
            Self::Lanczos3 => 3.0,
        }
    }

    fn weight(self, x: f64) -> f64 {
        let x = x.abs();
        match self {
            Self::Box => {
                if x <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Triangle => (1.0 - x).max(0.0),
            Self::CatmullRom => {
                if x < 1.0 {
                    1.5 * x * x * x - 2.5 * x * x + 1.0
                } else if x < 2.0 {
                    -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
                } else {
                    0.0
                }
            }
            Self::BSpline => {
                if x < 1.0 {
                    (3.0 * x * x * x - 6.0 * x * x + 4.0) / 6.0
                } else if x < 2.0 {
                    let t = 2.0 - x;
                    t * t * t / 6.0
                } else {
                    0.0
                }
            }
            Self::Lanczos3 => {
                if x < 1e-9 {
                    1.0
                } else if x < 3.0 {
                    let px = std::f64::consts::PI * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                } else {
                    0.0
                }
            }
        }
    }
}

/// (filter, widen support when shrinking).
fn filter(mode: Resampling, shrinking: bool) -> (Filter, bool) {
    match mode {
        Resampling::Bicubic => (Filter::CatmullRom, true),
        Resampling::BicubicSmooth => (Filter::BSpline, true),
        Resampling::Bilinear => (Filter::Triangle, true),
        Resampling::BilinearLowQuality => (Filter::Triangle, false),
        // Lanczos when shrinking keeps detail crisp; Catmull-Rom when enlarging.
        Resampling::AdaptiveSharp => (
            if shrinking {
                Filter::Lanczos3
            } else {
                Filter::CatmullRom
            },
            true,
        ),
        Resampling::Lanczos => (Filter::Lanczos3, true),
        // Fant: area-weighted averaging.
        Resampling::Fant => (Filter::Box, true),
        Resampling::NearestNeighbor => unreachable!("handled separately"),
    }
}

/// Per output pixel: (first source index, weights).
fn contributions(src: usize, dst: usize, mode: Resampling) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f64 / dst as f64;
    let (f, widen) = filter(mode, scale > 1.0);
    let fscale = if widen { scale.max(1.0) } else { 1.0 };
    let support = f.support() * fscale;
    (0..dst)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale;
            let lo = ((center - support).floor() as i64).max(0) as usize;
            let hi = ((center + support).ceil() as i64).min(src as i64) as usize;
            let mut w: Vec<f64> = (lo..hi)
                .map(|j| f.weight((j as f64 + 0.5 - center) / fscale))
                .collect();
            let sum: f64 = w.iter().sum();
            if sum.abs() < 1e-12 {
                // Degenerate (tiny box at an edge): nearest source pixel.
                let j = (center as usize).min(src - 1);
                return (j, vec![1.0]);
            }
            w.iter_mut().for_each(|v| *v /= sum);
            (lo, w.into_iter().map(|v| v as f32).collect())
        })
        .collect()
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Resize to `width` × `height`. Filtering runs in premultiplied alpha (no dark fringes),
/// optionally in linear light (`gamma_correct`, paint.net 5's Gamma Correction option).
pub fn resample(
    s: &Surface,
    width: u32,
    height: u32,
    mode: Resampling,
    gamma_correct: bool,
) -> Surface {
    let (sw, sh) = (s.width() as usize, s.height() as usize);
    let (dw, dh) = (width as usize, height as usize);
    let src = s.to_pixels();
    if mode == Resampling::NearestNeighbor {
        let mut px = vec![ColorBgra::TRANSPARENT; dw * dh];
        px.par_chunks_mut(dw).enumerate().for_each(|(y, row)| {
            let sy = ((y as f64 + 0.5) * sh as f64 / dh as f64) as usize;
            for (x, p) in row.iter_mut().enumerate() {
                let sx = ((x as f64 + 0.5) * sw as f64 / dw as f64) as usize;
                *p = src[sy.min(sh - 1) * sw + sx.min(sw - 1)];
            }
        });
        return rebuild(width, height, &px);
    }
    let lut: Vec<f32> = (0..256)
        .map(|i| {
            if gamma_correct {
                srgb_to_linear(i as f32 / 255.0)
            } else {
                i as f32 / 255.0
            }
        })
        .collect();
    // Premultiplied RGBA f32.
    let pre: Vec<[f32; 4]> = src
        .par_iter()
        .map(|p| {
            let a = p.a as f32 / 255.0;
            [
                lut[p.r as usize] * a,
                lut[p.g as usize] * a,
                lut[p.b as usize] * a,
                a,
            ]
        })
        .collect();
    let xs = contributions(sw, dw, mode);
    let mut horiz = vec![[0f32; 4]; dw * sh];
    horiz.par_chunks_mut(dw).enumerate().for_each(|(y, row)| {
        let src_row = &pre[y * sw..(y + 1) * sw];
        for (x, (start, w)) in xs.iter().enumerate() {
            let mut acc = [0f32; 4];
            for (k, &wk) in w.iter().enumerate() {
                let p = src_row[start + k];
                for c in 0..4 {
                    acc[c] += p[c] * wk;
                }
            }
            row[x] = acc;
        }
    });
    let ys = contributions(sh, dh, mode);
    let mut out = vec![ColorBgra::TRANSPARENT; dw * dh];
    out.par_chunks_mut(dw).enumerate().for_each(|(y, row)| {
        let (start, w) = &ys[y];
        for (x, p) in row.iter_mut().enumerate() {
            let mut acc = [0f32; 4];
            for (k, &wk) in w.iter().enumerate() {
                let q = horiz[(start + k) * dw + x];
                for c in 0..4 {
                    acc[c] += q[c] * wk;
                }
            }
            let a = acc[3].clamp(0.0, 1.0);
            let to = |v: f32| {
                let v = if a > 0.0 {
                    (v / a).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let v = if gamma_correct { linear_to_srgb(v) } else { v };
                (v * 255.0 + 0.5) as u8
            };
            *p = ColorBgra::from_rgba(to(acc[0]), to(acc[1]), to(acc[2]), (a * 255.0 + 0.5) as u8);
        }
    });
    rebuild(width, height, &out)
}

/// paint.net's Merge Layer Down: render `top` onto `bottom` with the top layer's blend mode
/// and opacity (`MergeLayerDownFunction`; visibility is not consulted, as in paint.net).
pub fn merge_down(top: &Layer, bottom: &mut Layer) {
    let s = &mut bottom.surface;
    for c in s.tile_coords().collect::<Vec<_>>() {
        let Some(src) = top.surface.tile(c) else {
            continue;
        };
        let dst = s.tile_mut(c);
        top.blend_mode
            .apply_row(dst.pixels_mut(), src.pixels(), top.opacity);
    }
}

/// Flatten into one layer, as Image > Flatten: the composite becomes "Background".
pub fn flatten_layers(doc: &crate::Document) -> Layer {
    let mut surface = crate::flatten(doc);
    surface.compact();
    Layer::new(doc.layer(0).name.clone(), surface)
}

/// Sampling for moved/rotated/scaled pixels (Move Selected Pixels' "Sampling" option).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sampling {
    #[default]
    Bicubic,
    Bilinear,
    NearestNeighbor,
}

/// 2D affine map `p' = [m0·x + m1·y + m2, m3·x + m4·y + m5]`.
pub type Affine = [f64; 6];

pub const IDENTITY: Affine = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

pub fn apply(m: &Affine, p: [f64; 2]) -> [f64; 2] {
    [
        m[0] * p[0] + m[1] * p[1] + m[2],
        m[3] * p[0] + m[4] * p[1] + m[5],
    ]
}

/// `a ∘ b`: apply `b`, then `a`.
pub fn compose(a: &Affine, b: &Affine) -> Affine {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}

pub fn invert(m: &Affine) -> Option<Affine> {
    let det = m[0] * m[4] - m[1] * m[3];
    if det.abs() < 1e-12 {
        return None;
    }
    let (a, b, d, e) = (m[4] / det, -m[1] / det, -m[3] / det, m[0] / det);
    Some([a, b, -(a * m[2] + b * m[5]), d, e, -(d * m[2] + e * m[5])])
}

pub fn translation(dx: f64, dy: f64) -> Affine {
    [1.0, 0.0, dx, 0.0, 1.0, dy]
}

pub fn rotation_about(theta: f64, c: [f64; 2]) -> Affine {
    let (s, co) = theta.sin_cos();
    [
        co,
        -s,
        c[0] - co * c[0] + s * c[1],
        s,
        co,
        c[1] - s * c[0] - co * c[1],
    ]
}

pub fn scale_about(sx: f64, sy: f64, c: [f64; 2]) -> Affine {
    [sx, 0.0, c[0] - sx * c[0], 0.0, sy, c[1] - sy * c[1]]
}

/// Draw `src` (pixels in image coordinates, i.e. `src.get(x, y)` is image pixel (x, y))
/// transformed by `m` onto `dst` with Normal blending, only where `src` has pixels inside
/// `src_rect`. Integer translations copy exactly; anything else resamples.
pub fn draw_transformed(
    dst: &mut Surface,
    src: &Surface,
    src_rect: crate::Rect,
    m: &Affine,
    sampling: Sampling,
) {
    let corners = [
        [src_rect.x as f64, src_rect.y as f64],
        [(src_rect.x + src_rect.width) as f64, src_rect.y as f64],
        [src_rect.x as f64, (src_rect.y + src_rect.height) as f64],
        [
            (src_rect.x + src_rect.width) as f64,
            (src_rect.y + src_rect.height) as f64,
        ],
    ]
    .map(|p| apply(m, p));
    let (dw, dh) = (dst.width() as i64, dst.height() as i64);
    let x0 = corners
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as i64;
    let y0 = corners
        .iter()
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as i64;
    let x1 = (corners
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil() as i64)
        .min(dw);
    let y1 = (corners
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil() as i64)
        .min(dh);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let Some(inv) = invert(m) else { return };
    let integer = m[0] == 1.0
        && m[1] == 0.0
        && m[3] == 0.0
        && m[4] == 1.0
        && m[2].fract() == 0.0
        && m[5].fract() == 0.0;
    let (sx0, sy0) = (src_rect.x as i64, src_rect.y as i64);
    let (sx1, sy1) = (sx0 + src_rect.width as i64, sy0 + src_rect.height as i64);
    let fetch = |x: i64, y: i64| -> [f32; 4] {
        if x < sx0 || y < sy0 || x >= sx1 || y >= sy1 {
            return [0.0; 4];
        }
        let c = src.get(x as u32, y as u32);
        let a = c.a as f32;
        [
            c.r as f32 * a / 255.0,
            c.g as f32 * a / 255.0,
            c.b as f32 * a / 255.0,
            a,
        ]
    };
    // How much the map shrinks: the smaller singular value of its linear part. Shrinking
    // below half size point-sampled aliases (moiré, shimmering edges), so then every output
    // pixel averages `sub` × `sub` samples; Nearest Neighbor stays crisp on purpose.
    let (p, q, r, t) = (m[0], m[1], m[3], m[4]);
    let e = (p * p + q * q + r * r + t * t) / 2.0;
    let det = (p * t - q * r).abs();
    let min_scale = (e - (e * e - det * det).max(0.0).sqrt()).max(0.0).sqrt();
    let sub = if sampling == Sampling::NearestNeighbor || min_scale >= 0.999 {
        1
    } else {
        ((1.0 / min_scale.max(1e-3)).ceil() as usize).clamp(1, 8)
    };
    let rows: Vec<(i64, Vec<ColorBgra>)> = (y0..y1)
        .into_par_iter()
        .map(|y| {
            let mut row = Vec::with_capacity((x1 - x0) as usize);
            for x in x0..x1 {
                if integer {
                    let (sx, sy) = (x - m[2] as i64, y - m[5] as i64);
                    let c = if sx < sx0 || sy < sy0 || sx >= sx1 || sy >= sy1 {
                        ColorBgra::TRANSPARENT
                    } else {
                        src.get(sx as u32, sy as u32)
                    };
                    row.push(c);
                    continue;
                }
                let sample_at = |q: [f64; 2]| -> [f32; 4] {
                    match sampling {
                        Sampling::NearestNeighbor => {
                            fetch(q[0].floor() as i64, q[1].floor() as i64)
                        }
                        Sampling::Bilinear => {
                            let (fx, fy) = (q[0] - 0.5, q[1] - 0.5);
                            let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
                            let (tx, ty) = ((fx - ix as f64) as f32, (fy - iy as f64) as f32);
                            let (a, b, c, d) = (
                                fetch(ix, iy),
                                fetch(ix + 1, iy),
                                fetch(ix, iy + 1),
                                fetch(ix + 1, iy + 1),
                            );
                            std::array::from_fn(|k| {
                                let top = a[k] + (b[k] - a[k]) * tx;
                                let bot = c[k] + (d[k] - c[k]) * tx;
                                top + (bot - top) * ty
                            })
                        }
                        Sampling::Bicubic => {
                            let (fx, fy) = (q[0] - 0.5, q[1] - 0.5);
                            let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
                            let (tx, ty) = (fx - ix as f64, fy - iy as f64);
                            let cr = |t: f64| -> [f32; 4] {
                                // Catmull-Rom weights for offsets -1, 0, 1, 2.
                                let t2 = t * t;
                                let t3 = t2 * t;
                                [
                                    (-0.5 * t3 + t2 - 0.5 * t) as f32,
                                    (1.5 * t3 - 2.5 * t2 + 1.0) as f32,
                                    (-1.5 * t3 + 2.0 * t2 + 0.5 * t) as f32,
                                    (0.5 * t3 - 0.5 * t2) as f32,
                                ]
                            };
                            let (wx, wy) = (cr(tx), cr(ty));
                            let mut acc = [0f32; 4];
                            for (j, wyj) in wy.iter().enumerate() {
                                for (i, wxi) in wx.iter().enumerate() {
                                    let p = fetch(ix + i as i64 - 1, iy + j as i64 - 1);
                                    for k in 0..4 {
                                        acc[k] += p[k] * wxi * wyj;
                                    }
                                }
                            }
                            acc
                        }
                    }
                };
                // Shrinking: average sub × sub samples (sub ≈ the shrink factor) so detail
                // averages out instead of aliasing; one sample per pixel otherwise.
                let acc = if sub == 1 {
                    sample_at(apply(&inv, [x as f64 + 0.5, y as f64 + 0.5]))
                } else {
                    let mut sum = [0f32; 4];
                    for sj in 0..sub {
                        for si in 0..sub {
                            let p = sample_at(apply(
                                &inv,
                                [
                                    x as f64 + (si as f64 + 0.5) / sub as f64,
                                    y as f64 + (sj as f64 + 0.5) / sub as f64,
                                ],
                            ));
                            for k in 0..4 {
                                sum[k] += p[k];
                            }
                        }
                    }
                    sum.map(|v| v / (sub * sub) as f32)
                };
                let a = acc[3].clamp(0.0, 255.0);
                let un = |v: f32| {
                    if a > 0.0 {
                        (v * 255.0 / a).round().clamp(0.0, 255.0) as u8
                    } else {
                        0
                    }
                };
                row.push(ColorBgra::from_rgba(
                    un(acc[0]),
                    un(acc[1]),
                    un(acc[2]),
                    a.round() as u8,
                ));
            }
            (y, row)
        })
        .collect();
    for (y, row) in rows {
        for (i, c) in row.into_iter().enumerate() {
            if c.a == 0 {
                continue;
            }
            let x = (x0 + i as i64) as u32;
            let y = y as u32;
            let below = dst.get(x, y);
            dst.set(x, y, crate::BlendMode::Normal.apply(below, c, 255));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shrinking a one-pixel checkerboard to a quarter must give an even grey: every output
    /// pixel covers 16 source pixels, half black and half white. Point sampling (3.36's way,
    /// and ours before Phase 15) picks single source pixels and keeps black/white aliasing.
    #[test]
    fn shrinking_averages_instead_of_aliasing() {
        let n = 256u32;
        let mut src = Surface::new_transparent(n, n);
        for y in 0..n {
            for x in 0..n {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                src.set(x, y, ColorBgra::from_rgba(v, v, v, 255));
            }
        }
        let rect = crate::Rect {
            x: 0,
            y: 0,
            width: n,
            height: n,
        };
        // Quarter size, slightly rotated so no grid lines up.
        let (c, s) = (0.25 * 0.2f64.cos(), 0.25 * 0.2f64.sin());
        let m: Affine = [c, -s, 64.0, s, c, 64.0];
        for sampling in [Sampling::Bilinear, Sampling::Bicubic] {
            let mut dst = Surface::new_transparent(n, n);
            draw_transformed(&mut dst, &src, rect, &m, sampling);
            let mut worst = 0i32;
            for y in 80..112 {
                for x in 80..112 {
                    worst = worst.max((dst.get(x, y).r as i32 - 128).abs());
                }
            }
            assert!(worst <= 20, "{sampling:?}: grey varies by {worst}");
        }
        let mut dst = Surface::new_transparent(n, n);
        draw_transformed(&mut dst, &src, rect, &m, Sampling::NearestNeighbor);
        let spread = (80..112)
            .flat_map(|y| (80..112).map(move |x| (x, y)))
            .map(|(x, y)| dst.get(x, y).r)
            .fold((255u8, 0u8), |(lo, hi), v| (lo.min(v), hi.max(v)));
        assert_eq!(spread, (0, 255), "Nearest Neighbor stays point-sampled");
    }
    use crate::BlendMode;

    fn numbered(w: u32, h: u32) -> Surface {
        let px: Vec<ColorBgra> = (0..w * h)
            .map(|i| ColorBgra::from_rgba(i as u8, (i >> 8) as u8, 7, 255))
            .collect();
        rebuild(w, h, &px)
    }

    #[test]
    fn flips_and_rotations() {
        let s = numbered(300, 260);
        let f = flip_horizontal(&s);
        assert_eq!(f.get(0, 5), s.get(299, 5));
        let v = flip_vertical(&s);
        assert_eq!(v.get(7, 0), s.get(7, 259));
        let cw = rotate_quarter(&s, 1);
        assert_eq!((cw.width(), cw.height()), (260, 300));
        // Top-left of the CW result is the old bottom-left.
        assert_eq!(cw.get(0, 0), s.get(0, 259));
        assert_eq!(cw.get(259, 0), s.get(0, 0));
        let ccw = rotate_quarter(&s, 3);
        assert_eq!(ccw.get(0, 0), s.get(299, 0));
        assert_eq!(
            rotate_quarter(&rotate_quarter(&s, 1), 3).to_pixels(),
            s.to_pixels()
        );
        assert_eq!(
            rotate_quarter(&rotate_quarter(&s, 1), 1).to_pixels(),
            rotate_quarter(&s, 2).to_pixels()
        );
    }

    #[test]
    fn canvas_resize_anchors() {
        let s = numbered(10, 10);
        let c = canvas_resize(&s, 20, 14, Anchor::Center, ColorBgra::WHITE);
        assert_eq!(c.get(5, 2), s.get(0, 0));
        assert_eq!(c.get(0, 0), ColorBgra::WHITE);
        let br = canvas_resize(&s, 20, 14, Anchor::BottomRight, ColorBgra::TRANSPARENT);
        assert_eq!(br.get(19, 13), s.get(9, 9));
        let shrink = canvas_resize(&s, 4, 4, Anchor::TopLeft, ColorBgra::WHITE);
        assert_eq!(shrink.get(3, 3), s.get(3, 3));
    }

    #[test]
    fn resample_preserves_flat_colour_and_alpha() {
        let mut s = Surface::new_transparent(64, 64);
        s.fill(ColorBgra::from_rgba(200, 100, 50, 128));
        for mode in Resampling::ALL {
            for (w, h) in [(17, 23), (150, 90)] {
                let r = resample(&s, w, h, mode, false);
                let p = r.get(w / 2, h / 2);
                assert!(
                    (p.r as i32 - 200).abs() <= 1
                        && (p.g as i32 - 100).abs() <= 1
                        && (p.a as i32 - 128).abs() <= 1,
                    "{mode:?} {w}x{h}: {p:?}"
                );
            }
        }
    }

    #[test]
    fn premultiplied_resampling_has_no_dark_fringe() {
        // Red next to fully transparent black: the blend must stay red, just less opaque.
        let mut s = Surface::new_transparent(4, 1);
        s.set(0, 0, ColorBgra::from_rgba(255, 0, 0, 255));
        s.set(1, 0, ColorBgra::from_rgba(255, 0, 0, 255));
        let r = resample(&s, 2, 1, Resampling::Bilinear, false);
        let edge = r.get(0, 0);
        assert_eq!((edge.r, edge.g, edge.b), (255, 0, 0));
    }

    #[test]
    fn affine_helpers() {
        let r = rotation_about(std::f64::consts::FRAC_PI_2, [10.0, 10.0]);
        let p = apply(&r, [20.0, 10.0]);
        assert!(
            (p[0] - 10.0).abs() < 1e-9 && (p[1] - 20.0).abs() < 1e-9,
            "{p:?}"
        );
        let m = compose(&translation(5.0, 0.0), &scale_about(2.0, 2.0, [0.0, 0.0]));
        assert_eq!(apply(&m, [1.0, 1.0]), [7.0, 2.0]);
        let inv = invert(&m).unwrap();
        let back = apply(&inv, [7.0, 2.0]);
        assert!((back[0] - 1.0).abs() < 1e-9 && (back[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn draw_transformed_translation_is_exact_and_rotation_samples() {
        let src = numbered(20, 20);
        let r = crate::Rect {
            x: 2,
            y: 2,
            width: 4,
            height: 3,
        };
        let mut dst = Surface::new_transparent(20, 20);
        draw_transformed(
            &mut dst,
            &src,
            r,
            &translation(10.0, 5.0),
            Sampling::Bicubic,
        );
        assert_eq!(dst.get(12, 7), src.get(2, 2));
        assert_eq!(dst.get(15, 9), src.get(5, 4));
        assert_eq!(
            dst.get(16, 9),
            ColorBgra::TRANSPARENT,
            "outside the rect isn't copied"
        );
        // 180° about the rect centre maps (2,2) to (5,4).
        let c = [4.0, 3.5];
        let mut rot = Surface::new_transparent(20, 20);
        draw_transformed(
            &mut rot,
            &src,
            r,
            &rotation_about(std::f64::consts::PI, c),
            Sampling::NearestNeighbor,
        );
        assert_eq!(rot.get(5, 4), src.get(2, 2));
    }

    #[test]
    fn merge_down_uses_top_blend_and_opacity() {
        let mut bottom = Layer::new("b", {
            let mut s = Surface::new_transparent(2, 1);
            s.fill(ColorBgra::WHITE);
            s
        });
        let mut top = Layer::new("t", Surface::new_transparent(2, 1));
        top.surface.set(0, 0, ColorBgra::from_rgba(255, 0, 0, 255));
        top.opacity = 128;
        top.blend_mode = BlendMode::Multiply;
        merge_down(&top, &mut bottom);
        assert_eq!(
            bottom.surface.get(0, 0),
            BlendMode::Multiply.apply(ColorBgra::WHITE, ColorBgra::from_rgba(255, 0, 0, 255), 128)
        );
        assert_eq!(bottom.surface.get(1, 0), ColorBgra::WHITE);
    }
}
