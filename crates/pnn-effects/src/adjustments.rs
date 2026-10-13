//! The Adjustments menu.
//!
//! Ported from paint.net 3.36 (MIT, see NOTICE): `Core/UnaryPixelOps.cs` (`Invert`,
//! `Desaturate`, `Level`, `HueSaturationLightness`, `BlendConstant`), `Core/HsvColor.cs` /
//! `RgbColor.cs`, `Core/Histogram*.cs` (Auto-Level), `Effects/SepiaEffect.cs`,
//! `Effects/PosterizeAdjustment.cs`, `Effects/BrightnessAndContrastAdjustment.cs`,
//! `Effects/HueAndSaturationAdjustment.cs`, `Effects/AutoLevelEffect.cs`.
//!
//! From paint.net 5's docs only (`AdjustmentsMenu.html`), maths guessed: Invert Alpha (alpha
//! only), Sepia's Intensity control (mixes the 3.36 sepia with the original), Exposure,
//! Highlights / Shadows, Temperature and Tint. Hue / Saturation uses 3.36's algorithm with
//! unquantised HSV (3.36 rounded S and V to whole percent, which bands photos).

use pnn_core::ColorBgra;

use crate::Adjustment;
use crate::framework::{ChannelCurve, EffectDef, Link, PixelOp, PropDef, Value, intensity};

/// 3.36 `UnaryPixelOps.Level` lookup table for in/out ranges and per-channel gamma
/// (index 0 = blue, as `ColorBgra`'s indexer).
pub fn level_curve(
    in_lo: [u8; 3],
    in_hi: [u8; 3],
    gamma: [f32; 3],
    out_lo: [u8; 3],
    out_hi: [u8; 3],
) -> ChannelCurve {
    let mut t = [[0u8; 256]; 3];
    for (i, table) in t.iter_mut().enumerate() {
        let (lo, hi) = (in_lo[i] as f32, in_hi[i] as f32);
        let (olo, ohi) = (out_lo[i] as f64, out_hi[i] as f64);
        for (j, v) in table.iter_mut().enumerate() {
            let x = j as f32 - lo;
            *v = if x < 0.0 {
                out_lo[i]
            } else if x + lo >= hi {
                out_hi[i]
            } else {
                let p = ((x / (hi - lo)) as f64).powf(gamma[i] as f64);
                (olo + (ohi - olo) * p).clamp(0.0, 255.0) as u8
            };
        }
    }
    ChannelCurve {
        b: t[0],
        g: t[1],
        r: t[2],
    }
}

/// 3.36 `PosterizeAdjustment.CalcLevels`.
fn posterize_levels(count: i32) -> [u8; 256] {
    let n = count.clamp(2, 64) as usize;
    let t1: Vec<u8> = (0..n).map(|i| ((255 * i) / (n - 1)) as u8).collect();
    let mut levels = [0u8; 256];
    let (mut j, mut k) = (0usize, 0usize);
    for v in levels.iter_mut() {
        *v = t1[j];
        k += n;
        if k > 255 {
            k -= 255;
            j += 1;
        }
    }
    levels
}

/// Every adjustment that has been implemented; the rest return `None`.
pub fn def(a: Adjustment) -> Option<EffectDef> {
    let no_props = |build| EffectDef {
        name: a.name(),
        props: Vec::new(),
        link: None,
        build,
    };
    Some(match a {
        Adjustment::InvertColors => no_props(|_, _, _| {
            Box::new(PixelOp(|c: ColorBgra| {
                ColorBgra::from_bgra(255 - c.b, 255 - c.g, 255 - c.r, c.a)
            }))
        }),
        Adjustment::InvertAlpha => {
            no_props(|_, _, _| Box::new(PixelOp(|c: ColorBgra| c.with_alpha(255 - c.a))))
        }
        Adjustment::BlackAndWhite => no_props(|_, _, _| {
            Box::new(PixelOp(|c: ColorBgra| {
                let i = intensity(c);
                ColorBgra::from_bgra(i, i, i, c.a)
            }))
        }),
        Adjustment::Posterize => EffectDef {
            name: a.name(),
            props: vec![
                PropDef::int("Red", 16, 2, 64),
                PropDef::int("Green", 16, 2, 64),
                PropDef::int("Blue", 16, 2, 64),
                PropDef::bool("Linked", true),
            ],
            link: Some(Link {
                toggle: 3,
                values: &[0, 1, 2],
            }),
            build: |v: &[Value], _, _| {
                Box::new(ChannelCurve {
                    r: posterize_levels(v[0].int()),
                    g: posterize_levels(v[1].int()),
                    b: posterize_levels(v[2].int()),
                })
            },
        },
        Adjustment::AutoLevel => no_props(|_, src, env| Box::new(auto_level(src, env.bounds))),
        Adjustment::BrightnessContrast => EffectDef {
            name: a.name(),
            props: vec![
                PropDef::int("Brightness", 0, -100, 100),
                PropDef::int("Contrast", 0, -100, 100),
            ],
            link: None,
            build: |v: &[Value], _, _| Box::new(BrightnessContrast::new(v[0].int(), v[1].int())),
        },
        Adjustment::HueSaturation => EffectDef {
            name: a.name(),
            props: vec![
                PropDef::int("Hue", 0, -180, 180),
                PropDef::int("Saturation", 100, 0, 200),
                PropDef::int("Lightness", 0, -100, 100),
            ],
            link: None,
            build: |v: &[Value], _, _| {
                let (hue, mut sat, light) = (v[0].int(), v[1].int(), v[2].int());
                // 3.36: map [0, 100] → [0, 100] and [101, 200] → [103, 400].
                if sat > 100 {
                    sat = (sat - 100) * 3 + 100;
                }
                Box::new(PixelOp(move |c: ColorBgra| {
                    hue_saturation(c, hue, sat, light)
                }))
            },
        },
        Adjustment::Sepia => EffectDef {
            name: a.name(),
            props: vec![PropDef::int("Intensity", 100, 0, 100)],
            link: None,
            build: |v: &[Value], _, _| {
                let k = v[0].int().clamp(0, 100) as u32;
                let levels = level_curve([0; 3], [255; 3], [1.2, 1.0, 0.8], [0; 3], [255; 3]);
                Box::new(PixelOp(move |c: ColorBgra| {
                    let i = intensity(c) as usize;
                    let mix =
                        |a: u8, b: u8| ((a as u32 * (100 - k) + b as u32 * k + 50) / 100) as u8;
                    ColorBgra::from_bgra(
                        mix(c.b, levels.b[i]),
                        mix(c.g, levels.g[i]),
                        mix(c.r, levels.r[i]),
                        c.a,
                    )
                }))
            },
        },
        Adjustment::Exposure => EffectDef {
            name: a.name(),
            props: vec![PropDef::float("Exposure", 0.0, -5.0, 5.0, 2)],
            link: None,
            build: |v: &[Value], _, _| {
                // Linear-light gain of 2^EV.
                let gain = 2f64.powf(v[0].float());
                let t = table(|x| from_linear(to_linear(x) * gain));
                Box::new(ChannelCurve { b: t, g: t, r: t })
            },
        },
        Adjustment::HighlightsShadows => EffectDef {
            name: a.name(),
            props: vec![
                PropDef::int("Shadows", 0, -100, 100),
                PropDef::int("Highlights", 0, -100, 100),
            ],
            link: None,
            build: |v: &[Value], _, _| {
                let (sh, hi) = (v[0].int() as f64 / 100.0, v[1].int() as f64 / 100.0);
                // A luminance curve: shadows lift (or deepen) the dark end, highlights the
                // bright end, each fading out towards the other.
                let curve = table(|l| {
                    let s = sh * 0.5 * (1.0 - l).powi(3) * l.sqrt() * 2.0;
                    let h = hi * 0.5 * l.powi(3) * (1.0 - l).sqrt() * 2.0;
                    (l + s + h).clamp(0.0, 1.0)
                });
                Box::new(PixelOp(move |c: ColorBgra| {
                    let l = intensity(c);
                    let d = curve[l as usize] as i32 - l as i32;
                    let ch = |v: u8| (v as i32 + d).clamp(0, 255) as u8;
                    ColorBgra::from_bgra(ch(c.b), ch(c.g), ch(c.r), c.a)
                }))
            },
        },
        Adjustment::TemperatureTint => EffectDef {
            name: a.name(),
            props: vec![
                PropDef::int("Temperature", 0, -100, 100),
                PropDef::int("Tint", 0, -100, 100),
            ],
            link: None,
            build: |v: &[Value], _, _| {
                let (t, n) = (v[0].int() as f64 / 100.0, v[1].int() as f64 / 100.0);
                // Warm = more red, less blue; tint = green ↔ magenta. Gains in linear light.
                let gains = [1.0 - 0.3 * t, 1.0 - 0.3 * n, 1.0 + 0.3 * t];
                let [b, g, r] = gains.map(|k| table(|x| from_linear(to_linear(x) * k)));
                Box::new(ChannelCurve { b, g, r })
            },
        },
        _ => return None,
    })
}

/// A 256-entry table from a function on 0..=1.
fn table(f: impl Fn(f64) -> f64) -> [u8; 256] {
    std::array::from_fn(|i| (f(i as f64 / 255.0).clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
}

fn to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// 3.36 `HistogramRgb.MakeLevelsAuto` over `bounds`: in-levels at the 0.5% and 99.5%
/// percentiles, gamma from the mean (`Level.AutoFromLoMdHi`).
fn auto_level(src: &pnn_core::Surface, bounds: pnn_core::Rect) -> ChannelCurve {
    let mut hist = [[0u64; 256]; 3];
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let c = src.get(x, y);
            hist[0][c.b as usize] += 1;
            hist[1][c.g as usize] += 1;
            hist[2][c.r as usize] += 1;
        }
    }
    let percentile = |h: &[u64; 256], f: f32| -> u8 {
        let sum: u64 = h.iter().sum();
        let mut integral = 0u64;
        for (j, &n) in h.iter().enumerate() {
            integral += n;
            if integral as f32 > sum as f32 * f {
                return j as u8;
            }
        }
        0
    };
    let mean = |h: &[u64; 256]| -> u8 {
        let sum: u64 = h.iter().sum();
        let avg: u64 = h.iter().enumerate().map(|(j, &n)| j as u64 * n).sum();
        let m = if sum == 0 {
            0.0
        } else {
            avg as f32 / sum as f32
        };
        (m + 0.5) as u8
    };
    let lo = [0, 1, 2].map(|i| percentile(&hist[i], 0.005));
    let md = [0, 1, 2].map(|i| mean(&hist[i]));
    let hi = [0, 1, 2].map(|i| percentile(&hist[i], 0.995));
    let gamma = [0, 1, 2].map(|i| {
        if lo[i] < md[i] && md[i] < hi[i] {
            let base = (md[i] - lo[i]) as f32 / (hi[i] - lo[i]) as f32;
            (0.5f64.ln() / (base as f64).ln()).clamp(0.1, 10.0) as f32
        } else {
            1.0
        }
    });
    level_curve(lo, hi, gamma, [0; 3], [255; 3])
}

/// 3.36 `BrightnessAndContrastAdjustment`: a table indexed by (intensity, channel value).
struct BrightnessContrast {
    table: Vec<u8>,
    /// Contrast +100: threshold on intensity.
    threshold: bool,
}

impl BrightnessContrast {
    fn new(brightness: i32, contrast: i32) -> Self {
        let (multiply, divide) = match contrast {
            c if c < 0 => (c + 100, 100),
            c if c > 0 => (100, 100 - c),
            _ => (1, 1),
        };
        let mut table = vec![0u8; 65536];
        if divide == 0 {
            for i in 0..256i32 {
                table[i as usize] = if i + brightness < 128 { 0 } else { 255 };
            }
        } else {
            for i in 0..256i32 {
                let shift = if divide == 100 {
                    (i - 127) * multiply / divide + 127 - i + brightness
                } else {
                    (i - 127 + brightness) * multiply / divide + 127 - i
                };
                for col in 0..256i32 {
                    table[(i * 256 + col) as usize] = (col + shift).clamp(0, 255) as u8;
                }
            }
        }
        Self {
            table,
            threshold: divide == 0,
        }
    }
}

impl crate::framework::Renderer for BrightnessContrast {
    fn render_row(&self, src: &pnn_core::Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (k, o) in out.iter_mut().enumerate() {
            let c = src.get(x0 + k as u32, y);
            let i = intensity(c) as usize;
            *o = if self.threshold {
                let v = self.table[i];
                ColorBgra::from_bgra(v, v, v, c.a)
            } else {
                let t = &self.table[i * 256..i * 256 + 256];
                ColorBgra::from_bgra(t[c.b as usize], t[c.g as usize], t[c.r as usize], c.a)
            };
        }
    }
}

/// 3.36 `UnaryPixelOps.HueSaturationLightness` (`sat` already mapped to 0..=400).
fn hue_saturation(c: ColorBgra, hue: i32, sat: i32, lightness: i32) -> ColorBgra {
    if hue == 0 && sat == 100 && lightness == 0 {
        return c;
    }
    let sat_factor = sat * 1024 / 100;
    let i = intensity(c) as i32;
    let s = |v: u8| ((i * 1024 + (v as i32 - i) * sat_factor) >> 10).clamp(0, 255);
    let (r, g, b) = (
        s(c.r) as f64 / 255.0,
        s(c.g) as f64 / 255.0,
        s(c.b) as f64 / 255.0,
    );
    // RgbColor.ToHsv / HsvColor.ToRgb, without rounding H, S, V to integers.
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let v = max;
    let (mut h, s_) = if max == 0.0 || delta == 0.0 {
        (0.0, 0.0)
    } else {
        let h = if r == max {
            (g - b) / delta
        } else if g == max {
            2.0 + (b - r) / delta
        } else {
            4.0 + (r - g) / delta
        };
        let h = h * 60.0;
        (if h < 0.0 { h + 360.0 } else { h }, delta / max)
    };
    h = (h + hue as f64).rem_euclid(360.0);
    let (r, g, b) = if s_ == 0.0 {
        (v, v, v)
    } else {
        let sector = h / 60.0;
        let n = sector.floor() as i32;
        let f = sector - n as f64;
        let (p, q, t) = (
            v * (1.0 - s_),
            v * (1.0 - s_ * f),
            v * (1.0 - s_ * (1.0 - f)),
        );
        match n {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        }
    };
    let (mut r, mut g, mut b) = ((r * 255.0) as i32, (g * 255.0) as i32, (b * 255.0) as i32);
    // `BlendConstant` towards white or black, dividing by 256 as 3.36 does.
    if lightness != 0 {
        let (target, a) = if lightness > 0 {
            (255, lightness * 255 / 100)
        } else {
            (0, -lightness * 255 / 100)
        };
        let blend = |v: i32| (v * (255 - a) + target * a) / 256;
        (r, g, b) = (blend(r), blend(g), blend(b));
    }
    ColorBgra::from_bgra(b as u8, g as u8, r as u8, c.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Env, render_rect};
    use pnn_core::{Rect, Surface};

    fn one(c: ColorBgra) -> Surface {
        let mut s = Surface::new_transparent(1, 1);
        s.set(0, 0, c);
        s
    }

    fn run(a: Adjustment, values: Option<Vec<Value>>, c: ColorBgra) -> ColorBgra {
        let d = def(a).unwrap();
        let src = one(c);
        let all = Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        let env = Env {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            bounds: all,
        };
        let v = values.unwrap_or_else(|| d.defaults());
        let r = (d.build)(&v, &src, &env);
        render_rect(r.as_ref(), &src, all)[0]
    }

    #[test]
    fn simple_adjustments() {
        let c = ColorBgra::from_rgba(200, 100, 50, 128);
        assert_eq!(
            run(Adjustment::InvertColors, None, c),
            ColorBgra::from_rgba(55, 155, 205, 128)
        );
        assert_eq!(
            run(Adjustment::InvertAlpha, None, c),
            ColorBgra::from_rgba(200, 100, 50, 127)
        );
        // Intensity: (7471·50 + 38470·100 + 19595·200) >> 16 = 124.
        assert_eq!(
            run(Adjustment::BlackAndWhite, None, c),
            ColorBgra::from_rgba(124, 124, 124, 128)
        );
        let s = run(Adjustment::Sepia, None, c);
        assert!(s.r > s.g && s.g > s.b && s.g == 124, "{s:?}");
    }

    #[test]
    fn posterize_matches_3_36_tables() {
        let l = posterize_levels(2);
        assert_eq!((l[0], l[127], l[128], l[255]), (0, 0, 255, 255));
        let p = run(
            Adjustment::Posterize,
            Some(vec![
                Value::Int(2),
                Value::Int(64),
                Value::Int(2),
                Value::Bool(false),
            ]),
            ColorBgra::from_rgba(200, 100, 50, 255),
        );
        assert_eq!((p.r, p.b), (255, 0));
    }

    #[test]
    fn level_curve_matches_3_36_formula() {
        let c = level_curve([10; 3], [200; 3], [1.0; 3], [0; 3], [255; 3]);
        assert_eq!(c.r[5], 0);
        assert_eq!(c.r[200], 255);
        // (105 − 10) / 190 × 255 = 127.5 → 127 (truncated).
        assert_eq!(c.r[105], 127);
    }

    #[test]
    fn brightness_contrast_matches_3_36() {
        let c = ColorBgra::from_rgba(100, 100, 100, 255);
        let v = |b, k| {
            run(
                Adjustment::BrightnessContrast,
                Some(vec![Value::Int(b), Value::Int(k)]),
                c,
            )
            .r
        };
        assert_eq!(v(0, 0), 100);
        assert_eq!(v(20, 0), 120, "brightness shifts");
        // Contrast 50: (100 − 127) · 100 / 50 + 127 = 73.
        assert_eq!(v(0, 50), 73);
        assert_eq!(v(0, 100), 0, "contrast 100 thresholds");
        assert_eq!(v(0, -100), 127, "contrast −100 flattens to mid grey");
    }

    #[test]
    fn hue_saturation() {
        let red = ColorBgra::from_rgba(255, 0, 0, 255);
        let hs = |h, s, l| {
            run(
                Adjustment::HueSaturation,
                Some(vec![Value::Int(h), Value::Int(s), Value::Int(l)]),
                red,
            )
        };
        assert_eq!(hs(0, 100, 0), red, "identity");
        let g = hs(120, 100, 0);
        assert!(
            g.g > 240 && g.r < 10 && g.b < 10,
            "red rotated to green: {g:?}"
        );
        let grey = hs(0, 0, 0);
        assert!(
            grey.r == grey.g && grey.g == grey.b,
            "desaturated: {grey:?}"
        );
        assert!(hs(0, 100, 100).g > 250, "lightness 100 → white");
    }

    #[test]
    fn auto_level_stretches_a_narrow_range() {
        let mut s = Surface::new_transparent(100, 1);
        for x in 0..100 {
            let v = 100 + (x / 2) as u8; // 100..=149
            s.set(x, 0, ColorBgra::from_rgba(v, v, v, 255));
        }
        let all = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 1,
        };
        let env = Env {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            bounds: all,
        };
        let d = def(Adjustment::AutoLevel).unwrap();
        let r = (d.build)(&[], &s, &env);
        let out = render_rect(r.as_ref(), &s, all);
        assert!(
            out[0].r < 10 && out[99].r > 245,
            "{:?} {:?}",
            out[0],
            out[99]
        );
    }

    #[test]
    fn photo_adjustments_are_identity_at_zero_and_move_the_right_way() {
        let c = ColorBgra::from_rgba(120, 130, 140, 255);
        for a in [
            Adjustment::Exposure,
            Adjustment::HighlightsShadows,
            Adjustment::TemperatureTint,
        ] {
            assert_eq!(run(a, None, c), c, "{a:?} default is identity");
        }
        assert!(run(Adjustment::Exposure, Some(vec![Value::Float(1.0)]), c).g > 160);
        let dark = ColorBgra::from_rgba(40, 40, 40, 255);
        let lifted = run(
            Adjustment::HighlightsShadows,
            Some(vec![Value::Int(100), Value::Int(0)]),
            dark,
        );
        assert!(lifted.r > 50, "shadows lifted: {lifted:?}");
        let warm = run(
            Adjustment::TemperatureTint,
            Some(vec![Value::Int(100), Value::Int(0)]),
            c,
        );
        assert!(warm.r > c.r && warm.b < c.b);
        let half = run(Adjustment::Sepia, Some(vec![Value::Int(50)]), c);
        let full = run(Adjustment::Sepia, None, c);
        assert!(half != full && half != c);
    }
}
