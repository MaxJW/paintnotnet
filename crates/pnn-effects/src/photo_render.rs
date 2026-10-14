//! Effects > Photo, Render, Object (Drop Shadow) and Color (Quantize).
//!
//! Ported from paint.net 3.36 (MIT, see NOTICE): `Effects/SharpenEffect.cs`,
//! `RedEyeRemoveEffect.cs` + `UnaryPixelOps.RedEyeRemove`, `SoftenPortraitEffect.cs` (by David
//! Issel "BoltBait", MIT, contributed to paint.net), `VignetteEffect.cs` + `SrgbUtility.cs`,
//! `CloudsEffect.cs`, `JuliaFractalEffect.cs`, `MandelbrotFractalEffect.cs`.
//!
//! From paint.net 5's docs only, maths guessed: Straighten (rotate, zoomed to leave no empty
//! corners), Turbulence (absolute-value Perlin octaves between the two colours), Drop Shadow
//! (the object's alpha blurred, offset and tinted with the primary colour, under the object)
//! and Quantize (median cut; ordered dithering, since error diffusion can't run tile by tile).
//! Clouds and Turbulence draw from the Seed setting only (3.36 mixed in a per-run random
//! seed).

use pnn_core::{BlendMode, ColorBgra, Rect, Surface};

use crate::adjustments::BrightnessContrast;
use crate::blurs::{LocalHistogram, Separable};
use crate::distort::{bilinear_clamped, perlin};
use crate::framework::{ColorDefault, EffectDef, Env, PropDef, Renderer, Value, intensity};

fn lerp_c(a: ColorBgra, b: ColorBgra, t: f64) -> ColorBgra {
    let l = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).clamp(0.0, 255.0) as u8;
    ColorBgra::from_bgra(l(a.b, b.b), l(a.g, b.g), l(a.r, b.r), l(a.a, b.a))
}

fn to_linear(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb_clamped(l: f64) -> f64 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.0031308 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// Per-pixel renderer over (src, x, y).
struct At<F: Fn(&Surface, u32, u32) -> ColorBgra + Send + Sync>(F);

impl<F: Fn(&Surface, u32, u32) -> ColorBgra + Send + Sync> Renderer for At<F> {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = (self.0)(src, x0 + i as u32, y);
        }
    }
}

/// 3.36 Soften Portrait: blur + brightness/contrast, overlaid with a warmed grey source.
struct SoftenPortrait {
    blur: Separable,
    bc: BrightnessContrast,
    warmth: i32,
}

impl Renderer for SoftenPortrait {
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
        let w = rect.width as usize;
        let (red, blue) = (
            1.0 + self.warmth as f32 / 100.0,
            1.0 - self.warmth as f32 / 100.0,
        );
        let mut tmp = Surface::new_transparent(w as u32, 1);
        for (j, row) in out.chunks_mut(w).enumerate() {
            for (i, c) in row.iter().enumerate() {
                tmp.set(i as u32, 0, *c);
            }
            self.bc.render_row(&tmp, 0, 0, row);
            for (i, o) in row.iter_mut().enumerate() {
                let s = src.get(rect.x + i as u32, rect.y + j as u32);
                let g = intensity(s);
                let grey = ColorBgra::from_bgra(
                    ((g as f32 * blue) as i32).clamp(0, 255) as u8,
                    g,
                    ((g as f32 * red) as i32).clamp(0, 255) as u8,
                    s.a,
                );
                *o = BlendMode::Overlay.apply(grey, *o, 255);
            }
        }
    }
}

/// Drop shadow: the object's alpha, blurred and offset, under the object.
struct DropShadow {
    blur: Separable,
    offset: [i64; 2],
    color: ColorBgra,
    opacity: f64,
    only_shadow: bool,
}

impl Renderer for DropShadow {
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
        // Blur an alpha-only copy of the source shifted by the offset, over this rect.
        let (w, h) = (src.width() as i64, src.height() as i64);
        let shifted = |x: i64, y: i64| -> u8 {
            let (sx, sy) = (x - self.offset[0], y - self.offset[1]);
            if sx < 0 || sy < 0 || sx >= w || sy >= h {
                0
            } else {
                src.get(sx as u32, sy as u32).a
            }
        };
        let r = self.blur.r as i64;
        let (ex, ey) = ((rect.x as i64 - r).max(0), (rect.y as i64 - r).max(0));
        let ex1 = (rect.x as i64 + rect.width as i64 + r).min(w);
        let ey1 = (rect.y as i64 + rect.height as i64 + r).min(h);
        let mut mask = Surface::new_transparent((ex1 - ex) as u32, (ey1 - ey) as u32);
        for y in ey..ey1 {
            for x in ex..ex1 {
                let a = shifted(x, y);
                if a > 0 {
                    mask.set(
                        (x - ex) as u32,
                        (y - ey) as u32,
                        ColorBgra::from_bgra(0, 0, 0, a),
                    );
                }
            }
        }
        let inner = Rect {
            x: (rect.x as i64 - ex) as u32,
            y: (rect.y as i64 - ey) as u32,
            width: rect.width,
            height: rect.height,
        };
        self.blur.render_rect(&mask, inner, out);
        let w_ = rect.width as usize;
        for (k, o) in out.iter_mut().enumerate() {
            let (x, y) = (rect.x + (k % w_) as u32, rect.y + (k / w_) as u32);
            let a = (o.a as f64 * self.opacity * self.color.a as f64 / 255.0).round() as u8;
            let shadow = self.color.with_alpha(a);
            *o = if self.only_shadow {
                shadow
            } else {
                BlendMode::Normal.apply(shadow, src.get(x, y), 255)
            };
        }
    }
}

/// A palette lookup with ordered dithering.
struct Quantize {
    palette: Vec<ColorBgra>,
    dither: i32,
}

const BAYER4: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

impl Renderer for Quantize {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            let x = x0 + i as u32;
            let c = src.get(x, y);
            // Bayer offset in −0.5..0.5 steps, scaled by the dither level.
            let t = (BAYER4[(y % 4) as usize][(x % 4) as usize] as f64 + 0.5) / 16.0 - 0.5;
            let d = (t * self.dither as f64 * 6.0) as i32;
            let (r, g, b) = (c.r as i32 + d, c.g as i32 + d, c.b as i32 + d);
            let best = self
                .palette
                .iter()
                .min_by_key(|p| {
                    let (dr, dg, db) = (p.r as i32 - r, p.g as i32 - g, p.b as i32 - b);
                    dr * dr + dg * dg + db * db
                })
                .copied()
                .unwrap_or(c);
            *o = best.with_alpha(c.a);
        }
    }
}

/// Median cut over the pixels in `bounds` (sampled), `n` colours.
fn median_cut(src: &Surface, bounds: Rect, n: usize) -> Vec<ColorBgra> {
    let step = ((bounds.width as u64 * bounds.height as u64 / 200_000).max(1) as f64).sqrt() as u32;
    let mut px: Vec<[u8; 3]> = Vec::new();
    let mut y = bounds.y;
    while y < bounds.y + bounds.height {
        let mut x = bounds.x;
        while x < bounds.x + bounds.width {
            let c = src.get(x, y);
            px.push([c.r, c.g, c.b]);
            x += step.max(1);
        }
        y += step.max(1);
    }
    let mut boxes: Vec<Vec<[u8; 3]>> = vec![px];
    while boxes.len() < n {
        // Split the box with the widest channel range.
        let (bi, ch, range) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let r = (0..3)
                    .map(|c| {
                        let lo = b.iter().map(|p| p[c]).min().unwrap_or(0);
                        let hi = b.iter().map(|p| p[c]).max().unwrap_or(0);
                        (c, hi - lo)
                    })
                    .max_by_key(|&(_, r)| r)
                    .unwrap_or((0, 0));
                (i, r.0, r.1)
            })
            .max_by_key(|&(_, _, r)| r)
            .unwrap_or((0, 0, 0));
        if range == 0 {
            break;
        }
        let mut b = boxes.swap_remove(bi);
        b.sort_unstable_by_key(|p| p[ch]);
        let upper = b.split_off(b.len() / 2);
        boxes.push(b);
        boxes.push(upper);
    }
    boxes
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let n = b.len() as u64;
            let s = b.iter().fold([0u64; 3], |a, p| {
                [a[0] + p[0] as u64, a[1] + p[1] as u64, a[2] + p[2] as u64]
            });
            ColorBgra::from_rgba((s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8, 255)
        })
        .collect()
}

fn julia(mut x: f64, mut y: f64, r: f64, i: f64) -> f64 {
    let mut c = 0.0;
    while c < 256.0 && x * x + y * y < 10000.0 {
        let t = x;
        x = x * x - y * y + r;
        y = 2.0 * t * y + i;
        c += 1.0;
    }
    c - (2.0 - 2.0 * 10000f64.ln() / (x * x + y * y).ln())
}

fn mandelbrot(r: f64, i: f64, factor: i32) -> f64 {
    // Points in the main cardioid or the period-2 bulb never escape; 3.36 iterates them to
    // the limit and every channel then clamps to 255. Infinity gives exactly that colour
    // without the 1024 iterations (most of the default view).
    let q = (r - 0.25) * (r - 0.25) + i * i;
    if q * (q + (r - 0.25)) <= 0.25 * i * i || (r + 1.0) * (r + 1.0) + i * i <= 0.0625 {
        return f64::INFINITY;
    }
    mandelbrot_iterate(r, i, factor)
}

/// 3.36 `MandelbrotFractalEffect.Mandelbrot`: the escape count, smoothed.
fn mandelbrot_iterate(r: f64, i: f64, factor: i32) -> f64 {
    let (mut c, mut x, mut y) = (0i32, 0.0f64, 0.0f64);
    while c * factor < 1024 && x * x + y * y < 100000.0 {
        let t = x;
        x = x * x - y * y + r;
        y = 2.0 * t * y + i;
        c += 1;
    }
    c as f64 - (y * y + x * x).ln() / 100000f64.ln()
}

/// 3.36 `RenderClouds` for one pixel (`turb`: absolute octaves, for Turbulence).
#[allow(clippy::too_many_arguments)]
fn clouds_value(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    scale: i32,
    power: f64,
    seed: u8,
    turb: bool,
) -> f64 {
    let (dx, dy) = (2 * x as i64 - w as i64, 2 * y as i64 - h as i64);
    let (mut val, mut mult, mut div) = (0.0, 1.0, scale);
    let mut i = 0u8;
    while i < 12 && mult > 0.03 && div > 0 {
        let mut dxr = 65536.0 + dx as f64 / div as f64;
        let mut dyr = 65536.0 + dy as f64 / div as f64;
        let (dxd, dyd) = (dxr as i64, dyr as i64);
        dxr -= dxd as f64;
        dyr -= dyd as f64;
        let n = perlin::noise_cell(dxd as u8, dyd as u8, dxr, dyr, seed ^ i);
        val += if turb { n.abs() } else { n } * mult;
        div /= 2;
        mult *= power;
        i += 1;
    }
    val
}

const BLEND_NAMES: [&str; 14] = [
    "Normal",
    "Multiply",
    "Additive",
    "Color Burn",
    "Color Dodge",
    "Reflect",
    "Glow",
    "Overlay",
    "Difference",
    "Negation",
    "Lighten",
    "Darken",
    "Screen",
    "Xor",
];

pub fn def(name: &str) -> Option<EffectDef> {
    let d = |name: &'static str, props: Vec<PropDef>, build| EffectDef {
        name,
        props,
        unclipped: false,
        link: None,
        build,
    };
    Some(match name {
        "Sharpen" => d(
            "Sharpen",
            vec![PropDef::int("Amount", 2, 1, 20)],
            |v: &[Value], _, _| {
                Box::new(LocalHistogram {
                    radius: v[0].int(),
                    apply: |c: ColorBgra, h: &[[u32; 256]; 4], area| {
                        // Median (3.36 GetPercentile 50, with its off-by-one fixed as in
                        // Median Blur), then pushed away from it.
                        let min_count = area as i64 / 2;
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
                        let m = ColorBgra::from_bgra(
                            pick(&h[0]),
                            pick(&h[1]),
                            pick(&h[2]),
                            pick(&h[3]),
                        );
                        lerp_c(c, m, -0.5)
                    },
                })
            },
        ),
        "Red Eye Removal" => d(
            "Red Eye Removal",
            vec![
                PropDef::int("Tolerance", 70, 0, 100),
                PropDef::int("Saturation percentage", 90, 0, 100),
            ],
            |v: &[Value], _, _| {
                let (tol, sat) = (v[0].int(), v[1].int() as f64 / 100.0);
                Box::new(At(move |src: &Surface, x: u32, y: u32| {
                    let c = src.get(x, y);
                    let (r, g, b) = (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
                    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
                    let s = if mx == 0.0 || mx == mn {
                        0.0
                    } else {
                        (mx - mn) / mx
                    };
                    let saturation = (s * 255.0) as i32;
                    let difference = c.r as i32 - c.g.max(c.b) as i32;
                    if difference > tol && saturation > 100 {
                        let i = 255.0 * (0.114 * b + 0.587 * g + 0.299 * r);
                        ColorBgra::from_bgra(c.b, c.g, (i * sat) as u8, c.a)
                    } else {
                        c
                    }
                }))
            },
        ),
        "Soften Portrait" => d(
            "Soften Portrait",
            vec![
                PropDef::int("Softness", 5, 0, 10),
                PropDef::int("Lighting", 0, -20, 20),
                PropDef::int("Warmth", 10, 0, 20),
            ],
            |v: &[Value], _, _| {
                Box::new(SoftenPortrait {
                    blur: Separable::gaussian((v[0].int() * 3).max(0) as usize),
                    bc: BrightnessContrast::new(v[1].int(), -v[1].int() / 2),
                    warmth: v[2].int(),
                })
            },
        ),
        "Vignette" => d(
            "Vignette",
            vec![
                PropDef::float("Offset X", 0.0, -1.0, 1.0, 2),
                PropDef::float("Offset Y", 0.0, -1.0, 1.0, 2),
                PropDef::float("Radius", 0.5, 0.1, 4.0, 2),
                PropDef::float("Density", 1.0, 0.0, 1.0, 2),
            ],
            |v: &[Value], _, env: &Env| {
                let b = env.bounds;
                let (w, h) = (b.width as f64, b.height as f64);
                let cx = b.x as f64 + w * (1.0 + v[0].float()) * 0.5;
                let cy = b.y as f64 + h * (1.0 + v[1].float()) * 0.5;
                let mut radius = w.max(h) * 0.5 * v[2].float();
                radius *= radius;
                let radius_r = std::f64::consts::PI / (8.0 * radius);
                let amount = v[3].float();
                let amount1 = 1.0 - amount;
                Box::new(At(move |src: &Surface, x: u32, y: u32| {
                    let c = src.get(x, y);
                    let (ix, iy) = (x as f64 - cx, y as f64 - cy);
                    let d = (iy * iy + ix * ix) * radius_r;
                    let mut f = d.cos();
                    let k = if f <= 0.0 || d > std::f64::consts::PI {
                        amount1
                    } else {
                        f *= f;
                        f *= f;
                        amount1 + amount * f
                    };
                    let ch = |v: u8| (0.5 + 255.0 * to_srgb_clamped(to_linear(v) * k)) as u8;
                    ColorBgra::from_bgra(ch(c.b), ch(c.g), ch(c.r), c.a)
                }))
            },
        ),
        "Straighten" => d(
            "Straighten",
            vec![PropDef::float("Angle", 0.0, -45.0, 45.0, 2)],
            |v: &[Value], _, env: &Env| {
                let a = v[0].float().to_radians();
                let b = env.bounds;
                let (w, h) = (b.width as f64, b.height as f64);
                let (c, s) = (a.cos().abs(), a.sin().abs());
                // Zoom so the rotated image covers the whole area.
                let zoom = ((w * c + h * s) / w).max((w * s + h * c) / h);
                let (cx, cy) = (b.x as f64 + w / 2.0, b.y as f64 + h / 2.0);
                let (sin, cos) = a.sin_cos();
                Box::new(At(move |src: &Surface, x: u32, y: u32| {
                    let (dx, dy) = ((x as f64 - cx) / zoom, (y as f64 - cy) / zoom);
                    // Inverse rotation.
                    let (sx, sy) = (cx + dx * cos + dy * sin, cy - dx * sin + dy * cos);
                    bilinear_clamped(src, sx as f32, sy as f32)
                }))
            },
        ),
        "Clouds" | "Turbulence" => {
            let turb = name == "Turbulence";
            EffectDef {
                name: if turb { "Turbulence" } else { "Clouds" },
                props: vec![
                    PropDef::int("Scale", 250, 2, 1000),
                    PropDef::float("Roughness", 0.5, 0.0, 1.0, 2),
                    PropDef::choice("Blend mode", 0, &BLEND_NAMES),
                    PropDef::int("Seed", 0, 0, 255),
                ],
                unclipped: false,
                link: None,
                build: if turb {
                    |v: &[Value], src: &Surface, env: &Env| clouds(v, src, env, true)
                } else {
                    |v: &[Value], src: &Surface, env: &Env| clouds(v, src, env, false)
                },
            }
        }
        "Julia Fractal" => d(
            "Julia Fractal",
            vec![
                PropDef::float("Factor", 4.0, 1.0, 10.0, 2),
                PropDef::float("Zoom", 1.0, 0.1, 50.0, 2),
                PropDef::float("Angle", 0.0, -180.0, 180.0, 2),
                PropDef::int("Quality", 2, 1, 5),
            ],
            |v: &[Value], src, _| {
                let (factor, zoom, angle, q) = (
                    v[0].float(),
                    v[1].float(),
                    v[2].float().to_radians(),
                    v[3].int(),
                );
                let (w, h) = (src.width() as f64, src.height() as f64);
                Box::new(At(move |_: &Surface, x: u32, y: u32| {
                    let count = q * q + 1;
                    let (mut r, mut g, mut b, mut a) = (0i32, 0i32, 0i32, 0i32);
                    for i in 0..count {
                        let i = i as f64;
                        let u = (2.0 * x as f64 - w + i / count as f64) / h;
                        let v = (2.0 * y as f64 - h + ((i / q as f64) % 1.0)) / h;
                        let rad = (u * u + v * v).sqrt();
                        let th = v.atan2(u) + angle;
                        let (up, vp) = (rad * th.cos(), rad * th.sin());
                        let aspect = h / w;
                        let j = julia(
                            (up - vp * aspect) / zoom,
                            (vp + up * aspect) / zoom,
                            0.3125,
                            0.03,
                        );
                        let c = factor * j;
                        let cl = |v: f64| v.clamp(0.0, 255.0) as i32;
                        b += cl(c - 768.0);
                        g += cl(c - 512.0);
                        r += cl(c - 256.0);
                        a += cl(c);
                    }
                    let n = count;
                    ColorBgra::from_bgra((b / n) as u8, (g / n) as u8, (r / n) as u8, (a / n) as u8)
                }))
            },
        ),
        "Mandelbrot Fractal" => d(
            "Mandelbrot Fractal",
            vec![
                PropDef::int("Factor", 1, 1, 10),
                PropDef::float("Zoom", 10.0, 0.0, 100.0, 2),
                PropDef::float("Angle", 0.0, -180.0, 180.0, 2),
                PropDef::int("Quality", 2, 1, 5),
                PropDef::bool("Invert colors", false),
            ],
            |v: &[Value], src, _| {
                let factor = v[0].int();
                let zoom = 1.0 + 20.0 * v[1].float();
                let (angle, q, invert) = (v[2].float().to_radians(), v[3].int(), v[4].bool());
                let (w, h) = (src.width() as f64, src.height() as f64);
                Box::new(At(move |_: &Surface, x: u32, y: u32| {
                    let count = q * q + 1;
                    let (mut r, mut g, mut b, mut a) = (0i32, 0i32, 0i32, 0i32);
                    for i in 0..count {
                        let i = i as f64;
                        let u = (2.0 * x as f64 - w + i / count as f64) / h;
                        let v = (2.0 * y as f64 - h + ((i / q as f64) % 1.0)) / h;
                        let rad = (u * u + v * v).sqrt();
                        let th = v.atan2(u) + angle;
                        let (up, vp) = (rad * th.cos(), rad * th.sin());
                        let m = mandelbrot(up / zoom - 0.7, vp / zoom - 0.29, factor);
                        let c = 64.0 + factor as f64 * m;
                        let cl = |v: f64| v.clamp(0.0, 255.0) as i32;
                        r += cl(c - 768.0);
                        g += cl(c - 512.0);
                        b += cl(c - 256.0);
                        a += cl(c);
                    }
                    let n = count;
                    let c = ColorBgra::from_bgra(
                        (b / n) as u8,
                        (g / n) as u8,
                        (r / n) as u8,
                        (a / n) as u8,
                    );
                    if invert {
                        ColorBgra::from_bgra(255 - c.b, 255 - c.g, 255 - c.r, c.a)
                    } else {
                        c
                    }
                }))
            },
        ),
        "Drop Shadow" => d(
            "Drop Shadow",
            vec![
                PropDef::int("Shadow radius", 5, 0, 100),
                PropDef::int("Distance", 10, 0, 200),
                PropDef::float("Angle", 45.0, -180.0, 180.0, 2),
                PropDef::int("Opacity", 50, 0, 100),
                PropDef::bool("Only draw shadow", false),
                // Any colour (BoltBait's Drop Shadow Plus offered primary or secondary).
                PropDef::color("Color", ColorDefault::Primary, false),
            ],
            |v: &[Value], _, _: &Env| {
                let (dist, ang) = (v[1].int() as f64, v[2].float().to_radians());
                Box::new(DropShadow {
                    blur: Separable::gaussian(v[0].int().max(0) as usize),
                    // Light from the angle; the shadow falls opposite (down-right at 45°).
                    offset: [
                        (dist * ang.cos()).round() as i64,
                        (dist * ang.sin()).round() as i64,
                    ],
                    color: v[5].color(),
                    opacity: v[3].int() as f64 / 100.0,
                    only_shadow: v[4].bool(),
                })
            },
        ),
        "Quantize" => d(
            "Quantize",
            vec![
                PropDef::int("Colors", 256, 2, 256),
                PropDef::int("Dithering", 8, 0, 8),
            ],
            |v: &[Value], src, env: &Env| {
                Box::new(Quantize {
                    palette: median_cut(src, env.bounds, v[0].int().clamp(2, 256) as usize),
                    dither: v[1].int(),
                })
            },
        ),
        _ => return None,
    })
}

fn clouds(v: &[Value], src: &Surface, env: &Env, turb: bool) -> Box<dyn Renderer> {
    let (scale, power, mode, seed) = (v[0].int(), v[1].float(), v[2].choice(), v[3].int() as u8);
    let (w, h) = (src.width(), src.height());
    let (from, to) = (env.primary, env.secondary);
    let blend = BlendMode::ALL[mode.min(13)];
    Box::new(At(move |src: &Surface, x: u32, y: u32| {
        let val = clouds_value(x, y, w, h, scale, power, seed, turb);
        let t = if turb {
            val.min(1.0)
        } else {
            (val + 1.0) / 2.0
        };
        let c = lerp_c(from, to, t);
        if blend == BlendMode::Normal && from.a == 255 && to.a == 255 {
            c
        } else {
            blend.apply(src.get(x, y), c, 255)
        }
    }))
}

#[cfg(test)]
mod tests {

    /// Skipping the cardioid and bulb gives the same colour channels as iterating them.
    #[test]
    fn mandelbrot_interior_shortcut_is_exact() {
        let channels = |m: f64, factor: i32| {
            let c = 64.0 + factor as f64 * m;
            let cl = |v: f64| v.clamp(0.0, 255.0) as i32;
            [cl(c - 768.0), cl(c - 512.0), cl(c - 256.0), cl(c)]
        };
        for factor in [1, 3, 10] {
            for j in 0..200 {
                for k in 0..300 {
                    let (r, i) = (-2.2 + k as f64 * 0.01, -1.0 + j as f64 * 0.01);
                    assert_eq!(
                        channels(mandelbrot(r, i, factor), factor),
                        channels(mandelbrot_iterate(r, i, factor), factor),
                        "({r}, {i}) factor {factor}"
                    );
                }
            }
        }
    }

    use super::*;
    use crate::framework::render_rect;

    fn sample(w: u32, h: u32) -> Surface {
        let mut s = Surface::new_transparent(w, h);
        for y in 0..h {
            for x in 0..w {
                let inside = x > w / 4 && x < 3 * w / 4 && y > h / 4 && y < 3 * h / 4;
                let c = if inside {
                    ColorBgra::from_rgba(200, (x * 3) as u8, 60, 255)
                } else {
                    ColorBgra::TRANSPARENT
                };
                s.set(x, y, c);
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
        render_rect(d.renderer(&values, src, &env).as_ref(), src, all)
    }

    #[test]
    fn every_effect_runs() {
        let src = sample(48, 32);
        for name in [
            "Sharpen",
            "Red Eye Removal",
            "Soften Portrait",
            "Vignette",
            "Straighten",
            "Clouds",
            "Turbulence",
            "Julia Fractal",
            "Mandelbrot Fractal",
            "Drop Shadow",
            "Quantize",
        ] {
            let d = def(name).unwrap();
            assert_eq!(run(name, d.defaults(), &src).len(), 48 * 32, "{name}");
        }
    }

    #[test]
    fn drop_shadow_falls_down_right_under_the_object() {
        let src = sample(60, 60);
        let out = run(
            "Drop Shadow",
            vec![
                Value::Int(0),
                Value::Int(6),
                Value::Float(45.0),
                Value::Int(100),
                Value::Bool(false),
            ],
            &src,
        );
        // Just past the object's bottom-right corner: shadow (black), not transparent.
        let p = out[48 * 60 + 48];
        assert!(p.a > 200 && p.r < 10, "{p:?}");
        // Top-left outside stays clear; the object itself is unchanged.
        assert_eq!(out[10 * 60 + 10].a, 0);
        assert_eq!(out[30 * 60 + 30], src.get(30, 30));
    }

    #[test]
    fn quantize_limits_colours_and_straighten_zero_is_identity() {
        let mut s = Surface::new_transparent(64, 16);
        for y in 0..16 {
            for x in 0..64 {
                s.set(x, y, ColorBgra::from_rgba((x * 4) as u8, 100, 50, 255));
            }
        }
        let q = run("Quantize", vec![Value::Int(4), Value::Int(0)], &s);
        let mut colours: Vec<u32> = q.iter().map(|c| c.to_u32()).collect();
        colours.sort_unstable();
        colours.dedup();
        assert!(colours.len() <= 4, "{}", colours.len());
        let st = run("Straighten", vec![Value::Float(0.0)], &s);
        assert_eq!(st, s.to_pixels());
    }

    #[test]
    fn clouds_are_seeded_and_span_both_colours() {
        let src = Surface::new_transparent(64, 64);
        let a = run("Clouds", def("Clouds").unwrap().defaults(), &src);
        assert_eq!(a, run("Clouds", def("Clouds").unwrap().defaults(), &src));
        let (lo, hi) = a
            .iter()
            .fold((255u8, 0u8), |(l, h), c| (l.min(c.r), h.max(c.r)));
        assert!(lo < 110 && hi > 145, "{lo}..{hi}");
    }
}
