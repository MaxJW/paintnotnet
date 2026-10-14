//! OKLab / OKLCH (Björn Ottosson, "A perceptual color space for image processing", 2020,
//! https://bottosson.github.io/posts/oklab/; the matrices are from that post) for the Colors
//! panel's sliders. Lightness and hue in OKLCH change evenly to the eye, unlike HSV.

use pnn_core::ColorBgra;

/// Lightness 0–1, chroma ≥ 0 (sRGB reaches about 0.37), hue in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

/// Highest chroma the sliders offer (sRGB's most saturated colours sit just below it).
pub const MAX_CHROMA: f32 = 0.37;

fn to_linear(v: u8) -> f32 {
    let v = f32::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB → OKLab (L, a, b).
pub fn oklab(c: ColorBgra) -> [f32; 3] {
    let (r, g, b) = (to_linear(c.r), to_linear(c.g), to_linear(c.b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab → linear sRGB (may be out of gamut).
fn linear_rgb([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

pub fn to_lch(c: ColorBgra) -> Lch {
    let [l, a, b] = oklab(c);
    Lch {
        l,
        c: a.hypot(b),
        h: b.atan2(a).to_degrees().rem_euclid(360.0),
    }
}

fn lab_of(lch: Lch) -> [f32; 3] {
    let h = lch.h.to_radians();
    [lch.l, lch.c * h.cos(), lch.c * h.sin()]
}

fn in_gamut(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v))
}

/// OKLCH → sRGB with `alpha`. Out-of-gamut colours keep their lightness and hue and lose
/// chroma until they fit (CSS Color 4's approach, by bisection).
pub fn from_lch(lch: Lch, alpha: u8) -> ColorBgra {
    let lch = Lch {
        l: lch.l.clamp(0.0, 1.0),
        c: lch.c.max(0.0),
        h: lch.h,
    };
    let mut rgb = linear_rgb(lab_of(lch));
    if !in_gamut(rgb) {
        let (mut lo, mut hi) = (0.0, lch.c);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if in_gamut(linear_rgb(lab_of(Lch { c: mid, ..lch }))) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        rgb = linear_rgb(lab_of(Lch { c: lo, ..lch }));
    }
    let byte = |v: f32| (to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
    ColorBgra::from_rgba(byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn matches_ottossons_reference_values() {
        // White is L 1 with no chroma; red and blue as colorjs.io computes them in OKLab.
        let w = oklab(ColorBgra::WHITE);
        assert!(close(w[0], 1.0, 1e-3) && close(w[1], 0.0, 1e-3) && close(w[2], 0.0, 1e-3));
        let red = oklab(ColorBgra::from_rgba(255, 0, 0, 255));
        assert!(close(red[0], 0.627_955, 1e-3), "{red:?}");
        assert!(close(red[1], 0.224_863, 1e-3) && close(red[2], 0.125_846, 1e-3));
        let blue = oklab(ColorBgra::from_rgba(0, 0, 255, 255));
        assert!(close(blue[0], 0.452_014, 1e-3) && close(blue[2], -0.311_528, 1e-3));
    }

    #[test]
    fn round_trips_every_8th_colour() {
        for r in (0..=255).step_by(32) {
            for g in (0..=255).step_by(32) {
                for b in (0..=255).step_by(32) {
                    let c = ColorBgra::from_rgba(r as u8, g as u8, b as u8, 200);
                    let back = from_lch(to_lch(c), 200);
                    for (x, y) in [(c.r, back.r), (c.g, back.g), (c.b, back.b)] {
                        assert!(x.abs_diff(y) <= 1, "{c:?} → {back:?}");
                    }
                    assert_eq!(back.a, 200);
                }
            }
        }
    }

    #[test]
    fn out_of_gamut_keeps_lightness_and_hue() {
        let wanted = Lch {
            l: 0.7,
            c: MAX_CHROMA,
            h: 150.0,
        };
        let got = to_lch(from_lch(wanted, 255));
        assert!(close(got.l, 0.7, 0.01), "{got:?}");
        assert!(close(got.h, 150.0, 2.0), "{got:?}");
        assert!(got.c < MAX_CHROMA);
    }
}
