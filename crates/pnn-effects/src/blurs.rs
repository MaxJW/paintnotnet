//! Effects > Blurs.
//!
//! Ported from paint.net 3.36 (MIT, see NOTICE): `Effects/GaussianBlurEffect.cs`,
//! `MotionBlurEffect.cs`, `RadialBlurEffect.cs`, `ZoomBlurEffect.cs`, `SurfaceBlurEffect.cs`,
//! `MedianEffect.cs` + `LocalHistogramEffect.cs`, `FragmentEffect.cs`; helpers from
//! `Core/ColorBgra.cs` (`Blend`, `BlendColors4W16IP`) and `Core/Surface.cs`
//! (`GetBilinearSample`).
//!
//! - Gaussian: 3.36's kernel is a tent (weights 16·(i+1)), alpha-weighted. It's computed here
//!   as two running box sums per axis (a tent is a box convolved with a box), so cost doesn't
//!   grow with the radius; results match 3.36 except for its per-sample integer truncation
//!   (within ±1).
//! - Radial and Zoom take their centre as two "Offset" numbers (3.36 used a pan control).
//! - From paint.net 5's docs only, maths guessed: Square Blur (alpha-weighted box) and Bokeh
//!   Blur (disc average). Sketch Blur is left out (no description of its maths).

use pnn_core::{BlendMode, ColorBgra, Rect, Surface};

use crate::framework::{EffectDef, GpuJob, PropDef, Renderer, Value};

/// 3.36 `ColorBgra.Blend(colors, count)`: alpha-weighted average.
/// [`blend`], or the same average taken in linear light.
fn blend_in(samples: &[ColorBgra], linear: bool) -> ColorBgra {
    if !linear || samples.is_empty() {
        return blend(samples);
    }
    let a_sum: u64 = samples.iter().map(|c| c.a as u64).sum();
    let a = (a_sum / samples.len() as u64) as u8;
    if a_sum == 0 {
        return ColorBgra::from_bgra(0, 0, 0, a);
    }
    let ch = |f: fn(&ColorBgra) -> u8| {
        let sum: i64 = samples
            .iter()
            .map(|c| c.a as i64 * srgb_to_linear16(f(c)))
            .sum();
        linear16_to_srgb(sum / a_sum as i64)
    };
    ColorBgra::from_bgra(ch(|c| c.b), ch(|c| c.g), ch(|c| c.r), a)
}

pub fn blend(samples: &[ColorBgra]) -> ColorBgra {
    if samples.is_empty() {
        return ColorBgra::TRANSPARENT;
    }
    let a_sum: u64 = samples.iter().map(|c| c.a as u64).sum();
    let a = (a_sum / samples.len() as u64) as u8;
    if a_sum == 0 {
        return ColorBgra::from_bgra(0, 0, 0, a);
    }
    let ch = |f: fn(&ColorBgra) -> u8| {
        (samples
            .iter()
            .map(|c| c.a as u64 * f(c) as u64)
            .sum::<u64>()
            / a_sum) as u8
    };
    ColorBgra::from_bgra(ch(|c| c.b), ch(|c| c.g), ch(|c| c.r), a)
}

/// 3.36 `Surface.GetBilinearSample` (transparent outside the surface).
pub fn bilinear(src: &Surface, x: f32, y: f32) -> ColorBgra {
    bilinear_with(|x, y| src.get(x, y), src.width(), src.height(), x, y)
}

/// [`bilinear`] over any pixel source of `width` × `height`.
#[inline]
fn bilinear_with(
    get: impl Fn(u32, u32) -> ColorBgra,
    width: u32,
    height: u32,
    x: f32,
    y: f32,
) -> ColorBgra {
    let (w, h) = (width as f32, height as f32);
    if !(x.is_finite() && y.is_finite()) || x < 0.0 || y < 0.0 || x >= w || y >= h {
        return ColorBgra::TRANSPARENT;
    }
    let (iu, iv) = (x.floor() as u32, y.floor() as u32);
    let fx = (256.0 * (x - iu as f32)) as u32;
    let fy = (256.0 * (y - iv as f32)) as u32;
    let (ix, iy) = (256 - fx, 256 - fy);
    let right = if iu == width - 1 { iu } else { iu + 1 };
    let bottom = if iv == height - 1 { iv } else { iv + 1 };
    let c = [
        (get(iu, iv), ix * iy),
        (get(right, iv), fx * iy),
        (get(iu, bottom), ix * fy),
        (get(right, bottom), fx * fy),
    ];
    // BlendColors4W16IP.
    let af: u64 = c.iter().map(|(p, w)| p.a as u64 * *w as u64).sum();
    let a = ((af + 32768) >> 16) as u8;
    if a == 0 {
        return ColorBgra::from_bgra(0, 0, 0, 0);
    }
    let ch = |f: fn(&ColorBgra) -> u8| {
        (c.iter()
            .map(|(p, w)| p.a as u64 * f(p) as u64 * *w as u64)
            .sum::<u64>()
            / af) as u8
    };
    ColorBgra::from_bgra(ch(|p| p.b), ch(|p| p.g), ch(|p| p.r), a)
}

/// The source pixels a tile's samples can reach (the tile plus `margin`, clipped to the
/// image), copied once: effects that take many samples per pixel then index a flat slice
/// instead of looking up the tiled surface for every sample. Pixels outside the copy fall
/// back to the surface, so results never depend on the margin.
pub(crate) struct Block<'a> {
    src: &'a Surface,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    pixels: Vec<ColorBgra>,
}

impl<'a> Block<'a> {
    pub(crate) fn new(src: &'a Surface, rect: Rect, margin: u32) -> Self {
        let x0 = rect.x.saturating_sub(margin);
        let y0 = rect.y.saturating_sub(margin);
        let x1 = (rect.x + rect.width + margin).min(src.width());
        let y1 = (rect.y + rect.height + margin).min(src.height());
        let (w, h) = (x1 - x0, y1 - y0);
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                pixels.push(src.get(x, y));
            }
        }
        Self {
            src,
            x0,
            y0,
            w,
            h,
            pixels,
        }
    }

    #[inline]
    pub(crate) fn get(&self, x: u32, y: u32) -> ColorBgra {
        let (bx, by) = (x.wrapping_sub(self.x0), y.wrapping_sub(self.y0));
        if bx < self.w && by < self.h {
            self.pixels[(by * self.w + bx) as usize]
        } else {
            self.src.get(x, y)
        }
    }

    #[inline]
    pub(crate) fn bilinear(&self, x: f32, y: f32) -> ColorBgra {
        bilinear_with(
            |x, y| self.get(x, y),
            self.src.width(),
            self.src.height(),
            x,
            y,
        )
    }
}

/// 3.36's weighted 1-D tent (r+1−|d|) of `a` (length L + 2r, index r = first output):
/// out[j] for j in 0..L, via two running box sums. Kept to test against: the Gaussian
/// replaced it.
#[cfg(test)]
pub(crate) fn tent(a: &[i64], r: usize, out: &mut [i64]) {
    let n = a.len();
    let mut s = vec![0i64; n + 1];
    for i in 0..n {
        s[i + 1] = s[i] + a[i];
    }
    // B1[i] = a[i] + … + a[i + r].
    let m = n - r;
    let mut t = vec![0i64; m + 1];
    for i in 0..m {
        let b1 = s[i + r + 1] - s[i];
        t[i + 1] = t[i] + b1;
    }
    for (j, o) in out.iter_mut().enumerate() {
        *o = t[j + r + 1] - t[j];
    }
}

/// Box of width 2r+1 (`a` as in [`tent`]).
fn boxed(a: &[i64], r: usize, out: &mut [i64]) {
    let mut s = vec![0i64; a.len() + 1];
    for i in 0..a.len() {
        s[i + 1] = s[i] + a[i];
    }
    for (j, o) in out.iter_mut().enumerate() {
        *o = s[j + 2 * r + 1] - s[j];
    }
}

/// A separable blur's 1-D kernel.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kernel {
    /// A box of width 2r+1 (Square Blur).
    Box,
    /// Boxes of these radii one after another: a large Gaussian (see [`Separable::gaussian`]).
    Boxes(Vec<usize>),
    /// Explicit integer weights (length 2r+1): a small Gaussian, sampled exactly.
    Weights(Vec<i64>),
}

/// Separable alpha-weighted blur with a 1-D kernel applied on both axes; `r` is the kernel's
/// half-width. `linear` averages colours in linear light instead of sRGB.
pub(crate) struct Separable {
    pub(crate) r: usize,
    pub(crate) kernel: Kernel,
    pub(crate) linear: bool,
}

impl Separable {
    /// Square Blur's box.
    pub(crate) fn square(radius: usize) -> Self {
        Self {
            r: radius,
            kernel: Kernel::Box,
            linear: false,
        }
    }

    /// A true Gaussian, replacing 3.36's tent kernel (weights r+1−|d|, a triangle, which
    /// blurs highlights into diamonds and kinks edges). σ matches the tent's variance at the
    /// same radius, so a radius blurs as much as it did; the Gaussian is approximated by
    /// four successive boxes (Kovesi's widths), so the cost doesn't grow with the radius.
    pub(crate) fn gaussian(radius: usize) -> Self {
        let sigma = tent_sigma(radius);
        if sigma < 0.3 {
            return Self::square(0);
        }
        if sigma <= 8.0 {
            // Small: sample the curve to ±3σ (boxes this narrow can't hit σ closely).
            let r = (3.0 * sigma).ceil() as usize;
            let weights = (0..=2 * r)
                .map(|i| {
                    let d = i as f64 - r as f64;
                    ((-d * d / (2.0 * sigma * sigma)).exp() * 65536.0).round() as i64
                })
                .collect();
            return Self {
                r,
                kernel: Kernel::Weights(weights),
                linear: false,
            };
        }
        let radii = gaussian_boxes(sigma, 4);
        Self {
            r: radii.iter().sum(),
            kernel: Kernel::Boxes(radii),
            linear: false,
        }
    }

    pub(crate) fn linear(mut self, linear: bool) -> Self {
        self.linear = linear;
        self
    }

    /// Apply the kernel to `a` (length `out.len() + 2r`).
    fn apply(&self, a: &[i64], out: &mut [i64]) {
        match &self.kernel {
            Kernel::Box => boxed(a, self.r, out),
            // Normalised per box / pass so sums never grow (four wide boxes on both axes
            // would overflow i64); inputs carry 16 extra bits, so nothing is lost.
            Kernel::Boxes(radii) => {
                let mut cur = a.to_vec();
                for &r in radii {
                    let mut next = vec![0i64; cur.len() - 2 * r];
                    boxed(&cur, r, &mut next);
                    let width = (2 * r + 1) as i64;
                    for v in &mut next {
                        *v = (*v + width / 2).div_euclid(width);
                    }
                    cur = next;
                }
                out.copy_from_slice(&cur);
            }
            Kernel::Weights(w) => {
                let total: i64 = w.iter().sum();
                for (j, o) in out.iter_mut().enumerate() {
                    let sum: i64 = w.iter().zip(&a[j..]).map(|(k, v)| k * v).sum();
                    *o = (sum + total / 2).div_euclid(total);
                }
            }
        }
    }
}

/// Standard deviation of 3.36's tent kernel of `radius` (weights r+1−|d|).
pub(crate) fn tent_sigma(radius: usize) -> f64 {
    let r = radius as i64;
    let (mut sum, mut moment) = (0.0, 0.0);
    for d in -r..=r {
        let w = (r + 1 - d.abs()) as f64;
        sum += w;
        moment += w * (d * d) as f64;
    }
    (moment / sum).sqrt()
}

/// Radii of `n` successive boxes whose combined kernel is closest to a Gaussian of `sigma`
/// (P. Kovesi, "Fast almost-Gaussian filtering", 2010): `m` boxes of the lower odd width,
/// the rest two wider. Empty for no blur.
pub(crate) fn gaussian_boxes(sigma: f64, n: usize) -> Vec<usize> {
    if sigma < 0.3 {
        return Vec::new();
    }
    let nf = n as f64;
    let ideal = (12.0 * sigma * sigma / nf + 1.0).sqrt();
    let mut wl = ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let wlf = wl as f64;
    let m_ideal =
        (12.0 * sigma * sigma - nf * wlf * wlf - 4.0 * nf * wlf - 3.0 * nf) / (-4.0 * wlf - 4.0);
    let m = (m_ideal.round().clamp(0.0, nf)) as usize;
    (0..n)
        .map(|i| (if i < m { wl } else { wu } as usize - 1) / 2)
        .filter(|&r| r > 0)
        .collect()
}

/// sRGB byte → linear light in 0..=65535.
fn srgb_to_linear16(v: u8) -> i64 {
    static LUT: std::sync::OnceLock<[u16; 256]> = std::sync::OnceLock::new();
    LUT.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f64 / 255.0;
            let l = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            (l * 65535.0).round() as u16
        })
    })[v as usize] as i64
}

/// Linear light in 0..=65535 → sRGB byte.
fn linear16_to_srgb(v: i64) -> u8 {
    static LUT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    LUT.get_or_init(|| {
        (0..=65535u32)
            .map(|i| {
                let l = i as f64 / 65535.0;
                let c = if l <= 0.003_130_8 {
                    l * 12.92
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                (c * 255.0).round() as u8
            })
            .collect()
    })[v.clamp(0, 65535) as usize]
}

const CHANNELS: &str = "Channels and blending";

/// Gaussian Blur limited to some channels (the rest keep the source's), then blended onto the
/// source with a blend mode (Normal = replace).
struct ChannelBlur {
    blur: Separable,
    /// Red, green, blue, alpha.
    channels: [bool; 4],
    mode: BlendMode,
}

impl Renderer for ChannelBlur {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let rect = Rect {
            x: x0,
            y,
            width: out.len() as u32,
            height: 1,
        };
        self.render_rect(src, rect, out);
    }

    fn render_rect(&self, src: &Surface, rect: Rect, out: &mut [ColorBgra]) {
        self.blur.render_rect(src, rect, out);
        let [r, g, b, a] = self.channels;
        for (j, row) in out.chunks_mut(rect.width.max(1) as usize).enumerate() {
            for (i, o) in row.iter_mut().enumerate() {
                let s = src.get(rect.x + i as u32, rect.y + j as u32);
                let mixed = ColorBgra::from_bgra(
                    if b { o.b } else { s.b },
                    if g { o.g } else { s.g },
                    if r { o.r } else { s.r },
                    if a { o.a } else { s.a },
                );
                *o = if self.mode == BlendMode::Normal {
                    mixed
                } else {
                    self.mode.apply(s, mixed, 255)
                };
            }
        }
    }
}

impl Renderer for Separable {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let rect = Rect {
            x: x0,
            y,
            width: out.len() as u32,
            height: 1,
        };
        self.render_rect(src, rect, out);
    }

    fn render_rect(&self, src: &Surface, rect: Rect, out: &mut [ColorBgra]) {
        let r = self.r;
        if r == 0 {
            for (j, row) in out.chunks_mut(rect.width as usize).enumerate() {
                for (i, o) in row.iter_mut().enumerate() {
                    *o = src.get(rect.x + i as u32, rect.y + j as u32);
                }
            }
            return;
        }
        let (w, h) = (rect.width as usize, rect.height as usize);
        let (we, he) = (w + 2 * r, h + 2 * r);
        let (sw, sh) = (src.width() as i64, src.height() as i64);
        let (ex, ey) = (rect.x as i64 - r as i64, rect.y as i64 - r as i64);
        // Per source pixel: [1, A', A'·A, A'·B, A'·G, A'·R] with A' = A + A>>7 (3.36);
        // in linear light the colours are 16-bit linear values instead of sRGB bytes.
        let linear = self.linear;
        let q = |c: ColorBgra| -> [i64; 6] {
            let a = c.a as i64;
            let ap = a + (a >> 7);
            let ch = |v: u8| {
                if linear {
                    srgb_to_linear16(v)
                } else {
                    v as i64
                }
            };
            // 16 extra bits of headroom for the normalised kernels.
            [1, ap, ap * a, ap * ch(c.b), ap * ch(c.g), ap * ch(c.r)].map(|v| v << 16)
        };
        // Vertical pass over every extended column, for the output rows.
        let mut v = vec![[0i64; 6]; we * h];
        let mut col = vec![0i64; he];
        let mut tmp = vec![0i64; h];
        let mut samples = vec![[0i64; 6]; he];
        for cx in 0..we {
            let x = ex + cx as i64;
            if x < 0 || x >= sw {
                continue;
            }
            for (cy, s) in samples.iter_mut().enumerate() {
                let y = ey + cy as i64;
                *s = if y < 0 || y >= sh {
                    [0; 6]
                } else {
                    q(src.get(x as u32, y as u32))
                };
            }
            for k in 0..6 {
                for (c, s) in col.iter_mut().zip(&samples) {
                    *c = s[k];
                }
                self.apply(&col, &mut tmp);
                for (j, t) in tmp.iter().enumerate() {
                    v[j * we + cx][k] = *t;
                }
            }
        }
        // Horizontal pass.
        let mut row = vec![0i64; we];
        let mut res = vec![[0i64; 6]; w];
        let mut tmp = vec![0i64; w];
        for j in 0..h {
            for k in 0..6 {
                for (cx, rv) in row.iter_mut().enumerate() {
                    *rv = v[j * we + cx][k];
                }
                self.apply(&row, &mut tmp);
                for (i, t) in tmp.iter().enumerate() {
                    res[i][k] = *t;
                }
            }
            for (i, q) in res.iter().enumerate() {
                let [wa, wc, a, b, g, rr] = *q;
                let wc8 = wc >> 8;
                out[j * w + i] = if wa == 0 || wc8 == 0 {
                    ColorBgra::TRANSPARENT
                } else {
                    let c = |v: i64| {
                        if linear {
                            linear16_to_srgb((v + wc / 2) / wc)
                        } else {
                            ((v + wc / 2) / wc).clamp(0, 255) as u8
                        }
                    };
                    ColorBgra::from_bgra(
                        c(b),
                        c(g),
                        c(rr),
                        (((a >> 8) + wa / 2) / wa).clamp(0, 255) as u8,
                    )
                };
            }
        }
    }
}

/// Points along a line, sampled bilinearly (3.36 Motion Blur).
struct Motion {
    points: Vec<[f32; 2]>,
    linear: bool,
}

impl Renderer for Motion {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let (w, h) = ((src.width() - 1) as f32, (src.height() - 1) as f32);
        let mut samples = Vec::with_capacity(self.points.len());
        for (i, o) in out.iter_mut().enumerate() {
            let x = (x0 + i as u32) as f32;
            samples.clear();
            for p in &self.points {
                let (px, py) = (p[0] + x, p[1] + y as f32);
                if px >= 0.0 && py >= 0.0 && px <= w && py <= h {
                    samples.push(bilinear(src, px, py));
                }
            }
            *o = blend_in(&samples, self.linear);
        }
    }

    fn render_rect(&self, src: &Surface, rect: Rect, out: &mut [ColorBgra]) {
        let reach = self
            .points
            .iter()
            .map(|p| p[0].abs().max(p[1].abs()))
            .fold(0.0f32, f32::max);
        let block = Block::new(src, rect, reach.ceil() as u32 + 2);
        let (w, h) = ((src.width() - 1) as f32, (src.height() - 1) as f32);
        let mut samples = Vec::with_capacity(self.points.len());
        for (j, row) in out.chunks_mut(rect.width.max(1) as usize).enumerate() {
            let y = (rect.y + j as u32) as f32;
            for (i, o) in row.iter_mut().enumerate() {
                let x = (rect.x + i as u32) as f32;
                samples.clear();
                for p in &self.points {
                    let (px, py) = (p[0] + x, p[1] + y);
                    if px >= 0.0 && py >= 0.0 && px <= w && py <= h {
                        samples.push(block.bilinear(px, py));
                    }
                }
                *o = blend_in(&samples, self.linear);
            }
        }
    }

    fn gpu(&self, _: &Surface) -> Option<GpuJob> {
        (!self.linear).then(|| GpuJob::Motion {
            points: self.points.clone(),
        })
    }
}

/// 3.36 Zoom Blur (fixed point).
struct Zoom {
    amount: i64,
    offset: [f64; 2],
    linear: bool,
}

impl Zoom {
    fn centre(&self, src: &Surface) -> (i64, i64) {
        let (w, h) = (src.width() as i64, src.height() as i64);
        let fox = (w as f64 * self.offset[0] * 32768.0) as i64;
        let foy = (h as f64 * self.offset[1] * 32768.0) as i64;
        (fox + (w << 15), foy + (h << 15))
    }
}

impl Renderer for Zoom {
    fn gpu(&self, src: &Surface) -> Option<GpuJob> {
        // The shader works in i32: positions (x << 16, x < 2^14) and the centre must fit.
        let (fcx, fcy) = self.centre(src);
        let fits = |v: i64| v.abs() < 1 << 30;
        (!self.linear
            && src.width() < 1 << 14
            && src.height() < 1 << 14
            && fits(fcx)
            && fits(fcy)
            && (0..=100).contains(&self.amount))
        .then_some(GpuJob::Zoom {
            amount: self.amount as i32,
            fcx: fcx as i32,
            fcy: fcy as i32,
        })
    }

    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let (w, h) = (src.width() as i64, src.height() as i64);
        let (fcx, fcy) = self.centre(src);
        for (i, o) in out.iter_mut().enumerate() {
            let x = (x0 + i as u32) as i64;
            let c0 = src.get(x as u32, y);
            let (mut fx, mut fy) = ((x << 16) - fcx, ((y as i64) << 16) - fcy);
            let (mut sr, mut sg, mut sb, mut sa, mut sc) = (0i64, 0i64, 0i64, 0i64, 0i64);
            let linear = self.linear;
            let mut add = |c: ColorBgra| {
                let ch = |v: u8| {
                    if linear {
                        srgb_to_linear16(v)
                    } else {
                        v as i64
                    }
                };
                sr += ch(c.r) * c.a as i64;
                sg += ch(c.g) * c.a as i64;
                sb += ch(c.b) * c.a as i64;
                sa += c.a as i64;
                sc += 1;
            };
            add(c0);
            for _ in 0..64 {
                fx -= ((fx >> 4) * self.amount) >> 10;
                fy -= ((fy >> 4) * self.amount) >> 10;
                let u = (fx + fcx + 32768) >> 16;
                let v = (fy + fcy + 32768) >> 16;
                if u >= 0 && v >= 0 && u < w && v < h {
                    add(src.get(u as u32, v as u32));
                }
            }
            *o = if sa != 0 {
                let c = |v: i64| {
                    if self.linear {
                        linear16_to_srgb(v / sa)
                    } else {
                        (v / sa).clamp(0, 255) as u8
                    }
                };
                ColorBgra::from_bgra(c(sb), c(sg), c(sr), (sa / sc).clamp(0, 255) as u8)
            } else {
                ColorBgra::TRANSPARENT
            };
        }
    }
}

/// 3.36 Radial Blur (fixed point rotation).
struct Radial {
    angle: f64,
    offset: [f64; 2],
    quality: i32,
    linear: bool,
}

fn rotate(fx: &mut i32, fy: &mut i32, fr: i32) {
    let (cx, cy) = (*fx, *fy);
    *fx = cx
        .wrapping_sub(((cy >> 8).wrapping_mul(fr)) >> 8)
        .wrapping_sub(((cx >> 14).wrapping_mul(fr.wrapping_mul(fr) >> 11)) >> 8);
    *fy = cy
        .wrapping_add(((cx >> 8).wrapping_mul(fr)) >> 8)
        .wrapping_sub(((cy >> 14).wrapping_mul(fr.wrapping_mul(fr) >> 11)) >> 8);
}

impl Radial {
    /// (fcx, fcy, steps, step angle) in 3.36's fixed point.
    fn params(&self, src: &Surface) -> (i32, i32, i32, i32) {
        let (w, h) = (src.width() as i32, src.height() as i32);
        let fcx = (w << 15).wrapping_add((self.offset[0] * (w << 15) as f64) as i32);
        let fcy = (h << 15).wrapping_add((self.offset[1] * (h << 15) as f64) as i32);
        let q = self.quality;
        let n = (q * q) * (30 + q * q);
        let fr = (self.angle * std::f64::consts::PI * 65536.0 / 181.0) as i32;
        (fcx, fcy, n, fr / n)
    }
}

impl Renderer for Radial {
    fn gpu(&self, src: &Surface) -> Option<GpuJob> {
        // The CPU wraps in i32 as 3.36 did, so the shader matches it at any size.
        let (fcx, fcy, n, fsr) = self.params(src);
        (!self.linear).then_some(GpuJob::Radial { n, fsr, fcx, fcy })
    }

    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let (w, h) = (src.width() as i32, src.height() as i32);
        let (fcx, fcy, n, fsr) = self.params(src);
        for (i, o) in out.iter_mut().enumerate() {
            let x = (x0 + i as u32) as i32;
            let fx = (x << 16).wrapping_sub(fcx);
            let fy = ((y as i32) << 16).wrapping_sub(fcy);
            let (mut sr, mut sg, mut sb, mut sa, mut sc) = (0i64, 0i64, 0i64, 0i64, 0i64);
            let linear = self.linear;
            let mut add = |c: ColorBgra| {
                let ch = |v: u8| {
                    if linear {
                        srgb_to_linear16(v)
                    } else {
                        v as i64
                    }
                };
                sr += ch(c.r) * c.a as i64;
                sg += ch(c.g) * c.a as i64;
                sb += ch(c.b) * c.a as i64;
                sa += c.a as i64;
                sc += 1;
            };
            add(src.get(x as u32, y));
            let (mut ox1, mut oy1, mut ox2, mut oy2) = (fx, fy, fx, fy);
            for _ in 0..n {
                rotate(&mut ox1, &mut oy1, fsr);
                rotate(&mut ox2, &mut oy2, -fsr);
                for (ox, oy) in [(ox1, oy1), (ox2, oy2)] {
                    let u = ox.wrapping_add(fcx).wrapping_add(32768) >> 16;
                    let v = oy.wrapping_add(fcy).wrapping_add(32768) >> 16;
                    if u > 0 && v > 0 && u < w && v < h {
                        add(src.get(u as u32, v as u32));
                    }
                }
            }
            *o = if sa > 0 {
                let c = |v: i64| {
                    if self.linear {
                        linear16_to_srgb(v / sa)
                    } else {
                        (v / sa).clamp(0, 255) as u8
                    }
                };
                ColorBgra::from_bgra(c(sb), c(sg), c(sr), (sa / sc).clamp(0, 255) as u8)
            } else {
                ColorBgra::TRANSPARENT
            };
        }
    }
}

/// Integer offsets around a circle (3.36 Fragment).
struct Fragment {
    offsets: Vec<[i32; 2]>,
}

impl Renderer for Fragment {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let (w, h) = (src.width() as i32, src.height() as i32);
        let mut samples = Vec::with_capacity(self.offsets.len());
        for (i, o) in out.iter_mut().enumerate() {
            let x = (x0 + i as u32) as i32;
            samples.clear();
            for off in &self.offsets {
                let (u, v) = (x - off[0], y as i32 - off[1]);
                if u >= 0 && u < w && v >= 0 && v < h {
                    samples.push(src.get(u as u32, v as u32));
                }
            }
            *o = blend(&samples);
        }
    }
}

/// A disc-shaped sliding histogram (3.36 `LocalHistogramEffect.RenderRect`), with an
/// operation on (source pixel, histograms, area).
pub(crate) struct LocalHistogram<F: Fn(ColorBgra, &[[u32; 256]; 4], u32) -> ColorBgra + Send + Sync>
{
    pub(crate) radius: i32,
    pub(crate) apply: F,
}

impl<F: Fn(ColorBgra, &[[u32; 256]; 4], u32) -> ColorBgra + Send + Sync> Renderer
    for LocalHistogram<F>
{
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let rad = self.radius;
        let (w, h) = (src.width() as i32, src.height() as i32);
        let cutoff = ((rad * 2 + 1) * (rad * 2 + 1) + 2) / 4;
        // Half-width of the disc on each row offset.
        let edge: Vec<i32> = (0..=rad)
            .map(|v| {
                (0..=rad)
                    .filter(|u| u * u + v * v <= cutoff)
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let y = y as i32;
        let (top, bottom) = (-(rad.min(y)), rad.min(h - 1 - y));
        let mut hist = [[0u32; 256]; 4];
        let mut area = 0u32;
        let put = |x: i32, yy: i32, d: i32, hist: &mut [[u32; 256]; 4], area: &mut u32| {
            if x < 0 || x >= w {
                return;
            }
            let c = src.get(x as u32, yy as u32);
            for (k, v) in [c.b, c.g, c.r, c.a].into_iter().enumerate() {
                hist[k][v as usize] = (hist[k][v as usize] as i32 + d) as u32;
            }
            *area = (*area as i32 + d) as u32;
        };
        let x_first = x0 as i32;
        for v in top..=bottom {
            let e = edge[v.unsigned_abs() as usize];
            for u in -e..=e {
                put(x_first + u, y + v, 1, &mut hist, &mut area);
            }
        }
        for (i, o) in out.iter_mut().enumerate() {
            let x = x_first + i as i32;
            *o = (self.apply)(src.get(x as u32, y as u32), &hist, area);
            // Slide right: drop the left edge, add the next right edge.
            for v in top..=bottom {
                let e = edge[v.unsigned_abs() as usize];
                put(x - e, y + v, -1, &mut hist, &mut area);
                put(x + e + 1, y + v, 1, &mut hist, &mut area);
            }
        }
    }

    /// The same disc histograms, but the source block the tile needs is copied out once and
    /// the window snakes through the tile (right along a row, down one, left along the next),
    /// so it is never rebuilt: O(radius) per pixel with no per-row O(radius²) start-up.
    fn render_rect(&self, src: &Surface, rect: Rect, out: &mut [ColorBgra]) {
        let rad = self.radius;
        let (w, h) = (src.width() as i32, src.height() as i32);
        let (rw, rh) = (rect.width as i32, rect.height as i32);
        if rw == 0 || rh == 0 {
            return;
        }
        let cutoff = ((rad * 2 + 1) * (rad * 2 + 1) + 2) / 4;
        let edge: Vec<i32> = (0..=rad)
            .map(|v| {
                (0..=rad)
                    .filter(|u| u * u + v * v <= cutoff)
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let e = |d: i32| edge[d.unsigned_abs() as usize];
        // The source block every window in the tile can reach, clipped to the image.
        let bx0 = (rect.x as i32 - rad - 1).max(0);
        let bx1 = (rect.x as i32 + rw + rad + 1).min(w);
        let by0 = (rect.y as i32 - rad - 1).max(0);
        let by1 = (rect.y as i32 + rh + rad + 1).min(h);
        let bw = (bx1 - bx0) as usize;
        let mut block = Vec::with_capacity(bw * (by1 - by0) as usize);
        for yy in by0..by1 {
            for xx in bx0..bx1 {
                block.push(src.get(xx as u32, yy as u32));
            }
        }
        let mut hist = [[0u32; 256]; 4];
        let mut area = 0u32;
        let put = |hist: &mut [[u32; 256]; 4], area: &mut u32, x: i32, y: i32, add: bool| {
            if x < bx0 || x >= bx1 || y < by0 || y >= by1 {
                return;
            }
            let c = block[(y - by0) as usize * bw + (x - bx0) as usize];
            if add {
                hist[0][c.b as usize] += 1;
                hist[1][c.g as usize] += 1;
                hist[2][c.r as usize] += 1;
                hist[3][c.a as usize] += 1;
                *area += 1;
            } else {
                hist[0][c.b as usize] -= 1;
                hist[1][c.g as usize] -= 1;
                hist[2][c.r as usize] -= 1;
                hist[3][c.a as usize] -= 1;
                *area -= 1;
            }
        };
        let (mut x, mut y) = (rect.x as i32, rect.y as i32);
        for v in -rad..=rad {
            for u in -e(v)..=e(v) {
                put(&mut hist, &mut area, x + u, y + v, true);
            }
        }
        for j in 0..rh {
            if j > 0 {
                // Down one row: each column of the disc loses its top pixel, gains one below.
                for u in -rad..=rad {
                    put(&mut hist, &mut area, x + u, y - e(u), false);
                    put(&mut hist, &mut area, x + u, y + 1 + e(u), true);
                }
                y += 1;
            }
            let rightwards = j % 2 == 0;
            for k in 0..rw {
                let i = if rightwards { k } else { rw - 1 - k };
                let c = block[(y - by0) as usize * bw + (x - bx0) as usize];
                out[(j * rw + i) as usize] = (self.apply)(c, &hist, area);
                if k + 1 == rw {
                    break;
                }
                for v in -rad..=rad {
                    if rightwards {
                        put(&mut hist, &mut area, x - e(v), y + v, false);
                        put(&mut hist, &mut area, x + e(v) + 1, y + v, true);
                    } else {
                        put(&mut hist, &mut area, x + e(v), y + v, false);
                        put(&mut hist, &mut area, x - e(v) - 1, y + v, true);
                    }
                }
                x += if rightwards { 1 } else { -1 };
            }
        }
    }
}

/// 3.36 `LocalHistogramEffect.GetPercentile`, except that 3.36 stepped one bin past the one
/// reaching the count (so a flat area came out one level brighter each run); this returns
/// that bin.
fn percentile(p: i32, area: u32, h: &[[u32; 256]; 4]) -> ColorBgra {
    let min_count = area as i64 * p as i64 / 100;
    let pick = |hist: &[u32; 256]| {
        let mut v = 0usize;
        while v < 255 && hist[v] == 0 {
            v += 1;
        }
        let mut count = hist[v] as i64;
        while v < 255 && count < min_count {
            v += 1;
            count += hist[v] as i64;
        }
        v as u8
    };
    ColorBgra::from_bgra(pick(&h[0]), pick(&h[1]), pick(&h[2]), pick(&h[3]))
}

/// 3.36 Surface Blur's per-channel weighted average of similar values.
fn surface_channel(current: u8, hist: &[u32; 256], weights: &[i32; 256]) -> u8 {
    let (mut sum, mut div) = (0i64, 0i64);
    for (bin, &n) in hist.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let w = weights[(bin as i32 - current as i32).unsigned_abs() as usize];
        if w > 0 {
            let t = n as i64 * w as i64;
            sum += t * bin as i64;
            div += t;
        }
    }
    if div > 0 {
        ((sum + (div >> 1)) / div) as u8
    } else {
        current
    }
}

/// Disc average (Bokeh), using per-row prefix sums over each disc chord.
struct Disc {
    r: i32,
}

impl Renderer for Disc {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let r = self.r;
        let (w, h) = (src.width() as i32, src.height() as i32);
        let n = out.len() as i32;
        let (xs, xe) = ((x0 as i32 - r).max(0), (x0 as i32 + n + r).min(w));
        let mut acc = vec![[0i64; 4]; n as usize];
        let mut row = vec![[0i64; 4]; (xe - xs + 1) as usize];
        for dy in -r..=r {
            let yy = y as i32 + dy;
            if yy < 0 || yy >= h {
                continue;
            }
            let e = ((r * r - dy * dy) as f64).sqrt() as i32;
            // Prefix sums of premultiplied values along this row.
            for x in xs..xe {
                let c = src.get(x as u32, yy as u32);
                let a = c.a as i64;
                let i = (x - xs) as usize;
                row[i + 1] = [
                    row[i][0] + a,
                    row[i][1] + a * c.b as i64,
                    row[i][2] + a * c.g as i64,
                    row[i][3] + a * c.r as i64,
                ];
            }
            for (i, s) in acc.iter_mut().enumerate() {
                let x = x0 as i32 + i as i32;
                let (l, rr) = ((x - e).max(xs), (x + e + 1).min(xe));
                if rr <= l {
                    continue;
                }
                let (pl, pr) = (row[(l - xs) as usize], row[(rr - xs) as usize]);
                for k in 0..4 {
                    s[k] += pr[k] - pl[k];
                }
                s[0] += 0;
            }
        }
        // Count of in-bounds disc pixels for the alpha average.
        for (i, o) in out.iter_mut().enumerate() {
            let x = x0 as i32 + i as i32;
            let mut count = 0i64;
            for dy in -r..=r {
                let yy = y as i32 + dy;
                if yy < 0 || yy >= h {
                    continue;
                }
                let e = ((r * r - dy * dy) as f64).sqrt() as i32;
                count += ((x + e + 1).min(w) - (x - e).max(0)).max(0) as i64;
            }
            let [a, b, g, rr] = acc[i];
            *o = if a == 0 || count == 0 {
                ColorBgra::TRANSPARENT
            } else {
                let c = |v: i64| (v / a).clamp(0, 255) as u8;
                ColorBgra::from_bgra(c(b), c(g), c(rr), (a / count).clamp(0, 255) as u8)
            };
        }
    }
}

/// The Blurs submenu's effects by name.
pub fn def(name: &str) -> Option<EffectDef> {
    let d = |name: &'static str, props: Vec<PropDef>, build| EffectDef {
        name,
        props,
        unclipped: false,
        link: None,
        build,
    };
    Some(match name {
        "Gaussian Blur" => d(
            "Gaussian Blur",
            vec![
                PropDef::int("Radius", 2, 0, 200),
                // Channel choice and blending (as BoltBait's Gaussian Blur+); the defaults
                // give the plain blur.
                PropDef::bool("Red", true).in_section(CHANNELS),
                PropDef::bool("Green", true).in_section(CHANNELS),
                PropDef::bool("Blue", true).in_section(CHANNELS),
                PropDef::bool("Alpha", true).in_section(CHANNELS),
                PropDef::choice("Blend mode", 0, &BlendMode::NAMES).in_section(CHANNELS),
                // paintnotnet: average in linear light (highlights spread as light does).
                PropDef::bool("Linear light", false),
            ],
            |v: &[Value], _, _| {
                let blur = Separable::gaussian(v[0].int().max(0) as usize).linear(v[6].bool());
                let channels = [v[1].bool(), v[2].bool(), v[3].bool(), v[4].bool()];
                let mode = BlendMode::ALL[v[5].choice().min(BlendMode::ALL.len() - 1)];
                if channels == [true; 4] && mode == BlendMode::Normal {
                    Box::new(blur)
                } else {
                    Box::new(ChannelBlur {
                        blur,
                        channels,
                        mode,
                    })
                }
            },
        ),
        "Square Blur" => d(
            "Square Blur",
            vec![PropDef::int("Radius", 2, 0, 200)],
            |v: &[Value], _, _| Box::new(Separable::square(v[0].int().max(0) as usize)),
        ),
        "Motion Blur" => d(
            "Motion Blur",
            vec![
                PropDef::angle("Angle", 25.0, -180.0, 180.0),
                PropDef::bool("Centered", true),
                PropDef::int("Distance", 10, 1, 200),
                // paintnotnet: average in linear light.
                PropDef::bool("Linear light", false),
            ],
            |v: &[Value], _, _| {
                let (angle, centered, distance) = (v[0].float(), v[1].bool(), v[2].int());
                let theta = (angle + 180.0) * 2.0 * std::f64::consts::PI / 360.0;
                let (mut sx, mut sy) = (0.0f32, 0.0f32);
                let (mut ex, mut ey) = (
                    (distance as f64 * theta.cos()) as f32,
                    (-(distance as f64 * theta.sin())) as f32,
                );
                if centered {
                    (sx, sy) = (-ex / 2.0, -ey / 2.0);
                    (ex, ey) = (ex / 2.0, ey / 2.0);
                }
                let n = (((1 + distance) * 3) / 2) as usize;
                let points = if n <= 1 {
                    vec![[0.0, 0.0]]
                } else {
                    (0..n)
                        .map(|i| {
                            let f = i as f32 / (n - 1) as f32;
                            [sx + (ex - sx) * f, sy + (ey - sy) * f]
                        })
                        .collect()
                };
                Box::new(Motion {
                    points,
                    linear: v[3].bool(),
                })
            },
        ),
        "Radial Blur" => d(
            "Radial Blur",
            vec![
                PropDef::angle("Angle", 2.0, 0.0, 360.0),
                PropDef::float("Offset X", 0.0, -2.0, 2.0, 2),
                PropDef::float("Offset Y", 0.0, -2.0, 2.0, 2),
                PropDef::int("Quality", 2, 1, 5),
                // paintnotnet: average in linear light.
                PropDef::bool("Linear light", false),
            ],
            |v: &[Value], _, _| {
                Box::new(Radial {
                    angle: v[0].float(),
                    offset: [v[1].float(), v[2].float()],
                    quality: v[3].int(),
                    linear: v[4].bool(),
                })
            },
        ),
        "Zoom Blur" => d(
            "Zoom Blur",
            vec![
                PropDef::int("Zoom Amount", 10, 0, 100),
                PropDef::float("Offset X", 0.0, -2.0, 2.0, 2),
                PropDef::float("Offset Y", 0.0, -2.0, 2.0, 2),
                // paintnotnet: average in linear light.
                PropDef::bool("Linear light", false),
            ],
            |v: &[Value], _, _| {
                Box::new(Zoom {
                    amount: v[0].int() as i64,
                    offset: [v[1].float(), v[2].float()],
                    linear: v[3].bool(),
                })
            },
        ),
        "Fragment Blur" => d(
            "Fragment Blur",
            vec![
                PropDef::int("Fragments", 4, 2, 50),
                PropDef::int("Distance", 8, 0, 100),
                PropDef::float("Rotation", 0.0, 0.0, 360.0, 2),
            ],
            |v: &[Value], _, _| {
                let (n, dist, rot) = (v[0].int(), v[1].int() as f64, v[2].float());
                let step = 2.0 * std::f64::consts::PI / n as f64;
                let rr = (rot - 90.0) * std::f64::consts::PI / 180.0;
                let offsets = (0..n)
                    .map(|i| {
                        let a = rr + step * i as f64;
                        [
                            (dist * -a.sin()).round() as i32,
                            (dist * -a.cos()).round() as i32,
                        ]
                    })
                    .collect();
                Box::new(Fragment { offsets })
            },
        ),
        "Median Blur" => d(
            "Median Blur",
            vec![
                PropDef::int("Radius", 10, 1, 200),
                PropDef::int("Percentile", 50, 0, 100),
            ],
            |v: &[Value], _, _| {
                let p = v[1].int();
                Box::new(LocalHistogram {
                    radius: v[0].int(),
                    apply: move |_, h: &[[u32; 256]; 4], area| percentile(p, area, h),
                })
            },
        ),
        "Surface Blur" => d(
            "Surface Blur",
            vec![
                PropDef::int("Radius", 6, 1, 100),
                PropDef::int("Threshold", 15, 1, 100),
            ],
            |v: &[Value], _, _| {
                let slope = 96.0 / v[1].int() as f64;
                let weights: [i32; 256] =
                    std::array::from_fn(|i| ((255.0 - i as f64 * slope).round() as i32).max(0));
                Box::new(LocalHistogram {
                    radius: v[0].int(),
                    apply: move |c: ColorBgra, h: &[[u32; 256]; 4], _| {
                        ColorBgra::from_bgra(
                            surface_channel(c.b, &h[0], &weights),
                            surface_channel(c.g, &h[1], &weights),
                            surface_channel(c.r, &h[2], &weights),
                            c.a,
                        )
                    },
                })
            },
        ),
        "Bokeh Blur" => d(
            "Bokeh Blur",
            vec![PropDef::int("Radius", 10, 1, 100)],
            |v: &[Value], _, _| Box::new(Disc { r: v[0].int() }),
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Env, render_rect};

    /// Tile renderers (snaking histograms, copied source blocks) must give exactly what the
    /// row-by-row ones did.
    #[test]
    fn tile_renderers_match_rows() {
        let (w, h) = (300u32, 200u32);
        let mut src = Surface::new_transparent(w, h);
        let mut seed = 12345u32;
        for y in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let n = (seed >> 16) as u8;
                src.set(
                    x,
                    y,
                    ColorBgra::from_rgba(n, (x % 256) as u8, (y % 256) as u8, 255 - n / 4),
                );
            }
        }
        let all = Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let env = Env {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            bounds: all,
            selection: None,
            layers: Vec::new(),
            active_layer: 0,
            selection_contours: Default::default(),
        };
        for (name, values) in [
            ("Median Blur", vec![Value::Int(3), Value::Int(50)]),
            ("Median Blur", vec![Value::Int(40), Value::Int(80)]),
            ("Reduce Noise", vec![Value::Int(25), Value::Float(0.6)]),
            (
                "Motion Blur",
                vec![
                    Value::Float(25.0),
                    Value::Bool(true),
                    Value::Int(60),
                    Value::Bool(true),
                ],
            ),
            ("Surface Blur", vec![Value::Int(12), Value::Int(30)]),
        ] {
            let d = def(name).or_else(|| crate::stylize::def(name)).unwrap();
            let r = d.renderer(&values, &src, &env);
            let tiles = render_rect(r.as_ref(), &src, all);
            let mut rows = vec![ColorBgra::TRANSPARENT; (w * h) as usize];
            for (y, row) in rows.chunks_mut(w as usize).enumerate() {
                r.render_row(&src, 0, y as u32, row);
            }
            assert!(tiles == rows, "{name} {values:?}");
        }
    }

    fn checker(w: u32, h: u32) -> Surface {
        let mut s = Surface::new_transparent(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = if (x / 4 + y / 4) % 2 == 0 { 0 } else { 255 };
                s.set(x, y, ColorBgra::from_rgba(v, v, v, 255));
            }
        }
        s
    }

    fn run(name: &str, values: Vec<Value>, src: &Surface) -> Vec<ColorBgra> {
        let d = def(name).unwrap();
        let all = Rect {
            x: 0,
            y: 0,
            width: src.width(),
            height: src.height(),
        };
        let env = Env {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            bounds: all,
            selection: None,
            layers: Vec::new(),
            active_layer: 0,
            selection_contours: Default::default(),
        };
        let r = d.renderer(&values, src, &env);
        render_rect(r.as_ref(), src, all)
    }

    /// The 1-D kernel applied by `apply` (half-width `r`), as weights summing to 1.
    fn kernel_weights(apply: impl Fn(&[i64], &mut [i64]), r: usize) -> Vec<f64> {
        let raw: Vec<f64> = (0..=2 * r)
            .map(|i| {
                let mut a = vec![0i64; 4 * r + 1];
                a[r + i] = 1 << 20;
                let mut out = vec![0i64; 2 * r + 1];
                apply(&a, &mut out);
                out[r] as f64
            })
            .collect();
        let total: f64 = raw.iter().sum();
        raw.into_iter().map(|w| w / total).collect()
    }

    /// A 2-D separable kernel is round only if it is Gaussian: the weight at distance d along
    /// an axis must equal the weight at d along the diagonal. 3.36's tent fails this badly
    /// (highlights blur into diamonds); the box-cascade Gaussian doesn't.
    #[test]
    fn gaussian_is_round_where_the_tent_was_not() {
        let radius = 30;
        let g = Separable::gaussian(radius);
        let gw = kernel_weights(|a, o| g.apply(a, o), g.r);
        let tw = kernel_weights(|a, o| tent(a, radius, o), radius);
        // Linear interpolation into a centred kernel.
        let at = |w: &[f64], d: f64| {
            let c = (w.len() / 2) as f64 + d;
            let (i, f) = (c.floor() as usize, c.fract());
            w[i] * (1.0 - f) + w.get(i + 1).copied().unwrap_or(0.0) * f
        };
        let roundness = |w: &[f64], d: f64| {
            let axis = at(w, d) * at(w, 0.0);
            let diagonal = at(w, d / 2f64.sqrt()).powi(2);
            diagonal / axis
        };
        let d = 1.2 * tent_sigma(radius);
        let (gr, tr) = (roundness(&gw, d), roundness(&tw, d));
        assert!((gr - 1.0).abs() < 0.03, "Gaussian off-round by {gr}");
        assert!((tr - 1.0).abs() > 0.15, "the tent was off-round by {tr}");
    }

    /// The box cascade has the tent's σ (within 3%) and follows a Gaussian of its σ closely.
    #[test]
    fn gaussian_matches_the_ideal_curve() {
        for radius in [1usize, 5, 19, 30, 120] {
            let g = Separable::gaussian(radius);
            let w = kernel_weights(|a, o| g.apply(a, o), g.r);
            // The boxes have whole-pixel widths, so σ lands near the target, not on it.
            let sigma = w
                .iter()
                .enumerate()
                .map(|(i, &v)| v * (i as f64 - g.r as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            let target = tent_sigma(radius);
            assert!(
                (sigma / target - 1.0).abs() < 0.03,
                "radius {radius}: σ {sigma} for {target}"
            );
            let peak = w[g.r];
            let mut worst = 0.0f64;
            for (i, &v) in w.iter().enumerate() {
                let d = i as f64 - g.r as f64;
                let ideal =
                    (-d * d / (2.0 * sigma * sigma)).exp() / (sigma * std::f64::consts::TAU.sqrt());
                worst = worst.max((v - ideal).abs() / peak);
            }
            assert!(
                worst < 0.04,
                "radius {radius}: worst error {worst} of the peak"
            );
        }
        let same = run("Gaussian Blur", vec![Value::Int(0)], &checker(40, 30));
        assert_eq!(same, checker(40, 30).to_pixels(), "radius 0 copies");
    }

    /// Across a black/white edge, sRGB averaging gives a dark mid-grey (128); linear light
    /// gives the brighter grey half the light actually makes (≈188).
    #[test]
    fn linear_light_averages_light() {
        let mut edge = Surface::new_transparent(64, 8);
        for y in 0..8 {
            for x in 0..64 {
                let v = if x < 32 { 0 } else { 255 };
                edge.set(x, y, ColorBgra::from_rgba(v, v, v, 255));
            }
        }
        let mut values: Vec<Value> = def("Gaussian Blur").unwrap().defaults();
        values[0] = Value::Int(12);
        let plain = run("Gaussian Blur", values.clone(), &edge);
        values[6] = Value::Bool(true);
        let lin = run("Gaussian Blur", values, &edge);
        // Between the two middle columns.
        let mid = |p: &[ColorBgra]| (p[4 * 64 + 31].r as i32 + p[4 * 64 + 32].r as i32) / 2;
        assert!((mid(&plain) - 128).abs() <= 4, "sRGB {}", mid(&plain));
        assert!((mid(&lin) - 188).abs() <= 6, "linear {}", mid(&lin));
    }

    /// The sample-averaging blurs take the option too: Motion Blur across the same edge.
    #[test]
    fn motion_blur_linear_light() {
        let mut edge = Surface::new_transparent(64, 8);
        for y in 0..8 {
            for x in 0..64 {
                let v = if x < 32 { 0 } else { 255 };
                edge.set(x, y, ColorBgra::from_rgba(v, v, v, 255));
            }
        }
        let values = |linear| {
            vec![
                Value::Float(0.0),
                Value::Bool(true),
                Value::Int(20),
                Value::Bool(linear),
            ]
        };
        let plain = run("Motion Blur", values(false), &edge);
        let lin = run("Motion Blur", values(true), &edge);
        let mid = |p: &[ColorBgra]| (p[4 * 64 + 31].r as i32 + p[4 * 64 + 32].r as i32) / 2;
        assert!((mid(&plain) - 128).abs() <= 8, "sRGB {}", mid(&plain));
        assert!((mid(&lin) - 188).abs() <= 10, "linear {}", mid(&lin));
    }

    /// A wide Gaussian over a big image must neither overflow nor shift a flat colour, and
    /// fades evenly towards transparent edges.
    #[test]
    fn wide_blurs_keep_flat_colours() {
        let mut flat = Surface::new_transparent(900, 600);
        let c = ColorBgra::from_rgba(80, 120, 200, 255);
        for y in 0..600 {
            for x in 0..900 {
                flat.set(x, y, c);
            }
        }
        for linear in [false, true] {
            let mut values: Vec<Value> = def("Gaussian Blur").unwrap().defaults();
            values[0] = Value::Int(200);
            values[6] = Value::Bool(linear);
            let out = run("Gaussian Blur", values, &flat);
            for &p in &[out[0], out[300 * 900 + 450], out[599 * 900 + 899]] {
                assert_eq!(p, c, "linear {linear}");
            }
        }
    }

    #[test]
    fn blurs_average_and_keep_flat_areas() {
        let mut flat = Surface::new_transparent(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                flat.set(x, y, ColorBgra::from_rgba(80, 120, 200, 255));
            }
        }
        for name in [
            "Gaussian Blur",
            "Square Blur",
            "Motion Blur",
            "Radial Blur",
            "Zoom Blur",
            "Fragment Blur",
            "Median Blur",
            "Surface Blur",
            "Bokeh Blur",
        ] {
            let d = def(name).unwrap();
            let out = run(name, d.defaults(), &flat);
            let c = out[16 * 32 + 16];
            assert_eq!(
                c,
                ColorBgra::from_rgba(80, 120, 200, 255),
                "{name} keeps a flat area"
            );
        }
        // On a checkerboard, averaging blurs land near mid-grey in the middle.
        let src = checker(64, 64);
        for name in ["Gaussian Blur", "Square Blur", "Bokeh Blur"] {
            let d = def(name).unwrap();
            let out = run(name, d.defaults(), &src);
            let v = out[32 * 64 + 34].r;
            assert!((60..=195).contains(&v), "{name}: {v}");
        }
    }

    #[test]
    fn median_and_surface_preserve_edges() {
        let mut s = Surface::new_transparent(40, 10);
        for y in 0..10 {
            for x in 0..40 {
                let v = if x < 20 { 30 } else { 220 };
                s.set(x, y, ColorBgra::from_rgba(v, v, v, 255));
            }
        }
        let m = run("Median Blur", vec![Value::Int(3), Value::Int(50)], &s);
        assert_eq!((m[5 * 40 + 17].r, m[5 * 40 + 22].r), (30, 220));
        let sb = run("Surface Blur", vec![Value::Int(3), Value::Int(15)], &s);
        assert_eq!(
            (sb[5 * 40 + 18].r, sb[5 * 40 + 21].r),
            (30, 220),
            "edge kept"
        );
    }

    #[test]
    #[ignore = "timing; run with --release --ignored --nocapture"]
    fn blurs_4k() {
        let src = checker(3840, 2160);
        let all = Rect {
            x: 0,
            y: 0,
            width: 3840,
            height: 2160,
        };
        let env = Env {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            bounds: all,
            selection: None,
            layers: Vec::new(),
            active_layer: 0,
            selection_contours: Default::default(),
        };
        for (name, v) in [
            ("Gaussian Blur", vec![Value::Int(2)]),
            ("Gaussian Blur", vec![Value::Int(200)]),
            ("Square Blur", vec![Value::Int(50)]),
            (
                "Motion Blur",
                vec![Value::Float(25.0), Value::Bool(true), Value::Int(10)],
            ),
            (
                "Motion Blur",
                vec![Value::Float(25.0), Value::Bool(true), Value::Int(200)],
            ),
            ("Radial Blur", def("Radial Blur").unwrap().defaults()),
            ("Zoom Blur", def("Zoom Blur").unwrap().defaults()),
            ("Median Blur", vec![Value::Int(10), Value::Int(50)]),
            ("Surface Blur", vec![Value::Int(6), Value::Int(15)]),
            ("Bokeh Blur", vec![Value::Int(20)]),
        ] {
            let r = def(name).unwrap().renderer(&v, &src, &env);
            // Render in 256² tiles in parallel, as the app does.
            use rayon::prelude::*;
            let tiles: Vec<Rect> = (0..9)
                .flat_map(|ty| (0..15).map(move |tx| (tx, ty)))
                .map(|(tx, ty)| Rect {
                    x: tx * 256,
                    y: ty * 256,
                    width: 256,
                    height: (2160 - ty * 256).min(256),
                })
                .collect();
            let t = std::time::Instant::now();
            tiles.par_iter().for_each(|&rect| {
                let mut out = vec![ColorBgra::TRANSPARENT; (rect.width * rect.height) as usize];
                r.render_rect(&src, rect, &mut out);
                std::hint::black_box(out);
            });
            println!(
                "{name} {:?}: {:.0} ms",
                v.first(),
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    #[test]
    fn gaussian_channels_and_blend() {
        // Left half red, right half blue, opaque.
        let mut src = Surface::new_transparent(20, 10);
        for y in 0..10 {
            for x in 0..20 {
                let c = if x < 10 { (255, 0, 0) } else { (0, 0, 255) };
                src.set(x, y, ColorBgra::from_rgba(c.0, c.1, c.2, 255));
            }
        }
        let v = |r, g, b, a, mode| {
            vec![
                Value::Int(4),
                Value::Bool(r),
                Value::Bool(g),
                Value::Bool(b),
                Value::Bool(a),
                Value::Choice(mode),
            ]
        };
        let all = run("Gaussian Blur", v(true, true, true, true, 0), &src);
        let plain = run("Gaussian Blur", vec![Value::Int(4)], &src);
        assert_eq!(all, plain, "defaults are the plain blur");
        // Only red blurred: blue keeps its hard edge.
        let red = run("Gaussian Blur", v(true, false, false, true, 0), &src);
        let at = |o: &[ColorBgra], x: usize| o[5 * 20 + x];
        assert!(at(&red, 10).r > 0 && at(&red, 10).r < 255);
        assert_eq!((at(&red, 9).b, at(&red, 10).b), (0, 255));
        // Darken (BlendMode::ALL[11]) never brightens a channel.
        let dark = run("Gaussian Blur", v(true, true, true, true, 11), &src);
        for x in 0..20 {
            assert!(at(&dark, x).r <= src.get(x as u32, 5).r);
            assert!(at(&dark, x).b <= src.get(x as u32, 5).b);
        }
    }
}
