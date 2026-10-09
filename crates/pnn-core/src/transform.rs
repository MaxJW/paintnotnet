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

#[cfg(test)]
mod tests {
    use super::*;
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
