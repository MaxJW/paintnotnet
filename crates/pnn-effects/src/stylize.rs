//! Effects > Noise, Stylize, Artistic, and Photo > Glow (which Ink Sketch builds on).
//!
//! Ported from paint.net 3.36 (MIT, see NOTICE): `Effects/AddNoiseEffect.cs`,
//! `ReduceNoiseEffect.cs`, `ColorDifferenceEffect.cs` + `EdgeDetectEffect.cs` /
//! `ReliefEffect.cs`, `EmbossEffect.cs`, `OutlineEffect.cs`, `GlowEffect.cs`,
//! `OilPaintingEffect.cs`, `PencilSketchEffect.cs`, and `InkSketchEffect.cs` (by David Issel
//! "BoltBait", MIT, contributed to paint.net 3.36).
//!
//! Differences: Add Noise draws its random numbers from a hash of the pixel position and a
//! Seed setting (3.36 used the clock), so tiles agree; paint.net 5's Edge Detect offers
//! Prewitt/Sobel kernels, here it is 3.36's angle-based kernel.

use pnn_core::{BlendMode, ColorBgra, Rect, Surface};

use crate::adjustments::BrightnessContrast;
use crate::blurs::{LocalHistogram, Separable, tent};
use crate::distort::hash01;
use crate::framework::{EffectDef, PropDef, Renderer, Value, intensity};

/// The 3×3 direction kernel 3.36 builds for Edge Detect, Relief and Emboss.
fn angle_kernel(angle: f64, centre: f64) -> [[f64; 3]; 3] {
    let r = angle.to_radians();
    let dr = std::f64::consts::FRAC_PI_4;
    [
        [(r + dr).cos(), (r + 2.0 * dr).cos(), (r + 3.0 * dr).cos()],
        [r.cos(), centre, (r + 4.0 * dr).cos()],
        [(r - dr).cos(), (r - 2.0 * dr).cos(), (r - 3.0 * dr).cos()],
    ]
}

/// 3.36 `ColorDifferenceEffect` (per channel) or Emboss (on intensity, +128).
struct Kernel3 {
    w: [[f64; 3]; 3],
    emboss: bool,
}

impl Renderer for Kernel3 {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let (sw, sh) = (src.width() as i64, src.height() as i64);
        for (i, o) in out.iter_mut().enumerate() {
            let x = (x0 + i as u32) as i64;
            let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
            for fy in 0..3i64 {
                let yy = y as i64 - 1 + fy;
                if yy < 0 || yy >= sh {
                    continue;
                }
                for fx in 0..3i64 {
                    let xx = x - 1 + fx;
                    if xx < 0 || xx >= sw {
                        continue;
                    }
                    let w = self.w[fy as usize][fx as usize];
                    let c = src.get(xx as u32, yy as u32);
                    if self.emboss {
                        r += w * intensity(c) as f64;
                    } else {
                        r += w * c.r as f64;
                        g += w * c.g as f64;
                        b += w * c.b as f64;
                    }
                }
            }
            *o = if self.emboss {
                let v = (r as i32 + 128).clamp(0, 255) as u8;
                ColorBgra::from_bgra(v, v, v, 255)
            } else {
                let c = |v: f64| (v as i32).clamp(0, 255) as u8;
                ColorBgra::from_bgra(c(b), c(g), c(r), 255)
            };
        }
    }
}

/// 3.36 Add Noise's normal-distribution lookup (`InitLookup`).
fn noise_lookup() -> &'static [i32] {
    static LOOKUP: std::sync::OnceLock<Vec<i32>> = std::sync::OnceLock::new();
    LOOKUP.get_or_init(|| {
        const N: usize = 16384;
        let normal = |x: f64, scale: f64| scale * (-x * x / 2.0).exp();
        let at = |i: usize| 16.0 * (i as f64 - N as f64 / 2.0) / N as f64;
        let (mut l, mut r, mut scale) = (5.0, 10.0, 50.0);
        while r - l > 0.0000001 {
            let mut sum = 0.0;
            scale = (l + r) * 0.5;
            for i in 0..N {
                sum += normal(at(i), scale);
                if sum > 1_000_000.0 {
                    break;
                }
            }
            if sum > N as f64 {
                r = scale;
            } else if sum < N as f64 {
                l = scale;
            } else {
                break;
            }
        }
        let mut lookup = vec![0i32; N];
        let (mut sum, mut rounded) = (0.0, 0usize);
        for i in 0..N {
            sum += normal(at(i), scale);
            let last = rounded;
            rounded = sum as usize;
            let v = ((i as i64 - N as i64 / 2) * 65536 / N as i64) as i32;
            for slot in &mut lookup[last.min(N)..rounded.min(N)] {
                *slot = v;
            }
        }
        lookup
    })
}

/// 3.36 Glow: blurred, brightness/contrast adjusted, screened over the original.
pub(crate) struct Glow {
    blur: Separable,
    bc: BrightnessContrast,
}

impl Glow {
    pub(crate) fn new(radius: i32, brightness: i32, contrast: i32) -> Self {
        Self {
            blur: Separable {
                r: radius.max(0) as usize,
                kernel: tent,
            },
            bc: BrightnessContrast::new(brightness, contrast),
        }
    }
}

impl Renderer for Glow {
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
        for (j, row) in out.chunks_mut(w).enumerate() {
            let blurred = row.to_vec();
            // Brightness/contrast in place, then screen(original, adjusted).
            let mut tmp = Surface::new_transparent(w as u32, 1);
            for (i, c) in blurred.iter().enumerate() {
                tmp.set(i as u32, 0, *c);
            }
            self.bc.render_row(&tmp, 0, 0, row);
            for (i, o) in row.iter_mut().enumerate() {
                let s = src.get(rect.x + i as u32, rect.y + j as u32);
                *o = BlendMode::Screen.apply(s, *o, 255);
            }
        }
    }
}

/// 3.36 Ink Sketch.
struct InkSketch {
    glow: Glow,
    outline: i32,
}

const INK: [[i32; 5]; 5] = [
    [-1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1],
    [-1, -1, 30, -1, -1],
    [-1, -1, -1, -1, -1],
    [-1, -1, -5, -1, -1],
];

impl Renderer for InkSketch {
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
        self.glow.render_rect(src, rect, out);
        let (sw, sh) = (src.width() as i32, src.height() as i32);
        let w = rect.width as usize;
        for (k, o) in out.iter_mut().enumerate() {
            let (x, y) = (
                rect.x as i32 + (k % w) as i32,
                rect.y as i32 + (k / w) as i32,
            );
            let (mut r, mut g, mut b) = (0i32, 0i32, 0i32);
            for v in (y - 2).max(0)..(y + 3).min(sh) {
                for u in (x - 2).max(0)..(x + 3).min(sw) {
                    let wt = INK[(v - y + 2) as usize][(u - x + 2) as usize];
                    let c = src.get(u as u32, v as u32);
                    r += c.r as i32 * wt;
                    g += c.g as i32 * wt;
                    b += c.b as i32 * wt;
                }
            }
            let top = ColorBgra::from_bgra(
                b.clamp(0, 255) as u8,
                g.clamp(0, 255) as u8,
                r.clamp(0, 255) as u8,
                255,
            );
            let i = intensity(top);
            let top = if i as i32 > self.outline * 255 / 100 {
                ColorBgra::from_bgra(255, 255, 255, 255)
            } else {
                ColorBgra::from_bgra(0, 0, 0, 255)
            };
            *o = BlendMode::Darken.apply(top, *o, 255);
        }
    }
}

/// 3.36 Pencil Sketch.
struct PencilSketch {
    blur: Separable,
    bc: BrightnessContrast,
}

impl Renderer for PencilSketch {
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
        let mut tmp = Surface::new_transparent(w as u32, 1);
        for (j, row) in out.chunks_mut(w).enumerate() {
            for (i, c) in row.iter().enumerate() {
                tmp.set(i as u32, 0, *c);
            }
            self.bc.render_row(&tmp, 0, 0, row);
            for (i, o) in row.iter_mut().enumerate() {
                // Invert, desaturate, then colour-dodge the desaturated source onto it.
                let inv = ColorBgra::from_bgra(255 - o.b, 255 - o.g, 255 - o.r, o.a);
                let d = intensity(inv);
                let layer = ColorBgra::from_bgra(d, d, d, inv.a);
                let s = src.get(rect.x + i as u32, rect.y + j as u32);
                let si = intensity(s);
                let grey = ColorBgra::from_bgra(si, si, si, s.a);
                *o = BlendMode::ColorDodge.apply(grey, layer, 255);
            }
        }
    }
}

/// 3.36 Oil Painting: the most common intensity bucket in the brush square, averaged.
struct OilPainting {
    brush: i32,
    coarseness: u8,
}

impl Renderer for OilPainting {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let (w, h) = (src.width() as i32, src.height() as i32);
        let n = self.coarseness as usize + 1;
        let (top, bottom) = (
            (y as i32 - self.brush).max(0),
            (y as i32 + self.brush + 1).min(h),
        );
        let mut count = vec![0u32; n];
        let mut sums = vec![[0u32; 4]; n];
        for (i, o) in out.iter_mut().enumerate() {
            let x = x0 as i32 + i as i32;
            count.iter_mut().for_each(|c| *c = 0);
            sums.iter_mut().for_each(|s| *s = [0; 4]);
            let (left, right) = ((x - self.brush).max(0), (x + self.brush + 1).min(w));
            for j in top..bottom {
                for k in left..right {
                    let c = src.get(k as u32, j as u32);
                    // FastScaleByteByByte(intensity, coarseness).
                    let r1 = intensity(c) as u32 * self.coarseness as u32 + 0x80;
                    let bucket = (((r1 >> 8) + r1) >> 8) as usize;
                    count[bucket] += 1;
                    let s = &mut sums[bucket];
                    s[0] += c.r as u32;
                    s[1] += c.g as u32;
                    s[2] += c.b as u32;
                    s[3] += c.a as u32;
                }
            }
            let (mut chosen, mut most) = (0usize, 0u32);
            for (b, &c) in count.iter().enumerate() {
                if c > most {
                    chosen = b;
                    most = c;
                }
            }
            let s = sums[chosen];
            let m = most.max(1);
            *o = ColorBgra::from_bgra(
                (s[2] / m) as u8,
                (s[1] / m) as u8,
                (s[0] / m) as u8,
                (s[3] / m) as u8,
            );
        }
    }
}

/// 3.36 Outline's two-percentile spread per channel.
fn outline_pixel(h: &[[u32; 256]; 4], area: u32, intensity_pct: i32) -> ColorBgra {
    let min1 = area as i64 * (100 - intensity_pct) as i64 / 200;
    let min2 = area as i64 * (100 + intensity_pct) as i64 / 200;
    let spread = |hist: &[u32; 256], skip: &[u32; 256]| {
        let mut count = 0i64;
        let mut v1 = 0usize;
        while v1 < 255 && skip[v1] == 0 {
            v1 += 1;
        }
        while v1 < 255 && count < min1 {
            count += hist[v1] as i64;
            v1 += 1;
        }
        let mut v2 = v1;
        while v2 < 255 && count < min2 {
            count += hist[v2] as i64;
            v2 += 1;
        }
        (v1, v2)
    };
    let (b1, b2) = spread(&h[0], &h[0]);
    let (g1, g2) = spread(&h[1], &h[1]);
    let (r1, r2) = spread(&h[2], &h[2]);
    // 3.36 skips empty bins of the *blue* histogram for alpha (kept as is).
    let (_, a2) = spread(&h[3], &h[0]);
    ColorBgra::from_bgra(
        (255 - (b2 - b1)) as u8,
        (255 - (g2 - g1)) as u8,
        (255 - (r2 - r1)) as u8,
        a2 as u8,
    )
}

pub fn def(name: &str) -> Option<EffectDef> {
    let d = |name: &'static str, props: Vec<PropDef>, build| EffectDef {
        name,
        props,
        link: None,
        build,
    };
    Some(match name {
        "Add Noise" => d(
            "Add Noise",
            vec![
                PropDef::int("Intensity", 64, 0, 100),
                PropDef::int("Color Saturation", 100, 0, 400),
                PropDef::float("Coverage", 100.0, 0.0, 100.0, 2),
                PropDef::int("Seed", 0, 0, 255),
            ],
            |v: &[Value], _, _| {
                let i = v[0].int();
                let dev = i * i / 4;
                let sat = v[1].int() * 4096 / 100;
                let coverage = 0.01 * v[2].float();
                let seed = v[3].int() as u64;
                let lookup = noise_lookup();
                Box::new(crate::framework::PixelOpXY(
                    move |c: ColorBgra, x: u32, y: u32| {
                        let rnd = |k: u64| hash01(x as i64, y as i64, seed, k);
                        if rnd(0) > coverage {
                            return c;
                        }
                        let pick =
                            |k: u64| lookup[(rnd(k) * lookup.len() as f64) as usize % lookup.len()];
                        let (r, g, b) = (pick(1), pick(2), pick(3));
                        let i = (4899 * r + 9618 * g + 1867 * b) >> 14;
                        let r = i + (((r - i) * sat) >> 12);
                        let g = i + (((g - i) * sat) >> 12);
                        let b = i + (((b - i) * sat) >> 12);
                        let ch = |v: u8, n: i32| {
                            (v as i32 + ((n * dev + 32768) >> 16)).clamp(0, 255) as u8
                        };
                        ColorBgra::from_bgra(ch(c.b, b), ch(c.g, g), ch(c.r, r), c.a)
                    },
                ))
            },
        ),
        "Reduce Noise" => d(
            "Reduce Noise",
            vec![
                PropDef::int("Radius", 10, 0, 200),
                PropDef::float("Strength", 0.4, 0.0, 1.0, 2),
            ],
            |v: &[Value], _, _| {
                let strength = -0.2 * v[1].float();
                Box::new(LocalHistogram {
                    radius: v[0].int(),
                    apply: move |c: ColorBgra, h: &[[u32; 256]; 4], area| {
                        let below = |hist: &[u32; 256], v: u8| {
                            hist[..v as usize].iter().map(|&n| n as i64).sum::<i64>()
                        };
                        let area = area.max(1) as i64;
                        let n = ColorBgra::from_bgra(
                            (below(&h[0], c.b) * 255 / area) as u8,
                            (below(&h[1], c.g) * 255 / area) as u8,
                            (below(&h[2], c.r) * 255 / area) as u8,
                            255,
                        );
                        let lerp = strength
                            * (1.0
                                - 0.75
                                    * (0.114 * c.b as f64
                                        + 0.587 * c.g as f64
                                        + 0.299 * c.r as f64)
                                    / 255.0);
                        let l = |a: u8, b: u8| {
                            (a as f64 + (b as f64 - a as f64) * lerp).clamp(0.0, 255.0) as u8
                        };
                        ColorBgra::from_bgra(l(c.b, n.b), l(c.g, n.g), l(c.r, n.r), l(c.a, n.a))
                    },
                })
            },
        ),
        "Edge Detect" => d(
            "Edge Detect",
            vec![PropDef::float("Angle", 45.0, -180.0, 180.0, 2)],
            |v: &[Value], _, _| {
                Box::new(Kernel3 {
                    w: angle_kernel(v[0].float(), 0.0),
                    emboss: false,
                })
            },
        ),
        "Relief" => d(
            "Relief",
            vec![PropDef::float("Angle", 45.0, -180.0, 180.0, 2)],
            |v: &[Value], _, _| {
                Box::new(Kernel3 {
                    w: angle_kernel(v[0].float(), 1.0),
                    emboss: false,
                })
            },
        ),
        "Emboss" => d(
            "Emboss",
            vec![PropDef::float("Angle", 0.0, -180.0, 180.0, 2)],
            |v: &[Value], _, _| {
                Box::new(Kernel3 {
                    w: angle_kernel(v[0].float(), 0.0),
                    emboss: true,
                })
            },
        ),
        "Outline" => d(
            "Outline",
            vec![
                PropDef::int("Thickness", 3, 1, 200),
                PropDef::int("Intensity", 50, 0, 100),
            ],
            |v: &[Value], _, _| {
                let k = v[1].int();
                Box::new(LocalHistogram {
                    radius: v[0].int(),
                    apply: move |_, h: &[[u32; 256]; 4], area| outline_pixel(h, area, k),
                })
            },
        ),
        "Glow" => d(
            "Glow",
            vec![
                PropDef::int("Radius", 6, 1, 20),
                PropDef::int("Brightness", 10, -100, 100),
                PropDef::int("Contrast", 10, -100, 100),
            ],
            |v: &[Value], _, _| Box::new(Glow::new(v[0].int(), v[1].int(), v[2].int())),
        ),
        "Ink Sketch" => d(
            "Ink Sketch",
            vec![
                PropDef::int("Ink Outline", 50, 0, 99),
                PropDef::int("Coloring", 50, 0, 100),
            ],
            |v: &[Value], _, _| {
                let c = v[1].int();
                Box::new(InkSketch {
                    glow: Glow::new(6, -(c - 50) * 2, -(c - 50) * 2),
                    outline: v[0].int(),
                })
            },
        ),
        "Pencil Sketch" => d(
            "Pencil Sketch",
            vec![
                PropDef::int("Pencil Tip Size", 2, 1, 20),
                PropDef::int("Range", 0, -20, 20),
            ],
            |v: &[Value], _, _| {
                Box::new(PencilSketch {
                    blur: Separable {
                        r: v[0].int().max(0) as usize,
                        kernel: tent,
                    },
                    bc: BrightnessContrast::new(v[1].int(), -v[1].int()),
                })
            },
        ),
        "Oil Painting" => d(
            "Oil Painting",
            vec![
                PropDef::int("Brush size", 3, 1, 8),
                PropDef::int("Coarseness", 50, 3, 255),
            ],
            |v: &[Value], _, _| {
                Box::new(OilPainting {
                    brush: v[0].int(),
                    coarseness: v[1].int().clamp(3, 255) as u8,
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

    fn photo(w: u32, h: u32) -> Surface {
        let mut s = Surface::new_transparent(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = if (x / 6 + y / 6) % 2 == 0 { 40 } else { 210 };
                s.set(
                    x,
                    y,
                    ColorBgra::from_rgba(v, (x * 4) as u8, (y * 4) as u8, 255),
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
        };
        render_rect((d.build)(&values, src, &env).as_ref(), src, all)
    }

    #[test]
    fn every_effect_runs() {
        let src = photo(40, 30);
        for name in [
            "Add Noise",
            "Reduce Noise",
            "Edge Detect",
            "Relief",
            "Emboss",
            "Outline",
            "Glow",
            "Ink Sketch",
            "Pencil Sketch",
            "Oil Painting",
        ] {
            let d = def(name).unwrap();
            let out = run(name, d.defaults(), &src);
            assert_eq!(out.len(), 1200, "{name}");
            assert!(out != src.to_pixels(), "{name} changes the image");
        }
    }

    #[test]
    fn known_values() {
        // Emboss of a flat image is mid-grey (kernel sums to zero, +128).
        let mut flat = Surface::new_transparent(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                flat.set(x, y, ColorBgra::from_rgba(100, 100, 100, 255));
            }
        }
        let e = run("Emboss", vec![Value::Float(0.0)], &flat);
        assert_eq!(e[4 * 8 + 4].r, 128);
        // Relief keeps a flat area (centre weight 1, the rest cancel).
        let r = run("Relief", vec![Value::Float(45.0)], &flat);
        assert!((r[4 * 8 + 4].r as i32 - 100).abs() <= 1);
        // Add Noise at intensity 0 changes nothing; same seed gives the same noise.
        let n0 = run(
            "Add Noise",
            vec![
                Value::Int(0),
                Value::Int(100),
                Value::Float(100.0),
                Value::Int(0),
            ],
            &flat,
        );
        assert_eq!(n0, flat.to_pixels());
        let src = photo(20, 20);
        let a = run("Add Noise", def("Add Noise").unwrap().defaults(), &src);
        assert_eq!(
            a,
            run("Add Noise", def("Add Noise").unwrap().defaults(), &src)
        );
        // Oil painting of a flat image is the image.
        let o = run("Oil Painting", vec![Value::Int(3), Value::Int(50)], &flat);
        assert_eq!(o, flat.to_pixels());
    }
}
