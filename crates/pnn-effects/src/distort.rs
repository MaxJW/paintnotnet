//! Effects > Distort.
//!
//! Ported from paint.net 3.36 (MIT, see NOTICE): `Effects/BulgeEffect.cs`, `DentsEffect.cs` +
//! `WarpEffectBase.cs` + `PerlinNoise2D.cs`, `FrostedGlassEffect.cs`, `PixelateEffect.cs`,
//! `PolarInversionEffect.cs`, `TileEffect.cs`, `TwistEffect.cs`, with `Utility.GetRgssOffsets`
//! and `Surface.GetBilinearSampleClamped` / `GetBilinearSampleWrapped`.
//!
//! Randomness: 3.36 reseeded from the clock on every render; here noise comes from a hash of
//! the pixel position and a Seed setting, so tiles rendered in any order agree and a preview
//! matches the result.
//!
//! From paint.net 5's docs only, maths guessed: Crystalize (Voronoi cells over jittered
//! points, each filled from its point's pixel) and Morphology (min/max over a square).

use pnn_core::{ColorBgra, Rect, Surface};

use crate::blurs::{bilinear, blend};
use crate::framework::{EffectDef, PropDef, Renderer, Value};

/// 3.36 `Utility.GetRgssOffsets`: rotated-grid supersampling offsets.
pub fn rgss(quality: i32) -> Vec<[f32; 2]> {
    let n = (quality * quality).max(1) as usize;
    if n == 1 {
        return vec![[0.0, 0.0]];
    }
    (0..n)
        .map(|i| {
            let y = (i as f64 + 1.0) / (n as f64 + 1.0);
            let mut x = y * quality as f64;
            x -= x.trunc();
            [(x - 0.5) as f32, (y - 0.5) as f32]
        })
        .collect()
}

/// 3.36 `GetBilinearSampleClamped`.
pub fn bilinear_clamped(src: &Surface, x: f32, y: f32) -> ColorBgra {
    if !(x.is_finite() && y.is_finite()) {
        return ColorBgra::TRANSPARENT;
    }
    let x = x.clamp(0.0, (src.width() - 1) as f32);
    let y = y.clamp(0.0, (src.height() - 1) as f32);
    bilinear(src, x, y)
}

/// 3.36 `GetBilinearSampleWrapped`.
pub fn bilinear_wrapped(src: &Surface, x: f32, y: f32) -> ColorBgra {
    if !(x.is_finite() && y.is_finite()) {
        return ColorBgra::TRANSPARENT;
    }
    let (w, h) = (src.width() as i64, src.height() as i64);
    let (iu, iv) = (x.floor() as i64, y.floor() as i64);
    let fx = (256.0 * (x - iu as f32)) as u32;
    let fy = (256.0 * (y - iv as f32)) as u32;
    let (ix, iy) = (256 - fx, 256 - fy);
    let wrap = |v: i64, n: i64| v.rem_euclid(n);
    let (sx, sy) = (wrap(iu, w), wrap(iv, h));
    let sr = if sx == w - 1 { 0 } else { sx + 1 };
    let sb = if sy == h - 1 { 0 } else { sy + 1 };
    let c = [
        (src.get(sx as u32, sy as u32), ix * iy),
        (src.get(sr as u32, sy as u32), fx * iy),
        (src.get(sx as u32, sb as u32), ix * fy),
        (src.get(sr as u32, sb as u32), fx * fy),
    ];
    blend4(&c)
}

/// 3.36 `ColorBgra.BlendColors4W16IP` (weights sum to 65536).
pub fn blend4(c: &[(ColorBgra, u32); 4]) -> ColorBgra {
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

/// Deterministic per-pixel random numbers in [0, 1).
pub fn hash01(x: i64, y: i64, seed: u64, i: u64) -> f64 {
    let mut z = (x as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
        .wrapping_add(seed.wrapping_mul(0x1656_67B1_9E37_79F9))
        .wrapping_add(i.wrapping_mul(0x27D4_EB2F_1656_67C5));
    // splitmix64 finaliser.
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// 3.36 `PerlinNoise2D`.
pub mod perlin {
    const PERM: [u8; 256] = [
        151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30,
        69, 142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94,
        252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171,
        168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60,
        211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1,
        216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86,
        164, 100, 109, 198, 173, 186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118,
        126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170,
        213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39,
        253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246, 97, 228, 251, 34,
        242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49,
        192, 214, 31, 181, 199, 106, 157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254,
        138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180,
    ];

    fn p(i: usize) -> usize {
        PERM[i & 255] as usize
    }

    fn fade(t: f64) -> f64 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    fn grad(hash: usize, x: f64, y: f64) -> f64 {
        let h = hash & 15;
        let u = if h < 8 { x } else { y };
        let v = if h < 4 {
            y
        } else if h == 12 || h == 14 {
            x
        } else {
            0.0
        };
        (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
    }

    fn noise1(mut x: f64, mut y: f64, seed: u8) -> f64 {
        let (xf, yf) = (x.floor(), y.floor());
        let (ix, iy) = ((xf as i64 & 255) as usize, (yf as i64 & 255) as usize);
        x -= xf;
        y -= yf;
        let (u, v) = (fade(x), fade(y));
        let s = seed as usize;
        // permuteLookup is the table twice over, so indices below 512 wrap at 256.
        let a = p(ix + s) + iy;
        let (aa, ab) = (p(a), p(a + 1));
        let b = p(ix + 1 + s) + iy;
        let (ba, bb) = (p(b), p(b + 1));
        let lerp = |a: f64, b: f64, t: f64| a + t * (b - a);
        let e1 = lerp(grad(p(aa), x, y), grad(p(ba), x - 1.0, y), u);
        let e2 = lerp(grad(p(ab), x, y - 1.0), grad(p(bb), x - 1.0, y - 1.0), u);
        lerp(e1, e2, v)
    }

    /// One noise cell at lattice (ix, iy) with fractional (x, y): 3.36 Clouds' `Noise`.
    pub fn noise_cell(ix: u8, iy: u8, x: f64, y: f64, seed: u8) -> f64 {
        let (u, v) = (fade(x), fade(y));
        let (ix, iy, s) = (ix as usize, iy as usize, seed as usize);
        let a = p(ix + s) + iy;
        let (aa, ab) = (p(a), p(a + 1));
        let b = p(ix + 1 + s) + iy;
        let (ba, bb) = (p(b), p(b + 1));
        let lerp = |a: f64, b: f64, t: f64| a + t * (b - a);
        let e1 = lerp(grad(p(aa), x, y), grad(p(ba), x - 1.0, y), u);
        let e2 = lerp(grad(p(ab), x, y - 1.0), grad(p(bb), x - 1.0, y - 1.0), u);
        lerp(e1, e2, v)
    }

    /// Octaved, rotated noise as 3.36's `Noise(x, y, detail, roughness, seed)`.
    pub fn noise(mut x: f64, mut y: f64, detail: f64, roughness: f64, seed: u8) -> f64 {
        let a = 137.2f64.to_radians();
        let (r11, r12, r21, r22) = (a.cos(), -a.sin(), a.sin(), a.cos());
        let (mut total, mut frequency, mut amplitude) = (0.0, 1.0, 1.0);
        let mut partial = detail;
        for _ in 0..detail.ceil() as i32 {
            let xr = x * r11 + y * r12;
            let yr = x * r21 + y * r22;
            let mut n = noise1(xr * frequency, yr * frequency, seed) * amplitude;
            if partial < 1.0 {
                n *= partial;
            }
            total += n;
            amplitude *= roughness;
            if amplitude < 0.001 {
                break;
            }
            frequency += frequency;
            partial -= 1.0;
            x = xr + 499.0;
            y = yr + 506.0;
        }
        total
    }
}

/// 3.36 `WarpEdgeBehavior` (the ones its warps offer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Clamp,
    Reflect,
    Wrap,
}

/// 3.36 `WarpEffectBase`: supersampled inverse mapping around the selection's centre.
struct Warp<F: Fn(f64, f64) -> (f64, f64) + Send + Sync> {
    centre: [f64; 2],
    quality: i32,
    edge: Edge,
    inverse: F,
}

fn reflect(v: f32, max: f32) -> f32 {
    let (mut v, mut flip) = (v, false);
    while v < 0.0 {
        v += max;
        flip = !flip;
    }
    while v > max {
        v -= max;
        flip = !flip;
    }
    if flip { max - v } else { v }
}

impl<F: Fn(f64, f64) -> (f64, f64) + Send + Sync> Renderer for Warp<F> {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let aa = rgss(self.quality);
        let (w, h) = ((src.width() - 1) as f32, (src.height() - 1) as f32);
        let ry = y as f64 - self.centre[1];
        let mut samples = Vec::with_capacity(aa.len());
        for (i, o) in out.iter_mut().enumerate() {
            let rx = (x0 + i as u32) as f64 - self.centre[0];
            samples.clear();
            for p in &aa {
                let (tx, ty) = (self.inverse)(rx + p[0] as f64, ry - p[1] as f64);
                let (sx, sy) = ((tx + self.centre[0]) as f32, (ty + self.centre[1]) as f32);
                samples.push(if sx >= 0.0 && sx <= w && sy >= 0.0 && sy <= h {
                    bilinear(src, sx, sy)
                } else {
                    match self.edge {
                        Edge::Clamp => bilinear_clamped(src, sx, sy),
                        Edge::Wrap => bilinear_wrapped(src, sx, sy),
                        Edge::Reflect => bilinear_clamped(
                            src,
                            reflect(sx, src.width() as f32),
                            reflect(sy, src.height() as f32),
                        ),
                    }
                });
            }
            *o = blend(&samples);
        }
    }
}

/// `WarpEffectBase.OnSetRenderInfo`: the centre and default radius from the selection.
fn warp_frame(bounds: Rect, offset: [f64; 2]) -> ([f64; 2], f64) {
    let (w, h) = (bounds.width as f64, bounds.height as f64);
    let radius = w.min(h) * 0.5;
    (
        [
            bounds.x as f64 + w * (1.0 + offset[0]) * 0.5,
            bounds.y as f64 + h * (1.0 + offset[1]) * 0.5,
        ],
        radius,
    )
}

const EDGES: &[&str] = &["Clamp", "Reflect", "Wrap"];

fn edge(v: Value) -> Edge {
    [Edge::Clamp, Edge::Reflect, Edge::Wrap][v.choice().min(2)]
}

/// Generic per-pixel renderer from a closure over (src, x, y).
struct PerPixel<F: Fn(&Surface, u32, u32) -> ColorBgra + Send + Sync>(F);

impl<F: Fn(&Surface, u32, u32) -> ColorBgra + Send + Sync> Renderer for PerPixel<F> {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = (self.0)(src, x0 + i as u32, y);
        }
    }
}

/// Square min/max per channel (Morphology), separable.
struct Morph {
    r: usize,
    dilate: bool,
}

impl Renderer for Morph {
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
        let r = self.r as i64;
        let (sw, sh) = (src.width() as i64, src.height() as i64);
        let pick = |a: u8, b: u8| if self.dilate { a.max(b) } else { a.min(b) };
        let comb = |a: ColorBgra, b: ColorBgra| {
            ColorBgra::from_bgra(
                pick(a.b, b.b),
                pick(a.g, b.g),
                pick(a.r, b.r),
                pick(a.a, b.a),
            )
        };
        let x0 = (rect.x as i64 - r).max(0);
        let x1 = (rect.x as i64 + rect.width as i64 + r).min(sw);
        // Vertical pass over the extended columns, for the output rows.
        let cols = (x1 - x0) as usize;
        let mut v = vec![ColorBgra::TRANSPARENT; cols * rect.height as usize];
        for j in 0..rect.height as i64 {
            let y = rect.y as i64 + j;
            for (c, x) in (x0..x1).enumerate() {
                let mut acc = src.get(x as u32, y as u32);
                for yy in (y - r).max(0)..=(y + r).min(sh - 1) {
                    acc = comb(acc, src.get(x as u32, yy as u32));
                }
                v[j as usize * cols + c] = acc;
            }
        }
        for j in 0..rect.height as usize {
            for i in 0..rect.width as usize {
                let x = rect.x as i64 + i as i64;
                let mut acc = v[j * cols + (x - x0) as usize];
                for xx in (x - r).max(x0)..=(x + r).min(x1 - 1) {
                    acc = comb(acc, v[j * cols + (xx - x0) as usize]);
                }
                out[j * rect.width as usize + i] = acc;
            }
        }
    }
}

pub fn def(name: &str) -> Option<EffectDef> {
    let d = |name: &'static str, props: Vec<PropDef>, build| EffectDef {
        name,
        props,
        unclipped: false,
        link: None,
        build,
    };
    Some(match name {
        "Bulge" => d(
            "Bulge",
            vec![
                PropDef::int("Bulge", 45, -200, 100),
                PropDef::float("Offset X", 0.0, -1.0, 1.0, 2),
                PropDef::float("Offset Y", 0.0, -1.0, 1.0, 2),
            ],
            |v: &[Value], src, _| {
                let amt = v[0].int() as f32 / 100.0;
                let (ox, oy) = (v[1].float() as f32, v[2].float() as f32);
                let (hw0, hh0) = (src.width() as f32 / 2.0, src.height() as f32 / 2.0);
                let maxrad = hw0.min(hh0);
                let (hw, hh) = (hw0 + ox * hw0, hh0 + oy * hh0);
                Box::new(PerPixel(move |src: &Surface, x: u32, y: u32| {
                    let (u, v) = (x as f32 - hw, y as f32 - hh);
                    let r = (u * u + v * v).sqrt();
                    let s1 = 1.0 - r / maxrad;
                    if s1 > 0.0 {
                        let s2 = 1.0 - amt * s1 * s1;
                        bilinear_clamped(src, u * s2 + hw, v * s2 + hh)
                    } else {
                        src.get(x, y)
                    }
                }))
            },
        ),
        "Dents" => d(
            "Dents",
            vec![
                PropDef::float("Scale", 25.0, 1.0, 200.0, 2),
                PropDef::float("Refraction", 50.0, 0.0, 200.0, 2),
                PropDef::float("Roughness", 10.0, 0.0, 100.0, 2),
                PropDef::float("Tension", 10.0, 0.0, 100.0, 2),
                PropDef::int("Quality", 2, 1, 5),
                PropDef::int("Seed", 0, 0, 255),
            ],
            |v: &[Value], _, env| {
                let (centre, radius) = warp_frame(env.bounds, [0.0, 0.0]);
                let (scale, refraction, rough, tension) =
                    (v[0].float(), v[1].float(), v[2].float(), v[3].float());
                let seed = v[5].int() as u8;
                let scale_r = (400.0 / radius) / scale;
                let refraction_scale = (refraction / 100.0) / scale_r;
                let theta = std::f64::consts::PI * 2.0 * tension / 10.0;
                let roughness = rough / 100.0;
                let detail3 = 1.0 + rough / 10.0;
                let max_detail = (scale_r.ln() / 0.5f64.ln()).floor();
                let detail = if detail3 > max_detail && max_detail >= 1.0 {
                    max_detail
                } else {
                    detail3
                };
                Box::new(Warp {
                    centre,
                    quality: v[4].int(),
                    edge: Edge::Wrap,
                    inverse: move |x: f64, y: f64| {
                        let a = theta
                            * perlin::noise(x * scale_r, y * scale_r, detail, roughness, seed);
                        (
                            x + refraction_scale * (-a).sin(),
                            y + refraction_scale * a.cos(),
                        )
                    },
                })
            },
        ),
        "Polar Inversion" => d(
            "Polar Inversion",
            vec![
                PropDef::float("Amount", 1.0, -4.0, 4.0, 2),
                PropDef::float("Offset X", 0.0, -2.0, 2.0, 2),
                PropDef::float("Offset Y", 0.0, -2.0, 2.0, 2),
                PropDef::choice("Edge behavior", 2, EDGES),
                PropDef::int("Quality", 2, 1, 5),
            ],
            |v: &[Value], _, env| {
                let amount = v[0].float();
                let (centre, radius) = warp_frame(env.bounds, [v[1].float(), v[2].float()]);
                let r2 = radius * radius;
                Box::new(Warp {
                    centre,
                    quality: v[4].int(),
                    edge: edge(v[3]),
                    inverse: move |x: f64, y: f64| {
                        let inv = 1.0 + (r2 / (x * x + y * y) - 1.0) * amount;
                        (x * inv, y * inv)
                    },
                })
            },
        ),
        "Twist" => d(
            "Twist",
            vec![
                PropDef::float("Amount", 30.0, -200.0, 200.0, 2),
                PropDef::float("Size", 1.0, 0.01, 2.0, 2),
                PropDef::float("Offset X", 0.0, -2.0, 2.0, 2),
                PropDef::float("Offset Y", 0.0, -2.0, 2.0, 2),
                PropDef::int("Quality", 2, 1, 5),
            ],
            |v: &[Value], src, _| {
                let amount = -v[0].float();
                let twist = amount * amount * amount.signum();
                let size = 1.0 / v[1].float();
                let (w, h) = (src.width() as f32, src.height() as f32);
                let hw = w / 2.0 + (w / 2.0) * v[2].float() as f32;
                let hh = h / 2.0 + (h / 2.0) * v[3].float() as f32;
                let invmax = 1.0 / (w as f64 / 2.0).min(h as f64 / 2.0);
                let aa = rgss(v[4].int());
                Box::new(PerPixel(move |src: &Surface, x: u32, y: u32| {
                    let (i, j) = (x as f32 - hw, y as f32 - hh);
                    let s: Vec<ColorBgra> = aa
                        .iter()
                        .map(|p| {
                            let (u, v) = (i + p[0], j + p[1]);
                            let rad = ((u * u + v * v) as f64).sqrt();
                            let mut theta = (v as f64).atan2(u as f64);
                            let t = 1.0 - rad * size * invmax;
                            let t = if t < 0.0 { 0.0 } else { t * t * t };
                            theta += t * twist / 100.0;
                            bilinear_clamped(
                                src,
                                hw + (rad * theta.cos()) as f32,
                                hh + (rad * theta.sin()) as f32,
                            )
                        })
                        .collect();
                    blend(&s)
                }))
            },
        ),
        "Tile Reflection" => d(
            "Tile Reflection",
            vec![
                PropDef::float("Angle", 30.0, -180.0, 180.0, 2),
                PropDef::float("Tile Size", 40.0, 1.0, 800.0, 2),
                PropDef::float("Curvature", 8.0, -100.0, 100.0, 2),
                PropDef::int("Quality", 2, 1, 5),
            ],
            |v: &[Value], src, _| {
                let rot = -v[0].float();
                let (sin, cos) = (
                    (rot.to_radians().sin()) as f32,
                    (rot.to_radians().cos()) as f32,
                );
                let scale = (std::f64::consts::PI / v[1].float()) as f32;
                let c = v[2].float();
                let intensity = (c * c / 10.0 * c.signum()) as f32;
                let mut q = v[3].int();
                if q != 1 {
                    q += 1;
                }
                let aa = rgss(q);
                let (hw, hh) = (src.width() as f32 / 2.0, src.height() as f32 / 2.0);
                Box::new(PerPixel(move |src: &Surface, x: u32, y: u32| {
                    let (i, j) = (x as f32 - hw, y as f32 - hh);
                    let s: Vec<ColorBgra> = aa
                        .iter()
                        .map(|p| {
                            let (u1, v1) = (i + p[0], j - p[1]);
                            let s1 = cos * u1 + sin * v1;
                            let t1 = -sin * u1 + cos * v1;
                            let s2 = s1 + intensity * (s1 * scale).tan();
                            let t2 = t1 + intensity * (t1 * scale).tan();
                            let (u2, v2) = (cos * s2 - sin * t2, sin * s2 + cos * t2);
                            bilinear_wrapped(src, hw + u2, hh + v2)
                        })
                        .collect();
                    blend(&s)
                }))
            },
        ),
        "Pixelate" => d(
            "Pixelate",
            vec![PropDef::int("Cell size", 2, 1, 100)],
            |v: &[Value], _, _| {
                let cell = v[0].int().max(1) as u32;
                Box::new(PerPixel(move |src: &Surface, x: u32, y: u32| {
                    // `ComputeCellColor`: the four corners of the cell, averaged.
                    let (l, t) = (x - x % cell, y - y % cell);
                    let r = (l + cell).min(src.width()) - 1;
                    let b = (t + cell).min(src.height()) - 1;
                    blend4(&[
                        (src.get(l, t), 16384),
                        (src.get(r, t), 16384),
                        (src.get(l, b), 16384),
                        (src.get(r, b), 16384),
                    ])
                }))
            },
        ),
        "Frosted Glass" => d(
            "Frosted Glass",
            vec![
                PropDef::float("Maximum scatter radius", 3.0, 0.0, 200.0, 2),
                PropDef::float("Minimum scatter radius", 0.0, 0.0, 200.0, 2),
                PropDef::int("Smoothness", 2, 1, 8),
                PropDef::int("Seed", 0, 0, 255),
            ],
            |v: &[Value], src, _| {
                let max_r = v[0].float();
                let min_r = v[1]
                    .float()
                    .min(max_r)
                    .min(src.width().min(src.height()) as f64 / 2.0);
                let (n, seed) = (v[2].int().max(1) as u64, v[3].int() as u64);
                let (w, h) = (src.width() as f64, src.height() as f64);
                Box::new(PerPixel(move |src: &Surface, x: u32, y: u32| {
                    let s: Vec<ColorBgra> = (0..n)
                        .map(|k| {
                            let mut tries = 0u64;
                            loop {
                                let a = hash01(x as i64, y as i64, seed, k * 64 + tries)
                                    * std::f64::consts::TAU;
                                let d = min_r
                                    + hash01(x as i64, y as i64, seed ^ 0xABCD, k * 64 + tries)
                                        * (max_r - min_r);
                                let (sx, sy) = (x as f64 + a.cos() * d, y as f64 + a.sin() * d);
                                tries += 1;
                                if (sx >= 0.0 && sx <= w && sy >= 0.0 && sy <= h) || tries > 32 {
                                    break bilinear(src, sx as f32, sy as f32);
                                }
                            }
                        })
                        .collect();
                    blend(&s)
                }))
            },
        ),
        "Crystalize" => d(
            "Crystalize",
            vec![
                PropDef::int("Cell size", 8, 2, 250),
                PropDef::int("Seed", 0, 0, 255),
            ],
            |v: &[Value], _, _| {
                let (cell, seed) = (v[0].int().max(2) as i64, v[1].int() as u64);
                // One jittered point per grid cell; each pixel takes its nearest point's pixel.
                let point = move |cx: i64, cy: i64| {
                    (
                        (cx as f64 + hash01(cx, cy, seed, 1)) * cell as f64,
                        (cy as f64 + hash01(cx, cy, seed, 2)) * cell as f64,
                    )
                };
                Box::new(PerPixel(move |src: &Surface, x: u32, y: u32| {
                    let (gx, gy) = (x as i64 / cell, y as i64 / cell);
                    let mut best = (f64::INFINITY, 0.0, 0.0);
                    for cy in gy - 1..=gy + 1 {
                        for cx in gx - 1..=gx + 1 {
                            let (px, py) = point(cx, cy);
                            let d = (px - x as f64).powi(2) + (py - y as f64).powi(2);
                            if d < best.0 {
                                best = (d, px, py);
                            }
                        }
                    }
                    let sx = (best.1 as i64).clamp(0, src.width() as i64 - 1) as u32;
                    let sy = (best.2 as i64).clamp(0, src.height() as i64 - 1) as u32;
                    src.get(sx, sy)
                }))
            },
        ),
        "Morphology" => d(
            "Morphology",
            vec![
                PropDef::choice("Mode", 0, &["Dilate", "Erode"]),
                PropDef::int("Radius", 2, 1, 100),
            ],
            |v: &[Value], _, _| {
                Box::new(Morph {
                    r: v[1].int().max(1) as usize,
                    dilate: v[0].choice() == 0,
                })
            },
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Env, render_rect};

    fn gradient(w: u32, h: u32) -> Surface {
        let mut s = Surface::new_transparent(w, h);
        for y in 0..h {
            for x in 0..w {
                s.set(
                    x,
                    y,
                    ColorBgra::from_rgba((x * 255 / w) as u8, (y * 255 / h) as u8, 90, 255),
                );
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
    fn rgss_matches_3_36() {
        assert_eq!(rgss(1), vec![[0.0, 0.0]]);
        let p = rgss(2);
        assert_eq!(p.len(), 4);
        // i = 0: y = 1/5, x = frac(0.4) = 0.4 → (−0.1, −0.3).
        assert!((p[0][0] + 0.1).abs() < 1e-6 && (p[0][1] + 0.3).abs() < 1e-6);
    }

    #[test]
    fn perlin_is_deterministic_and_bounded() {
        let a = perlin::noise(3.7, 1.2, 3.0, 0.5, 7);
        assert_eq!(a, perlin::noise(3.7, 1.2, 3.0, 0.5, 7));
        assert_ne!(a, perlin::noise(3.7, 1.2, 3.0, 0.5, 8));
        for i in 0..100 {
            let v = perlin::noise(i as f64 * 0.37, i as f64 * 0.11, 2.0, 0.5, 0);
            assert!(v.abs() < 2.0);
        }
    }

    #[test]
    fn every_distortion_runs_and_neutral_settings_keep_the_image() {
        let src = gradient(48, 40);
        for name in [
            "Bulge",
            "Dents",
            "Polar Inversion",
            "Twist",
            "Tile Reflection",
            "Pixelate",
            "Frosted Glass",
            "Crystalize",
            "Morphology",
        ] {
            let d = def(name).unwrap();
            let out = run(name, d.defaults(), &src);
            assert_eq!(out.len(), 48 * 40, "{name}");
        }
        let keep = |name: &str, v: Vec<Value>| {
            let out = run(name, v, &src);
            let c = out[20 * 48 + 24];
            let o = src.get(24, 20);
            assert!(
                (c.r as i32 - o.r as i32).abs() <= 2 && (c.g as i32 - o.g as i32).abs() <= 2,
                "{name}: {c:?} vs {o:?}"
            );
        };
        keep(
            "Bulge",
            vec![Value::Int(0), Value::Float(0.0), Value::Float(0.0)],
        );
        keep(
            "Twist",
            vec![
                Value::Float(0.0),
                Value::Float(1.0),
                Value::Float(0.0),
                Value::Float(0.0),
                Value::Int(1),
            ],
        );
        keep("Pixelate", vec![Value::Int(1)]);
        keep(
            "Frosted Glass",
            vec![
                Value::Float(0.0),
                Value::Float(0.0),
                Value::Int(1),
                Value::Int(0),
            ],
        );
    }

    #[test]
    fn pixelate_and_morphology() {
        let src = gradient(40, 40);
        let out = run("Pixelate", vec![Value::Int(10)], &src);
        assert_eq!(out[5 * 40 + 5], out[2 * 40 + 8], "same cell, same colour");
        assert_ne!(out[5 * 40 + 5], out[5 * 40 + 15]);
        let mut dot = Surface::new_transparent(20, 20);
        dot.set(10, 10, ColorBgra::WHITE);
        let dil = run("Morphology", vec![Value::Choice(0), Value::Int(2)], &dot);
        assert_eq!(dil[12 * 20 + 12], ColorBgra::WHITE, "dilated 2 px");
        assert_eq!(dil[13 * 20 + 10].a, 0);
        let ero = run("Morphology", vec![Value::Choice(1), Value::Int(1)], &dot);
        assert_eq!(ero[10 * 20 + 10].a, 0, "eroded away");
    }
}
