//! The 14 paint.net layer blend modes.
//!
//! Ported from paint.net 3.36 `Data/UserBlendOps.Generated.H.cs` (MIT, see NOTICE). paint.net
//! uses one generalised compositing formula with a replaceable per-channel function F:
//!
//! `G(A,a,B,b,F) = (a - ab)A + (b - ab)B + abF(A,B)`, normalised by the result alpha,
//!
//! where A/a are the lower (accumulated) colour and alpha and B/b the layer being applied.
//! All arithmetic matches the original integer maths. paint.net divides with a
//! multiply-add-shift table (`masTable`). That table was verified to equal exact floor division
//! for every divisor 1..=255 over the full numerator range 0..=65025, so plain `/` is used here.

use crate::ColorBgra;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Additive,
    ColorBurn,
    ColorDodge,
    Reflect,
    Glow,
    Overlay,
    Difference,
    Negation,
    Lighten,
    Darken,
    Screen,
    Xor,
}

impl BlendMode {
    /// In paint.net's order (the Layer Properties dropdown).
    pub const ALL: [BlendMode; 14] = [
        Self::Normal,
        Self::Multiply,
        Self::Additive,
        Self::ColorBurn,
        Self::ColorDodge,
        Self::Reflect,
        Self::Glow,
        Self::Overlay,
        Self::Difference,
        Self::Negation,
        Self::Lighten,
        Self::Darken,
        Self::Screen,
        Self::Xor,
    ];

    /// `name()` of each of [`Self::ALL`], for dropdowns.
    pub const NAMES: [&'static str; 14] = [
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

    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Additive => "Additive",
            Self::ColorBurn => "Color Burn",
            Self::ColorDodge => "Color Dodge",
            Self::Reflect => "Reflect",
            Self::Glow => "Glow",
            Self::Overlay => "Overlay",
            Self::Difference => "Difference",
            Self::Negation => "Negation",
            Self::Lighten => "Lighten",
            Self::Darken => "Darken",
            Self::Screen => "Screen",
            Self::Xor => "Xor",
        }
    }

    /// Blend `src` onto `dst` in place, with `src` alpha scaled by `opacity` (255 = unchanged).
    pub fn apply_row(self, dst: &mut [ColorBgra], src: &[ColorBgra], opacity: u8) {
        // Monomorphise per mode so the inner loop has no per-pixel dispatch.
        macro_rules! run {
            ($f:path) => {
                if opacity == 255 {
                    for (d, s) in dst.iter_mut().zip(src) {
                        *d = blend(*d, *s, $f);
                    }
                } else {
                    for (d, s) in dst.iter_mut().zip(src) {
                        let s = s.with_alpha(int_scale(s.a as u32, opacity as u32) as u8);
                        *d = blend(*d, s, $f);
                    }
                }
            };
        }
        match self {
            // Opaque source pixels replace, transparent ones keep (exactly what `f_over`
            // gives; tested), skipping the blend arithmetic for most of a live gradient.
            Self::Normal if opacity == 255 => {
                for (d, s) in dst.iter_mut().zip(src) {
                    match (s.a, d.a) {
                        (255, _) => *d = *s,
                        // (A transparent destination is normalised by the blend.)
                        (0, a) if a != 0 => {}
                        _ => *d = blend(*d, *s, f_over),
                    }
                }
            }
            Self::Normal => run!(f_over),
            Self::Multiply => run!(f_multiply),
            Self::Additive => run!(f_saturate),
            Self::ColorBurn => run!(f_color_burn),
            Self::ColorDodge => run!(f_color_dodge),
            Self::Reflect => run!(f_reflect),
            Self::Glow => run!(f_glow),
            Self::Overlay => run!(f_overlay),
            Self::Difference => run!(f_difference),
            Self::Negation => run!(f_negation),
            Self::Lighten => run!(f_max),
            Self::Darken => run!(f_min),
            Self::Screen => run!(f_screen),
            Self::Xor => run!(f_xor),
        }
    }

    /// Single-pixel form of [`Self::apply_row`].
    pub fn apply(self, lhs: ColorBgra, rhs: ColorBgra, opacity: u8) -> ColorBgra {
        let mut d = [lhs];
        self.apply_row(&mut d, &[rhs], opacity);
        d[0]
    }
}

/// `INT_SCALE`: multiply two 0..=255 values as if they were 0..=1, with paint.net's rounding.
#[inline(always)]
const fn int_scale(a: u32, b: u32) -> u32 {
    let r = a * b + 0x80;
    ((r >> 8) + r) >> 8
}

/// `BLEND(lhs, rhs, F, ID, ID)`.
#[inline(always)]
fn blend(lhs: ColorBgra, rhs: ColorBgra, f: impl Fn(u32, u32) -> u32) -> ColorBgra {
    let lhs_a = lhs.a as u32;
    let rhs_a = rhs.a as u32;
    let y = int_scale(lhs_a, 255 - rhs_a);
    let total_a = y + rhs_a;
    if total_a == 0 {
        return ColorBgra::TRANSPARENT;
    }
    let x = int_scale(lhs_a, rhs_a);
    let z = rhs_a - x;
    let channel = |l: u8, r: u8| -> u8 {
        let (l, r) = (l as u32, r as u32);
        ((l * y + r * z + f(l, r) * x) / total_a) as u8
    };
    ColorBgra {
        b: channel(lhs.b, rhs.b),
        g: channel(lhs.g, rhs.g),
        r: channel(lhs.r, rhs.r),
        // COMPUTE_ALPHA(lhsA, rhsA) == y + rhsA
        a: total_a as u8,
    }
}

// Per-channel functions F(A, B): A = lower layer, B = layer being applied.

#[inline(always)]
fn f_over(_a: u32, b: u32) -> u32 {
    b
}
#[inline(always)]
fn f_multiply(a: u32, b: u32) -> u32 {
    int_scale(a, b)
}
#[inline(always)]
fn f_saturate(a: u32, b: u32) -> u32 {
    (a + b).min(255)
}
#[inline(always)]
fn f_color_burn(a: u32, b: u32) -> u32 {
    // B == 0 gives 0, as in paint.net's COLORBURN.
    ((255 - a) * 255)
        .checked_div(b)
        .map_or(0, |q| 255u32.saturating_sub(q))
}
#[inline(always)]
fn f_color_dodge(a: u32, b: u32) -> u32 {
    if b == 255 {
        255
    } else {
        (a * 255 / (255 - b)).min(255)
    }
}
#[inline(always)]
fn f_reflect(a: u32, b: u32) -> u32 {
    if b == 255 {
        255
    } else {
        (a * a / (255 - b)).min(255)
    }
}
#[inline(always)]
fn f_glow(a: u32, b: u32) -> u32 {
    f_reflect(b, a)
}
#[inline(always)]
fn f_overlay(a: u32, b: u32) -> u32 {
    if a < 128 {
        int_scale(2 * a, b)
    } else {
        255 - int_scale(2 * (255 - a), 255 - b)
    }
}
#[inline(always)]
fn f_difference(a: u32, b: u32) -> u32 {
    a.abs_diff(b)
}
#[inline(always)]
fn f_negation(a: u32, b: u32) -> u32 {
    255 - (255i32 - a as i32 - b as i32).unsigned_abs()
}
#[inline(always)]
fn f_max(a: u32, b: u32) -> u32 {
    a.max(b)
}
#[inline(always)]
fn f_min(a: u32, b: u32) -> u32 {
    a.min(b)
}
#[inline(always)]
fn f_screen(a: u32, b: u32) -> u32 {
    b + a - int_scale(b, a)
}
#[inline(always)]
fn f_xor(a: u32, b: u32) -> u32 {
    a ^ b
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Normal fast path must equal the full blend for opaque and transparent sources.
    #[test]
    fn normal_fast_path_is_exact() {
        let mut seed = 7u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 16) as u8
        };
        for _ in 0..20_000 {
            let d = ColorBgra::from_rgba(next(), next(), next(), next());
            for a in [0u8, 255] {
                let s = ColorBgra::from_rgba(next(), next(), next(), a);
                assert_eq!(
                    BlendMode::Normal.apply(d, s, 255),
                    blend(d, s, f_over),
                    "{d:?} {s:?}"
                );
            }
        }
    }

    /// Expected values generated by a straight Python transliteration of the C# macros in
    /// `UserBlendOps.Generated.H.cs`, including its `masTable` division
    /// (`scripts/blend_reference.py`).
    const VECTORS: &[(BlendMode, u32, u32, u8, u32)] = include!("blend_vectors.in");

    #[test]
    fn matches_paint_net_reference() {
        for &(mode, lhs, rhs, opacity, expected) in VECTORS {
            let got = mode.apply(ColorBgra::from_u32(lhs), ColorBgra::from_u32(rhs), opacity);
            assert_eq!(
                got.to_u32(),
                expected,
                "{mode:?} lhs={lhs:#010x} rhs={rhs:#010x} opacity={opacity}: got {:#010x}",
                got.to_u32()
            );
        }
    }

    #[test]
    fn normal_over_transparent_is_identity_for_opaque() {
        let c = ColorBgra::from_rgba(10, 20, 30, 255);
        assert_eq!(BlendMode::Normal.apply(ColorBgra::TRANSPARENT, c, 255), c);
    }

    #[test]
    fn both_transparent_gives_zero() {
        let c = ColorBgra::from_rgba(10, 20, 30, 0);
        for mode in BlendMode::ALL {
            assert_eq!(mode.apply(c, c, 255), ColorBgra::TRANSPARENT);
        }
    }
}
