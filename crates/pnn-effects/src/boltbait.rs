//! Built-in equivalents of BoltBait's plugin-pack effects (the pack paint.net users install;
//! <https://www.boltbait.com/pdn/>).
//!
//! The pack is not open source and its settings aren't published, so these are paintnotnet's
//! own implementations of what each effect is described to do, with settings chosen to match
//! the descriptions. Shapes come from exact signed distance fields (`pnn-sdf`, the feather
//! engine): of the object (pixels at or above an alpha threshold) for Object effects, or of the
//! selection for Selection effects, so widths and softness are in true pixels.
//!
//! - Outline Object / Outline Selection: a band of the primary colour at a set width
//!   (outside, centred or inside the edge) with soft edges.
//! - Inner Shadow (object / selection): the primary colour inside the edge, offset by
//!   distance and angle, fading over the radius.
//! - Bevel Object / Selection: a rim lit from an angle (white highlights, black shadows).
//! - Drop Shadow Plus: offset X/Y, softness, opacity, primary or secondary colour, optional
//!   shadow only.
//! - Switch Alpha to Gray / Switch Gray to Alpha; Transparency; Color Balance.

use std::sync::Arc;

use pnn_core::{BlendMode, ColorBgra, Surface};
use pnn_sdf::SignedField;

use crate::Category;
use crate::framework::{EffectDef, Env, PropDef, Renderer, Value, intensity};

/// Signed distance (px; negative inside) of the object or selection.
struct Field {
    f: SignedField,
}

impl Field {
    fn object(src: &Surface, threshold: u8) -> Self {
        let cov: Vec<u8> = src
            .to_pixels()
            .iter()
            .map(|c| if c.a >= threshold { 255 } else { 0 })
            .collect();
        Self {
            f: SignedField::from_coverage(&cov, src.width() as usize, src.height() as usize, false),
        }
    }

    fn selection(src: &Surface, env: &Env) -> Self {
        let (w, h) = (src.width() as usize, src.height() as usize);
        let cov = match &env.selection {
            Some(m) => m.to_pixels(),
            None => vec![255u8; w * h],
        };
        Self {
            // The canvas edge is the edge of an all-over selection.
            f: SignedField::from_coverage(&cov, w, h, true),
        }
    }

    /// Distance at field cell (x, y); +∞ beyond the field.
    fn at(&self, x: i64, y: i64) -> f32 {
        let (w, h) = (self.f.width as i64, self.f.height as i64);
        if x < 0 || y < 0 || x >= w || y >= h {
            return f32::INFINITY;
        }
        self.f.sd[(y * w + x) as usize]
    }
}

/// A field plus its offset: selection fields are built with a 1 px border (the image starts
/// at cell (1, 1)).
struct Shape {
    field: Field,
    pad: i64,
}

impl Shape {
    fn object(src: &Surface, threshold: u8) -> Self {
        Self {
            field: Field::object(src, threshold),
            pad: 0,
        }
    }
    fn selection(src: &Surface, env: &Env) -> Self {
        Self {
            field: Field::selection(src, env),
            pad: 1,
        }
    }
    fn sd(&self, x: i64, y: i64) -> f32 {
        self.field.at(x + self.pad, y + self.pad)
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Coverage of a soft edge at signed distance `sd` (0.5 on the edge, `soft` px wide).
fn soft_inside(sd: f32, soft: f32) -> f32 {
    let s = soft.max(1.0);
    smooth(0.5 - sd / s)
}

struct At<F: Fn(&Surface, u32, u32) -> ColorBgra + Send + Sync>(F);

impl<F: Fn(&Surface, u32, u32) -> ColorBgra + Send + Sync> Renderer for At<F> {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = (self.0)(src, x0 + i as u32, y);
        }
    }
}

fn with_alpha_f(c: ColorBgra, k: f32) -> ColorBgra {
    c.with_alpha((c.a as f32 * k.clamp(0.0, 1.0)).round() as u8)
}

const POSITIONS: &[&str] = &["Outside", "Centered", "Inside"];

fn outline(shape: Shape, width: f32, soft: f32, pos: usize, color: ColorBgra) -> Box<dyn Renderer> {
    // Band [lo, hi] of signed distance covered by the outline.
    let (lo, hi) = match pos {
        0 => (0.0, width),
        1 => (-width / 2.0, width / 2.0),
        _ => (-width, 0.0),
    };
    let shape = Arc::new(shape);
    Box::new(At(move |src: &Surface, x: u32, y: u32| {
        let sd = shape.sd(x as i64, y as i64);
        let c = src.get(x, y);
        if !sd.is_finite() {
            return c;
        }
        // Inside the band, with soft (or 1 px antialiased) ends.
        let k = soft_inside(lo - sd, soft) * soft_inside(sd - hi, soft);
        if k <= 0.0 {
            return c;
        }
        let band = with_alpha_f(color, k);
        if pos == 0 {
            // Outside: behind the object.
            BlendMode::Normal.apply(band, c, 255)
        } else {
            BlendMode::Normal.apply(c, band, 255)
        }
    }))
}

fn inner_shadow(
    shape: Shape,
    radius: f32,
    offset: [i64; 2],
    opacity: f32,
    color: ColorBgra,
) -> Box<dyn Renderer> {
    let shape = Arc::new(shape);
    Box::new(At(move |src: &Surface, x: u32, y: u32| {
        let c = src.get(x, y);
        let here = shape.sd(x as i64, y as i64);
        if here > 0.5 {
            return c;
        }
        // Shadow strength: how close the offset point is to (or beyond) the edge.
        let q = shape.sd(x as i64 - offset[0], y as i64 - offset[1]);
        let s = if q.is_finite() {
            soft_inside(-q, radius.max(1.0) * 2.0)
        } else {
            1.0
        };
        let k = s * opacity * soft_inside(here, 1.0);
        if k <= 0.0 {
            return c;
        }
        let shade = with_alpha_f(color, k);
        let out = BlendMode::Normal.apply(c.with_alpha(255), shade, 255);
        out.with_alpha(c.a)
    }))
}

fn bevel(shape: Shape, depth: f32, angle: f32, strength: f32) -> Box<dyn Renderer> {
    let (lx, ly) = (angle.to_radians().cos(), -angle.to_radians().sin());
    let shape = Arc::new(shape);
    Box::new(At(move |src: &Surface, x: u32, y: u32| {
        let c = src.get(x, y);
        let (xi, yi) = (x as i64, y as i64);
        let sd = shape.sd(xi, yi);
        if !(sd <= 0.5 && -sd < depth) {
            return c;
        }
        // Edge normal (pointing out of the shape) from the distance gradient.
        let g = |dx: i64, dy: i64| {
            let v = shape.sd(xi + dx, yi + dy);
            if v.is_finite() { v } else { sd + 1.0 }
        };
        // A 5×5 Sobel-like stencil: the EDT's 1 px differences wobble on curves.
        let (mut gx, mut gy) = (0.0f32, 0.0f32);
        for k in -2i64..=2 {
            let w = 3.0 - k.abs() as f32;
            gx += w * (g(2, k) - g(-2, k));
            gy += w * (g(k, 2) - g(k, -2));
        }
        let n = (gx * gx + gy * gy).sqrt();
        if n == 0.0 {
            return c;
        }
        // Rims whose outward normal faces the light (which comes from `angle`) are lit.
        let lit = (gx * lx + gy * ly) / n;
        // Strongest at the edge, fading to flat at the bevel's depth.
        let fall = 1.0 - smooth(-sd / depth);
        let k = (lit * fall * strength).clamp(-1.0, 1.0) * soft_inside(sd, 1.0);
        let target = if k > 0.0 {
            ColorBgra::WHITE
        } else {
            ColorBgra::BLACK
        };
        let out = BlendMode::Normal.apply(c.with_alpha(255), with_alpha_f(target, k.abs()), 255);
        out.with_alpha(c.a)
    }))
}

fn drop_shadow_plus(
    shape: Shape,
    offset: [i64; 2],
    soft: f32,
    opacity: f32,
    color: ColorBgra,
    only_shadow: bool,
) -> Box<dyn Renderer> {
    let shape = Arc::new(shape);
    Box::new(At(move |src: &Surface, x: u32, y: u32| {
        let c = src.get(x, y);
        let q = shape.sd(x as i64 - offset[0], y as i64 - offset[1]);
        let k = if q.is_finite() {
            soft_inside(q, soft)
        } else {
            0.0
        } * opacity;
        let shadow = with_alpha_f(color, k);
        if only_shadow {
            shadow
        } else {
            BlendMode::Normal.apply(shadow, c, 255)
        }
    }))
}

pub fn def(name: &str, category: Category) -> Option<EffectDef> {
    let d = |name: &'static str, props: Vec<PropDef>, build| EffectDef {
        name,
        props,
        link: None,
        build,
    };
    let selection = category == Category::Selection;
    Some(match (name, selection) {
        ("Outline Object", _) => d(
            "Outline Object",
            vec![
                PropDef::int("Width", 4, 1, 100),
                PropDef::int("Softness", 1, 1, 50),
                PropDef::choice("Position", 0, POSITIONS),
                PropDef::int("Object threshold", 128, 1, 255),
            ],
            |v: &[Value], src, env: &Env| {
                outline(
                    Shape::object(src, v[3].int() as u8),
                    v[0].int() as f32,
                    v[1].int() as f32,
                    v[2].choice(),
                    env.primary,
                )
            },
        ),
        ("Outline Selection", _) => d(
            "Outline Selection",
            vec![
                PropDef::int("Width", 4, 1, 100),
                PropDef::int("Softness", 1, 1, 50),
            ],
            |v: &[Value], src, env: &Env| {
                // Effects are clipped to the selection, so the outline sits inside its edge.
                outline(
                    Shape::selection(src, env),
                    v[0].int() as f32,
                    v[1].int() as f32,
                    2,
                    env.primary,
                )
            },
        ),
        ("Inner Shadow", sel) => {
            let props = vec![
                PropDef::int("Radius", 6, 1, 100),
                PropDef::int("Distance", 4, 0, 100),
                PropDef::float("Angle", 135.0, -180.0, 180.0, 1),
                PropDef::int("Opacity", 60, 0, 100),
            ];
            let build: crate::framework::Build = if sel {
                |v, src, env| {
                    let (o, r, op) = shadow_args(v);
                    inner_shadow(Shape::selection(src, env), r, o, op, env.primary)
                }
            } else {
                |v, src, env| {
                    let (o, r, op) = shadow_args(v);
                    inner_shadow(Shape::object(src, 128), r, o, op, env.primary)
                }
            };
            d("Inner Shadow", props, build)
        }
        ("Bevel Object", _) | ("Bevel Selection", _) => {
            let props = vec![
                PropDef::int("Depth", 10, 1, 100),
                PropDef::float("Angle", 135.0, -180.0, 180.0, 1),
                PropDef::int("Strength", 60, 0, 100),
            ];
            if selection {
                d("Bevel Selection", props, |v, src, env| {
                    bevel(
                        Shape::selection(src, env),
                        v[0].int() as f32,
                        v[1].float() as f32,
                        v[2].int() as f32 / 100.0,
                    )
                })
            } else {
                d("Bevel Object", props, |v, src, _| {
                    bevel(
                        Shape::object(src, 128),
                        v[0].int() as f32,
                        v[1].float() as f32,
                        v[2].int() as f32 / 100.0,
                    )
                })
            }
        }
        ("Drop Shadow Plus", _) => d(
            "Drop Shadow Plus",
            vec![
                PropDef::int("Offset X", 8, -200, 200),
                PropDef::int("Offset Y", 8, -200, 200),
                PropDef::int("Softness", 10, 1, 200),
                PropDef::int("Opacity", 60, 0, 100),
                PropDef::choice("Color", 0, &["Primary", "Secondary"]),
                PropDef::bool("Only draw shadow", false),
            ],
            |v: &[Value], src, env: &Env| {
                drop_shadow_plus(
                    Shape::object(src, 128),
                    [v[0].int() as i64, v[1].int() as i64],
                    v[2].int() as f32,
                    v[3].int() as f32 / 100.0,
                    if v[4].choice() == 0 {
                        env.primary
                    } else {
                        env.secondary
                    },
                    v[5].bool(),
                )
            },
        ),
        ("Switch Alpha to Gray", _) => d(
            "Switch Alpha to Gray",
            vec![PropDef::bool("Invert", false)],
            |v: &[Value], _, _| {
                let inv = v[0].bool();
                Box::new(crate::framework::PixelOp(move |c: ColorBgra| {
                    let g = if inv { 255 - c.a } else { c.a };
                    ColorBgra::from_bgra(g, g, g, 255)
                }))
            },
        ),
        ("Switch Gray to Alpha", _) => d(
            "Switch Gray to Alpha",
            vec![
                PropDef::bool("Invert", false),
                PropDef::choice("Color", 0, &["Keep", "Primary"]),
            ],
            |v: &[Value], _, env: &Env| {
                let (inv, primary) = (v[0].bool(), (v[1].choice() == 1).then_some(env.primary));
                Box::new(crate::framework::PixelOp(move |c: ColorBgra| {
                    let g = intensity(c);
                    let a = if inv { 255 - g } else { g };
                    primary.unwrap_or(c).with_alpha(a)
                }))
            },
        ),
        ("Transparency", _) => d(
            "Transparency",
            vec![
                PropDef::int("Opacity", 50, 0, 100),
                PropDef::choice("Mode", 0, &["Multiply", "Set"]),
            ],
            |v: &[Value], _, _| {
                let (o, set) = (v[0].int() as u32, v[1].choice() == 1);
                Box::new(crate::framework::PixelOp(move |c: ColorBgra| {
                    let a = if set {
                        (o * 255 + 50) / 100
                    } else {
                        (c.a as u32 * o + 50) / 100
                    };
                    c.with_alpha(a as u8)
                }))
            },
        ),
        ("Color Balance", _) => d(
            "Color Balance",
            vec![
                PropDef::int("Cyan – Red", 0, -100, 100),
                PropDef::int("Magenta – Green", 0, -100, 100),
                PropDef::int("Yellow – Blue", 0, -100, 100),
                PropDef::bool("Preserve luminosity", true),
            ],
            |v: &[Value], _, _| {
                let (r, g, b, keep) = (v[0].int(), v[1].int(), v[2].int(), v[3].bool());
                Box::new(crate::framework::PixelOp(move |c: ColorBgra| {
                    // Each slider shifts its channel by up to ±64 levels (half each way on
                    // the other two when preserving luminosity).
                    let shift = |v: i32| v * 64 / 100;
                    let mut ch = [
                        c.r as i32 + shift(r),
                        c.g as i32 + shift(g),
                        c.b as i32 + shift(b),
                    ];
                    if keep {
                        let before = intensity(c) as i32;
                        let t = ColorBgra::from_rgba(
                            ch[0].clamp(0, 255) as u8,
                            ch[1].clamp(0, 255) as u8,
                            ch[2].clamp(0, 255) as u8,
                            255,
                        );
                        let d = before - intensity(t) as i32;
                        for v in &mut ch {
                            *v += d;
                        }
                    }
                    ColorBgra::from_rgba(
                        ch[0].clamp(0, 255) as u8,
                        ch[1].clamp(0, 255) as u8,
                        ch[2].clamp(0, 255) as u8,
                        c.a,
                    )
                }))
            },
        ),
        _ => return None,
    })
}

fn shadow_args(v: &[Value]) -> ([i64; 2], f32, f32) {
    let (r, dist, ang, op) = (
        v[0].int() as f32,
        v[1].int() as f64,
        v[2].float(),
        v[3].int(),
    );
    // Light from `ang`: the shadow is cast away from it.
    let a = ang.to_radians();
    (
        [
            (-dist * a.cos()).round() as i64,
            (dist * a.sin()).round() as i64,
        ],
        r,
        op as f32 / 100.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::render_rect;
    use pnn_core::Rect;

    /// A red square (20..40)² on transparency, 60×60.
    fn square() -> Surface {
        let mut s = Surface::new_transparent(60, 60);
        for y in 20..40 {
            for x in 20..40 {
                s.set(x, y, ColorBgra::from_rgba(220, 30, 30, 255));
            }
        }
        s
    }

    fn run(
        name: &str,
        cat: Category,
        v: Vec<Value>,
        src: &Surface,
        sel: Option<Arc<pnn_core::Mask>>,
    ) -> Vec<ColorBgra> {
        let d = def(name, cat).unwrap();
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
            selection: sel,
        };
        render_rect((d.build)(&v, src, &env).as_ref(), src, all)
    }

    #[test]
    fn outline_object_draws_a_band_outside() {
        let src = square();
        let out = run(
            "Outline Object",
            Category::Object,
            vec![
                Value::Int(4),
                Value::Int(1),
                Value::Choice(0),
                Value::Int(128),
            ],
            &src,
            None,
        );
        assert_eq!(
            out[30 * 60 + 17],
            ColorBgra::BLACK,
            "3 px outside the left edge"
        );
        assert_eq!(out[30 * 60 + 14].a, 0, "beyond the width");
        assert_eq!(out[30 * 60 + 30], src.get(30, 30), "object untouched");
    }

    #[test]
    fn inner_shadow_and_bevel_stay_inside() {
        let src = square();
        let sh = run(
            "Inner Shadow",
            Category::Object,
            def("Inner Shadow", Category::Object).unwrap().defaults(),
            &src,
            None,
        );
        assert_eq!(sh[30 * 60 + 10], src.get(10, 30), "outside untouched");
        assert_eq!(sh[30 * 60 + 30], src.get(30, 30), "centre untouched");
        // Light from 135° (top-left): the shadow hugs the top-left inner edges.
        assert!(
            sh[21 * 60 + 25].r < src.get(25, 21).r,
            "shadowed near the top edge"
        );
        let bv = run(
            "Bevel Object",
            Category::Object,
            vec![Value::Int(5), Value::Float(135.0), Value::Int(60)],
            &src,
            None,
        );
        assert!(bv[21 * 60 + 30].g > src.get(30, 21).g, "lit top rim");
        assert!(bv[38 * 60 + 30].r < src.get(30, 38).r, "shaded bottom rim");
        assert_eq!(bv[30 * 60 + 30], src.get(30, 30), "flat middle");
        assert_eq!(bv[30 * 60 + 5].a, 0);
    }

    #[test]
    fn selection_variants_follow_the_selection() {
        let src = Surface::new_transparent(40, 40);
        let mut m = pnn_core::Mask::new(40, 40, 0);
        for y in 10..30 {
            for x in 10..30 {
                m.set(x, y, 255);
            }
        }
        let out = run(
            "Outline Selection",
            Category::Selection,
            vec![Value::Int(3), Value::Int(1)],
            &src,
            Some(Arc::new(m)),
        );
        assert_eq!(out[20 * 40 + 11].a, 255, "inside the selection edge");
        assert_eq!(out[20 * 40 + 20].a, 0, "middle clear");
    }

    #[test]
    fn small_adjustments() {
        let src = square();
        let shadow = run(
            "Drop Shadow Plus",
            Category::Object,
            def("Drop Shadow Plus", Category::Object)
                .unwrap()
                .defaults(),
            &src,
            None,
        );
        assert!(shadow[45 * 60 + 45].a > 0, "shadow down-right");
        assert_eq!(shadow[30 * 60 + 30], src.get(30, 30));
        let g = run(
            "Switch Alpha to Gray",
            Category::Object,
            vec![Value::Bool(false)],
            &src,
            None,
        );
        assert_eq!((g[30 * 60 + 30].r, g[5].r, g[5].a), (255, 0, 255));
        let t = run(
            "Transparency",
            Category::Adjustment,
            vec![Value::Int(50), Value::Choice(0)],
            &src,
            None,
        );
        assert_eq!(t[30 * 60 + 30].a, 128);
        let cb = run(
            "Color Balance",
            Category::Adjustment,
            vec![
                Value::Int(0),
                Value::Int(0),
                Value::Int(100),
                Value::Bool(false),
            ],
            &src,
            None,
        );
        assert_eq!(cb[30 * 60 + 30].b, 30 + 64);
    }
}
