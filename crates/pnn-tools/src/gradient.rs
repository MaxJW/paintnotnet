//! Gradient rendering.
//!
//! Ported from paint.net 3.36 `Core/GradientRenderer.cs` and `Core/GradientRenderers.cs`
//! (MIT, see NOTICE): the per-type lerp maths, the 256-entry colour/alpha tables built with
//! `ColorBgra.Blend`, transparency mode (start alpha → inverse of end alpha), the start = end
//! solid fill and the truncating `(byte)(t * 255)`. Pixels are sampled at their integer
//! (top-left) coordinates, as 3.36 does.
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
        // `GradientRenderer.BeforeRender` tables.
        let colors: Vec<ColorBgra> = (0..=255u8)
            .map(|a| color_blend(self.start_color, self.end_color, a))
            .collect();
        let alphas: Vec<u8> = (0..=255i32)
            .map(|a| (start_a as i32 + ((end_a as i32 - start_a as i32) * a) / 255) as u8)
            .collect();
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
            let k = |i: usize| -> usize {
                if solid {
                    255
                } else {
                    (lerp.at((x0 + i as u32) as f32, y as f32) * 255.0) as u8 as usize
                }
            };
            if alpha_only {
                for (i, p) in row.iter_mut().enumerate() {
                    let a = alphas[k(i)];
                    *p = p.with_alpha(if op_mode.is_some() { scale(p.a, a) } else { a });
                }
            } else {
                let src: Vec<ColorBgra> = (0..n).map(|i| colors[k(i)]).collect();
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
        assert_eq!(
            layer.get(50, 0).r,
            color_blend(ColorBgra::BLACK, ColorBgra::WHITE, 127).r
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
