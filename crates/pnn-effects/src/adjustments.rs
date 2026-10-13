//! The Adjustments menu.
//!
//! Ported from paint.net 3.36 (MIT, see NOTICE): `Core/UnaryPixelOps.cs` (`Invert`,
//! `InvertWithAlpha`, `Desaturate`, `Level`), `Effects/SepiaEffect.cs`,
//! `Effects/PosterizeAdjustment.cs`. paint.net 5's Invert Alpha keeps the colour and inverts
//! only alpha (docs: `AdjustmentsMenu.html`).

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
        Adjustment::Sepia => no_props(|_, _, _| {
            let levels = level_curve([0; 3], [255; 3], [1.2, 1.0, 0.8], [0; 3], [255; 3]);
            Box::new(PixelOp(move |c: ColorBgra| {
                let i = intensity(c) as usize;
                ColorBgra::from_bgra(levels.b[i], levels.g[i], levels.r[i], c.a)
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
        _ => return None,
    })
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
}
