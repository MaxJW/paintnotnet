//! Gradient rendering.
//!
//! Ported from paint.net 3.36 `Core/GradientRenderer.cs` and `Core/GradientRenderers.cs`
//! (MIT, see NOTICE): the per-type lerp maths, the 256-entry colour/alpha tables built with
//! `ColorBgra.Blend`, transparency mode (start alpha → inverse of end alpha), the start = end
//! solid fill and the truncating `(byte)(t * 255)`. Pixels are sampled at their integer
//! (top-left) coordinates, as 3.36 does.
//!
//! paintnotnet change (Phase 15): 3.36 looks each pixel up in a 256-entry table, so a wide
//! gradient shows 256 flat bands (15 px each across a 4K canvas). Colours are now mixed per
//! pixel in floating point (the same alpha weighting as `ColorBgra.Blend`) and dithered to 8
//! bits with interleaved gradient noise, which removes the bands without visible grain.
//!
//! paint.net 5 additions, built from its docs (`GradientTool.html`), not from source: the
//! two spiral types and the Repeat Wrapped / Repeat Reflected modes. Their exact maths are
//! unpublished; here a spiral's value is `angle / 2π + distance / length` around the start
//! nub (wrapping), and the repeat modes wrap or mirror the unbounded value of the other
//! types.

use pnn_core::{BlendMode, ColorBgra, Mask, Rect, Surface, TILE_SIZE};

use crate::brush::for_each_tile_row;
use crate::{GradientRepeat, GradientType};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientType,
    pub repeat: GradientRepeat,
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub start_color: ColorBgra,
    pub end_color: ColorBgra,
    /// Transparency mode: only alpha changes.
    pub alpha_only: bool,
}

/// How the gradient lands on the layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientOp {
    Blend(BlendMode),
    Overwrite,
}

/// 3.36 `Utility.FastScaleByteByByte`.
fn scale(a: u8, b: u8) -> u8 {
    let r1 = a as u32 * b as u32 + 0x80;
    (((r1 >> 8) + r1) >> 8) as u8
}

/// 3.36 `ColorBgra.Blend(ca, cb, cbAlpha)`.
fn color_blend(ca: ColorBgra, cb: ColorBgra, cb_alpha: u8) -> ColorBgra {
    let ca_a = scale(255 - cb_alpha, ca.a) as u32;
    let cb_a = scale(cb_alpha, cb.a) as u32;
    let total = ca_a + cb_a;
    if total == 0 {
        return ColorBgra::from_bgra(0, 0, 0, 0);
    }
    let ch = |a: u8, b: u8| ((a as u32 * ca_a + b as u32 * cb_a) / total) as u8;
    ColorBgra::from_bgra(ch(ca.b, cb.b), ch(ca.g, cb.g), ch(ca.r, cb.r), total as u8)
}

/// `color_blend` at a continuous position `t` (0–1), unrounded: (b, g, r, a) in 0–255.
fn color_mix(ca: ColorBgra, cb: ColorBgra, t: f32) -> [f32; 4] {
    let ca_a = (1.0 - t) * ca.a as f32;
    let cb_a = t * cb.a as f32;
    let total = ca_a + cb_a;
    if total <= 0.0 {
        return [0.0; 4];
    }
    let ch = |a: u8, b: u8| (a as f32 * ca_a + b as f32 * cb_a) / total;
    [ch(ca.b, cb.b), ch(ca.g, cb.g), ch(ca.r, cb.r), total]
}

fn alpha_only_table(g: &Gradient) -> bool {
    g.alpha_only
}

/// Interleaved gradient noise (Jimenez 2014) in -0.5..0.5: an even, fine-grained dither
/// that doesn't form patterns.
#[inline]
fn dither(x: u32, y: u32) -> f32 {
    // Arguments are non-negative, so truncation is floor (`fract`/`round` are library calls
    // on baseline x86-64 and tripled the gradient's cost).
    let frac = |v: f32| v - (v as u32) as f32;
    let v = 0.067_110_56 * x as f32 + 0.005_837_15 * y as f32;
    frac(52.982_918 * frac(v)) - 0.5
}

#[inline]
fn quantize(v: f32, d: f32) -> u8 {
    // Round to nearest: +0.5 then truncate (the clamp keeps it non-negative).
    (v + d + 0.5).clamp(0.0, 255.0) as u8
}

impl Gradient {
    /// 3.36 `ComputeAlphaOnlyValuesFromColors`.
    fn alphas(&self) -> (u8, u8) {
        if self.alpha_only {
            (self.start_color.a, 255 - self.end_color.a)
        } else {
            (self.start_color.a, self.end_color.a)
        }
    }

    /// Position along the gradient, 0..=1, of pixel (x, y).
    pub fn lerp(&self, x: f32, y: f32) -> f32 {
        Lerp::new(self).at(x, y)
    }

    /// Render over `r` of `layer` from its pre-gradient copy `before`, clipped to the
    /// (soft) selection.
    pub fn render(
        &self,
        layer: &mut Surface,
        before: &Surface,
        selection: Option<&Mask>,
        r: Rect,
        op: GradientOp,
    ) {
        let (start_a, end_a) = self.alphas();
        // A solid fill (start = end) is the end of 3.36's tables, exactly.
        let solid_color = color_blend(self.start_color, self.end_color, 255);
        let solid_alpha = end_a;
        // Unrounded colours at 4096 positions (16× finer than 3.36's table, so the dither
        // has nothing to band against).
        const STEPS: usize = 4096;
        let table: Vec<[f32; 4]> = if alpha_only_table(self) {
            Vec::new()
        } else {
            (0..STEPS)
                .map(|i| {
                    color_mix(
                        self.start_color,
                        self.end_color,
                        i as f32 / (STEPS - 1) as f32,
                    )
                })
                .collect()
        };
        let step = |t: f32| ((t * (STEPS - 1) as f32) + 0.5) as usize;
        let solid = self.start == self.end;
        let lerp = Lerp::new(self);
        let op_mode = match op {
            GradientOp::Blend(m) => Some(m),
            GradientOp::Overwrite => None,
        };
        let alpha_only = self.alpha_only;
        for_each_tile_row(layer, r, |y, x0, row| {
            // Pre-gradient pixels and selection coverage for this row, straight from tiles.
            let n = row.len();
            let c = pnn_core::TileCoord {
                tx: x0 / TILE_SIZE,
                ty: y / TILE_SIZE,
            };
            let (ox, oy) = ((x0 % TILE_SIZE) as usize, y % TILE_SIZE);
            match before.tile(c) {
                Some(t) => row.copy_from_slice(&t.row(oy)[ox..ox + n]),
                None => row.fill(before.absent()),
            }
            let cov: Option<&[u8]> = selection.map(|m| match m.tile(c) {
                Some(t) => &t.row(oy)[ox..ox + n],
                None => &[0u8; TILE_SIZE as usize][..n],
            });
            if cov.is_some_and(|c| c.iter().all(|&v| v == 0)) {
                return;
            }
            let old: Vec<ColorBgra> = if cov.is_some() {
                row.to_vec()
            } else {
                Vec::new()
            };
            // Position along the gradient, unrounded (1 for a solid fill).
            let t = |i: usize| -> f32 {
                if solid {
                    1.0
                } else {
                    lerp.at((x0 + i as u32) as f32, y as f32).clamp(0.0, 1.0)
                }
            };
            if alpha_only {
                for (i, p) in row.iter_mut().enumerate() {
                    let a = if solid {
                        solid_alpha
                    } else {
                        let v = start_a as f32 + (end_a as f32 - start_a as f32) * t(i);
                        quantize(v, dither(x0 + i as u32, y))
                    };
                    *p = p.with_alpha(if op_mode.is_some() { scale(p.a, a) } else { a });
                }
            } else {
                let src: Vec<ColorBgra> = (0..n)
                    .map(|i| {
                        if solid {
                            return solid_color;
                        }
                        let [b, g, r, a] = table[step(t(i))];
                        let d = dither(x0 + i as u32, y);
                        ColorBgra::from_bgra(
                            quantize(b, d),
                            quantize(g, d),
                            quantize(r, d),
                            quantize(a, d),
                        )
                    })
                    .collect();
                match op_mode {
                    Some(m) => m.apply_row(row, &src, 255),
                    None => row.copy_from_slice(&src),
                }
            }
            if let Some(cov) = cov {
                for ((p, &c), &o) in row.iter_mut().zip(cov).zip(&old) {
                    if c != 255 {
                        *p = pnn_core::color::lerp_premultiplied(o, *p, c);
                    }
                }
            }
        });
    }
}

/// A gradient as plain numbers, for drawing it on the GPU (`pnn-render`'s compositor draws
/// the live gradient while its nubs are dragged): the same constants `Gradient::render`
/// computes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientShader {
    /// Index in [`GradientType::ALL`].
    pub kind: u32,
    /// 0 no repeat, 1 wrapped, 2 reflected.
    pub repeat: u32,
    pub start: [f32; 2],
    pub dtdx: f32,
    pub dtdy: f32,
    pub inv_len: f32,
    pub offset: f32,
    /// Start = end: a solid fill.
    pub solid: bool,
    pub alpha_only: bool,
    pub start_a: u8,
    pub end_a: u8,
    pub start_color: ColorBgra,
    pub end_color: ColorBgra,
    pub solid_color: ColorBgra,
    /// How it lands on the layer: blended in this mode, or (`None`) overwriting.
    pub op: Option<BlendMode>,
}

impl Gradient {
    pub fn shader(&self, op: GradientOp) -> GradientShader {
        let lerp = Lerp::new(self);
        let (start_a, end_a) = self.alphas();
        GradientShader {
            kind: GradientType::ALL
                .iter()
                .position(|&k| k == self.kind)
                .unwrap_or(0) as u32,
            repeat: match self.repeat {
                GradientRepeat::NoRepeat => 0,
                GradientRepeat::Wrapped => 1,
                GradientRepeat::Reflected => 2,
            },
            start: lerp.start,
            dtdx: lerp.dtdx,
            dtdy: lerp.dtdy,
            inv_len: lerp.inv_len,
            offset: lerp.offset,
            solid: self.start == self.end,
            alpha_only: self.alpha_only,
            start_a,
            end_a,
            start_color: self.start_color,
            end_color: self.end_color,
            solid_color: color_blend(self.start_color, self.end_color, 255),
            op: match op {
                GradientOp::Blend(m) => Some(m),
                GradientOp::Overwrite => None,
            },
        }
    }
}

/// The per-gradient constants of `Gradient::lerp` (3.36's `BeforeRender`).
struct Lerp {
    kind: GradientType,
    repeat: GradientRepeat,
    start: [f32; 2],
    dtdx: f32,
    dtdy: f32,
    /// Radial: 1 / length; spirals: 1 / length.
    inv_len: f32,
    /// Conical: `tOffset`; spirals: the end direction's angle.
    offset: f32,
}

impl Lerp {
    fn new(g: &Gradient) -> Self {
        let [sx, sy] = g.start;
        let [ex, ey] = g.end;
        let (vx, vy) = (ex - sx, ey - sy);
        let mag2 = vx * vx + vy * vy;
        let len = mag2.sqrt();
        // `LinearBase.BeforeRender`.
        let dtdx = if ex == sx { 0.0 } else { vx / mag2 };
        let dtdy = if ey == sy { 0.0 } else { vy / mag2 };
        let offset = match g.kind {
            // `Conical.BeforeRender`: the end point truncated to integers, as 3.36 does.
            GradientType::Conical => {
                -((ey.trunc() - sy).atan2(ex.trunc() - sx) * std::f32::consts::FRAC_1_PI)
            }
            _ => vy.atan2(vx),
        };
        let inv_len = match g.kind {
            GradientType::Radial if len == 0.0 => 0.0,
            GradientType::Radial => 1.0 / len,
            _ => 1.0 / len.max(1e-6),
        };
        Self {
            kind: g.kind,
            repeat: g.repeat,
            start: g.start,
            dtdx,
            dtdy,
            inv_len,
            offset,
        }
    }

    #[inline]
    fn at(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (x - self.start[0], y - self.start[1]);
        let unbounded = match self.kind {
            GradientType::Linear => dx * self.dtdx + dy * self.dtdy,
            GradientType::LinearReflected => (dx * self.dtdx + dy * self.dtdy).abs(),
            GradientType::Diamond => {
                let l1 = dx * self.dtdx + dy * self.dtdy;
                let l2 = dx * self.dtdy - dy * self.dtdx;
                l1.abs() + l2.abs()
            }
            GradientType::Radial => (dx * dx + dy * dy).sqrt() * self.inv_len,
            GradientType::Conical => {
                let t = dy.atan2(dx) * std::f32::consts::FRAC_1_PI + self.offset;
                let t = if t > 1.0 {
                    t - 2.0
                } else if t < -1.0 {
                    t + 2.0
                } else {
                    t
                };
                return t.abs().clamp(0.0, 1.0);
            }
            GradientType::SpiralClockwise | GradientType::SpiralCounterClockwise => {
                let mut a = (dy.atan2(dx) - self.offset) / std::f32::consts::TAU;
                if self.kind == GradientType::SpiralCounterClockwise {
                    a = -a;
                }
                let u = a + (dx * dx + dy * dy).sqrt() * self.inv_len;
                return match self.repeat {
                    GradientRepeat::Reflected => mirror(u),
                    _ => u - u.floor(),
                };
            }
        };
        match self.repeat {
            GradientRepeat::NoRepeat => unbounded.clamp(0.0, 1.0),
            GradientRepeat::Wrapped => unbounded - unbounded.floor(),
            GradientRepeat::Reflected => mirror(unbounded),
        }
    }
}

/// Triangle wave: 0→1 over [0, 1], back to 0 over [1, 2], and so on.
fn mirror(u: f32) -> f32 {
    let m = u.rem_euclid(2.0);
    if m > 1.0 { 2.0 - m } else { m }
}

/// 3.36 `GradientTool.ConstrainPoints`: snap `b` around `a` to 15° steps.
pub fn constrain(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = ((b[0] - a[0]) as f64, (b[1] - a[1]) as f64);
    let theta = dy.atan2(dx);
    let len = (dx * dx + dy * dy).sqrt();
    let theta = (12.0 * theta / std::f64::consts::PI).round() * std::f64::consts::PI / 12.0;
    [
        (a[0] as f64 + len * theta.cos()) as f32,
        (a[1] as f64 + len * theta.sin()) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(kind: GradientType) -> Gradient {
        Gradient {
            kind,
            repeat: GradientRepeat::NoRepeat,
            start: [0.0, 0.0],
            end: [100.0, 0.0],
            start_color: ColorBgra::BLACK,
            end_color: ColorBgra::WHITE,
            alpha_only: false,
        }
    }

    #[test]
    fn linear_types() {
        let l = g(GradientType::Linear);
        assert_eq!(l.lerp(50.0, 30.0), 0.5);
        assert_eq!(l.lerp(-20.0, 0.0), 0.0);
        assert_eq!(l.lerp(150.0, 0.0), 1.0);
        let r = g(GradientType::LinearReflected);
        assert_eq!(r.lerp(-50.0, 0.0), 0.5);
        let d = g(GradientType::Diamond);
        assert_eq!(d.lerp(25.0, 25.0), 0.5);
        let rad = g(GradientType::Radial);
        assert_eq!(rad.lerp(30.0, 40.0), 0.5);
    }

    #[test]
    fn conical_is_zero_along_the_end_direction_and_one_opposite() {
        let c = g(GradientType::Conical);
        assert_eq!(c.lerp(50.0, 0.0), 0.0);
        assert!((c.lerp(-50.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((c.lerp(0.0, 50.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn repeat_modes() {
        let mut l = g(GradientType::Linear);
        l.repeat = GradientRepeat::Wrapped;
        assert!((l.lerp(125.0, 0.0) - 0.25).abs() < 1e-6);
        l.repeat = GradientRepeat::Reflected;
        assert!((l.lerp(125.0, 0.0) - 0.75).abs() < 1e-6);
        assert!((l.lerp(-25.0, 0.0) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn spirals_turn_opposite_ways() {
        let cw = g(GradientType::SpiralClockwise);
        let ccw = g(GradientType::SpiralCounterClockwise);
        // A quarter turn (y down = clockwise on screen) at half the length.
        let (a, b) = (cw.lerp(0.0, 50.0), ccw.lerp(0.0, 50.0));
        assert!((a - 0.75).abs() < 1e-5, "{a}");
        assert!((b - 0.25).abs() < 1e-5, "{b}");
    }

    #[test]
    fn colour_table_matches_paint_net_blend() {
        // ColorBgra.Blend(black, white, 128): 127/128 split of 255 → 128 grey.
        let c = color_blend(ColorBgra::BLACK, ColorBgra::WHITE, 128);
        assert_eq!((c.r, c.a), (128, 255));
        let t = color_blend(
            ColorBgra::from_rgba(255, 0, 0, 0),
            ColorBgra::from_rgba(0, 0, 255, 0),
            100,
        );
        assert_eq!(t, ColorBgra::from_bgra(0, 0, 0, 0));
    }

    #[test]
    fn render_modes() {
        let full = Rect {
            x: 0,
            y: 0,
            width: 101,
            height: 1,
        };
        let mut before = Surface::new_transparent(101, 1);
        for x in 0..101 {
            before.set(x, 0, ColorBgra::from_rgba(0, 200, 0, 255));
        }
        // Colour mode, overwrite: exact table colours.
        let mut layer = before.clone();
        g(GradientType::Linear).render(&mut layer, &before, None, full, GradientOp::Overwrite);
        assert_eq!(layer.get(0, 0), ColorBgra::BLACK);
        assert_eq!(layer.get(100, 0), ColorBgra::WHITE);
        // Half way: t = 0.5 exactly, so 127.5 dithered to 127 or 128.
        assert!(
            matches!(layer.get(50, 0).r, 127 | 128),
            "{:?}",
            layer.get(50, 0)
        );
        // Transparency mode: alpha from primary alpha (255) to inverse secondary alpha (0).
        let mut t = g(GradientType::Linear);
        t.alpha_only = true;
        let mut layer = before.clone();
        t.render(
            &mut layer,
            &before,
            None,
            full,
            GradientOp::Blend(BlendMode::Normal),
        );
        assert_eq!(layer.get(0, 0).a, 255);
        assert_eq!(layer.get(100, 0).a, 0);
        assert_eq!(layer.get(50, 0).g, 200, "colour untouched");
        // Selection clips.
        let mut sel: Mask = pnn_core::TiledBuffer::new(101, 1, 0);
        sel.set(10, 0, 255);
        let mut layer = before.clone();
        g(GradientType::Linear).render(
            &mut layer,
            &before,
            Some(&sel),
            full,
            GradientOp::Overwrite,
        );
        assert_eq!(layer.get(11, 0), before.get(11, 0));
        assert_ne!(layer.get(10, 0), before.get(10, 0));
    }

    /// Across a 4096 px black-to-white gradient, 64 rows averaged per column follow the
    /// ideal ramp to within a third of a level: no bands. 3.36's table truncated each pixel to
    /// one of 256 steps, so its columns sit up to a whole level below the ramp.
    #[test]
    fn wide_gradients_do_not_band() {
        let (w, h) = (4096u32, 64u32);
        let full = Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let before = Surface::new_transparent(w, h);
        let mut layer = before.clone();
        let mut grad = g(GradientType::Linear);
        grad.end = [w as f32 - 1.0, 0.0];
        grad.render(&mut layer, &before, None, full, GradientOp::Overwrite);
        let mut worst_new = 0.0f32;
        let mut worst_old = 0.0f32;
        for x in 0..w {
            let ideal = grad.lerp(x as f32, 0.0) * 255.0;
            let mean = (0..h).map(|y| layer.get(x, y).r as f32).sum::<f32>() / h as f32;
            worst_new = worst_new.max((mean - ideal).abs());
            let old = color_blend(ColorBgra::BLACK, ColorBgra::WHITE, (ideal) as u8).r as f32;
            worst_old = worst_old.max((old - ideal).abs());
        }
        assert!(
            worst_new < 0.34,
            "dithered columns off the ramp by {worst_new}"
        );
        assert!(worst_old > 0.9, "the 3.36 table was off by {worst_old}");
    }

    #[test]
    fn constrain_snaps_to_15_degrees() {
        let b = constrain([0.0, 0.0], [100.0, 10.0]);
        assert!((b[1]).abs() < 1e-3 && (b[0] - 100.498).abs() < 1e-2);
        let b = constrain([0.0, 0.0], [100.0, 30.0]); // 16.7° → 15°
        assert!((b[1] / b[0] - 15f32.to_radians().tan()).abs() < 1e-4);
    }

    #[test]
    #[ignore = "timing; run with --release --ignored --nocapture"]
    fn render_4k() {
        let (w, h) = (3840, 2160);
        let before = Surface::new_transparent(w, h);
        let mut layer = before.clone();
        let r = Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        for kind in GradientType::ALL {
            let mut spec = g(kind);
            spec.start = [1000.0, 800.0];
            spec.end = [2500.0, 1500.0];
            let t = std::time::Instant::now();
            spec.render(
                &mut layer,
                &before,
                None,
                r,
                GradientOp::Blend(BlendMode::Normal),
            );
            println!("{kind:?}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
    }
}
