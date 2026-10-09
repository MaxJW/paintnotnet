//! Layers > Rotate / Zoom: rotate, pan and zoom the active layer about its centre.
//!
//! Parameters follow paint.net's dialog (Angle, Pan, Zoom, Tiling, Preserve background;
//! 3.36 `RotateZoomEffectConfigDialog` strings). Sampling is bilinear in premultiplied alpha.

use pnn_core::{BlendMode, ColorBgra, Surface};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotateZoom {
    /// Degrees, clockwise.
    pub angle: f64,
    /// Pan as a fraction of the image half-size, -1..=1 on each axis.
    pub pan: [f64; 2],
    /// Magnification (1.0 = unchanged).
    pub zoom: f64,
    /// Repeat the image to fill the canvas.
    pub tiling: bool,
    /// Keep the original pixels underneath the transformed ones.
    pub preserve_background: bool,
}

impl Default for RotateZoom {
    fn default() -> Self {
        Self {
            angle: 0.0,
            pan: [0.0, 0.0],
            zoom: 1.0,
            tiling: false,
            preserve_background: false,
        }
    }
}

impl RotateZoom {
    pub fn is_identity(&self) -> bool {
        self.angle.rem_euclid(360.0) == 0.0 && self.pan == [0.0, 0.0] && self.zoom == 1.0
    }
}

pub fn rotate_zoom(src: &Surface, p: &RotateZoom) -> Surface {
    if p.is_identity() {
        return src.clone();
    }
    let (w, h) = (src.width() as usize, src.height() as usize);
    let px = src.to_pixels();
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let (sin, cos) = (-p.angle.to_radians()).sin_cos();
    let zoom = p.zoom.max(1e-3);
    let pan = [p.pan[0] * cx, p.pan[1] * cy];
    let fetch = |x: i64, y: i64| -> [f32; 4] {
        let (x, y) = if p.tiling {
            (x.rem_euclid(w as i64), y.rem_euclid(h as i64))
        } else if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return [0.0; 4];
        } else {
            (x, y)
        };
        let c = px[y as usize * w + x as usize];
        let a = c.a as f32 / 255.0;
        [c.r as f32 * a, c.g as f32 * a, c.b as f32 * a, a]
    };
    let mut out = vec![ColorBgra::TRANSPARENT; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            // Inverse map: destination → source.
            let dx = x as f64 + 0.5 - cx - pan[0];
            let dy = y as f64 + 0.5 - cy - pan[1];
            let sx = (dx * cos - dy * sin) / zoom + cx - 0.5;
            let sy = (dx * sin + dy * cos) / zoom + cy - 0.5;
            let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
            let (fx, fy) = ((sx - x0 as f64) as f32, (sy - y0 as f64) as f32);
            let (a, b, c, d) = (
                fetch(x0, y0),
                fetch(x0 + 1, y0),
                fetch(x0, y0 + 1),
                fetch(x0 + 1, y0 + 1),
            );
            let mut acc = [0f32; 4];
            for k in 0..4 {
                let top = a[k] + (b[k] - a[k]) * fx;
                let bot = c[k] + (d[k] - c[k]) * fx;
                acc[k] = top + (bot - top) * fy;
            }
            let alpha = acc[3].clamp(0.0, 1.0);
            let un = |v: f32| {
                if alpha > 0.0 {
                    (v / alpha).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            *o = ColorBgra::from_rgba(
                un(acc[0]),
                un(acc[1]),
                un(acc[2]),
                (alpha * 255.0).round() as u8,
            );
        }
    });
    if p.preserve_background {
        for (o, &bg) in out.iter_mut().zip(&px) {
            *o = BlendMode::Normal.apply(bg, *o, 255);
        }
    }
    let mut s = Surface::new_transparent(w as u32, h as u32);
    s.write_pixels(&out);
    s.compact();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot() -> Surface {
        let mut s = Surface::new_transparent(20, 10);
        s.set(15, 5, ColorBgra::from_rgba(255, 0, 0, 255));
        s
    }

    #[test]
    fn identity_is_a_no_op() {
        let s = dot();
        assert_eq!(
            rotate_zoom(&s, &RotateZoom::default()).to_pixels(),
            s.to_pixels()
        );
    }

    #[test]
    fn rotating_180_moves_pixel_through_centre() {
        let r = rotate_zoom(
            &dot(),
            &RotateZoom {
                angle: 180.0,
                ..Default::default()
            },
        );
        assert_eq!(r.get(4, 4).r, 255);
        assert_eq!(r.get(15, 5).a, 0);
    }

    #[test]
    fn zoom_2_doubles_distance_from_centre() {
        let r = rotate_zoom(
            &dot(),
            &RotateZoom {
                zoom: 2.0,
                ..Default::default()
            },
        );
        // (15.5,5.5) is 5.5 px right of centre (10,5); at 2x it lands ~11 px right → x ≈ 20.5,
        // off the canvas.
        assert!(r.to_pixels().iter().all(|p| p.r == 0 || p.a < 255));
    }

    #[test]
    fn preserve_background_keeps_original() {
        let s = dot();
        let r = rotate_zoom(
            &s,
            &RotateZoom {
                angle: 180.0,
                preserve_background: true,
                ..Default::default()
            },
        );
        assert_eq!(r.get(15, 5).r, 255, "original still there");
        assert_eq!(r.get(4, 4).r, 255, "rotated copy on top");
    }
}
