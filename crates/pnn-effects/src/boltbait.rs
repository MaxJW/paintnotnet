//! Object and Selection effects, and two extra adjustments, covering what BoltBait's plugin
//! pack (<https://www.boltbait.com/pdn/>) adds to paint.net, built in as ordinary effects.
//!
//! The pack is not open source. Controls follow the screenshots of its v6.0 dialogs in the
//! pack's forum post (forums.paint.net topic 121343) where they show them; the maths, ranges
//! and anything a screenshot doesn't show are paintnotnet's own. Shapes come from exact
//! signed distance fields (`pnn-sdf`, the feather engine) of the object (alpha ≥ 50%) or the
//! selection, so widths are true pixels; Outline Selection strokes the selection's outline.
//!
//! Angles use the angle dial's convention (0° = right, counter-clockwise) and give the
//! direction the light comes from.

use std::sync::Arc;

use pnn_core::{BlendMode, ColorBgra, Surface};
use pnn_sdf::SignedField;

use crate::Category;
use crate::framework::{ColorDefault, EffectDef, Env, PropDef, Renderer, Value, intensity};

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

/// Light direction (unit vector, y down) for a dial angle.
fn light(angle: f64) -> (f32, f32) {
    let a = angle.to_radians();
    (a.cos() as f32, -a.sin() as f32)
}

/// Outline Object: a band of `width` px around the object, drawn behind it. Hard, 1 px
/// antialiased, or softened over `blur` px at its outer edge.
fn outline_object(
    shape: Shape,
    width: f32,
    color: ColorBgra,
    antialias: bool,
    blur: f32,
) -> Box<dyn Renderer> {
    let shape = Arc::new(shape);
    Box::new(At(move |src: &Surface, x: u32, y: u32| {
        let c = src.get(x, y);
        let sd = shape.sd(x as i64, y as i64);
        if !sd.is_finite() || sd > width + blur * 2.0 + 1.0 {
            return c;
        }
        let k = if blur > 0.0 {
            soft_inside(sd - width, blur * 2.0)
        } else if antialias {
            soft_inside(sd - width, 1.0)
        } else if sd <= width {
            1.0
        } else {
            0.0
        };
        if k <= 0.0 {
            return c;
        }
        // Behind the object, so its own antialiased edge blends into the outline.
        BlendMode::Normal.apply(with_alpha_f(color, k), c, 255)
    }))
}

/// Outline Selection's line style: dash pattern in multiples of the width (GDI+'s patterns).
const LINE_STYLES: &[&str] = &["Solid", "Dash", "Dot", "Dash dot", "Dash dot dot"];
const LOCATIONS: &[&str] = &["Inside selection", "On selection", "Outside selection"];
const CORNERS: &[&str] = &["Miter", "Round", "Bevel"];

/// How Outline Selection strokes the outline (indices into the dropdowns above).
struct OutlineStyle {
    width: f32,
    location: usize,
    corner: usize,
    line: usize,
    antialias: bool,
}

/// Coverage (one byte per pixel, `w`×`h`) of the selection outline stroked as asked.
fn stroke_selection(
    contours: &[Vec<[f64; 2]>],
    w: u32,
    h: u32,
    o: &OutlineStyle,
) -> Option<Vec<u8>> {
    let OutlineStyle {
        width,
        location,
        corner,
        line: style,
        antialias,
    } = *o;
    use tiny_skia::{
        FillRule, LineCap, LineJoin, Mask, PathBuilder, Stroke, StrokeDash, Transform,
    };
    let mut pb = PathBuilder::new();
    let whole = [[
        [0.0, 0.0],
        [w as f64, 0.0],
        [w as f64, h as f64],
        [0.0, h as f64],
    ]
    .to_vec()];
    let contours = if contours.is_empty() {
        &whole[..]
    } else {
        contours
    };
    for c in contours {
        let mut pts = c.iter();
        let Some(p0) = pts.next() else { continue };
        pb.move_to(p0[0] as f32, p0[1] as f32);
        for p in pts {
            pb.line_to(p[0] as f32, p[1] as f32);
        }
        pb.close();
    }
    let outline = pb.finish()?;
    // Inside / outside strokes are centred strokes twice as wide, cut by the selection.
    let stroke_width = if location == 1 { width } else { width * 2.0 };
    let mut stroke = Stroke {
        width: stroke_width,
        line_join: [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel][corner.min(2)],
        line_cap: if corner == 1 {
            LineCap::Round
        } else {
            LineCap::Butt
        },
        ..Stroke::default()
    };
    let pattern: &[f32] = match style {
        1 => &[3.0, 1.0],
        2 => &[1.0, 1.0],
        3 => &[3.0, 1.0, 1.0, 1.0],
        4 => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        _ => &[],
    };
    if !pattern.is_empty() {
        stroke.dash = StrokeDash::new(pattern.iter().map(|d| d * width).collect(), 0.0);
    }
    // tiny-skia's stroker ignores dashes: dash the path first.
    let dashed = match stroke.dash.take() {
        Some(d) => outline.dash(&d, 1.0)?,
        None => outline.clone(),
    };
    let band = dashed.stroke(&stroke, 1.0)?;
    let mut mask = Mask::new(w, h)?;
    mask.fill_path(&band, FillRule::Winding, antialias, Transform::identity());
    let mut cov = mask.data().to_vec();
    if location != 1 {
        let mut inside = Mask::new(w, h)?;
        inside.fill_path(
            &outline,
            FillRule::EvenOdd,
            antialias,
            Transform::identity(),
        );
        for (c, &i) in cov.iter_mut().zip(inside.data()) {
            let keep = if location == 0 { i } else { 255 - i };
            *c = ((*c as u32 * keep as u32 + 127) / 255) as u8;
        }
    }
    Some(cov)
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

#[derive(Clone, Copy)]
struct Bevel {
    depth: f32,
    angle: f64,
    strength: f32,
    hard: bool,
    shadow: bool,
    highlight: ColorBgra,
    dark: ColorBgra,
}

fn bevel(shape: Shape, b: Bevel) -> Box<dyn Renderer> {
    let (lx, ly) = light(b.angle);
    let shape = Arc::new(shape);
    // The optional drop shadow falls away from the light, `depth` px out, `depth` px soft.
    let off = [
        (-lx * b.depth).round() as i64,
        (-ly * b.depth).round() as i64,
    ];
    Box::new(At(move |src: &Surface, x: u32, y: u32| {
        let c = src.get(x, y);
        let (xi, yi) = (x as i64, y as i64);
        let sd = shape.sd(xi, yi);
        let behind = |c: ColorBgra| {
            if !b.shadow {
                return c;
            }
            let q = shape.sd(xi - off[0], yi - off[1]);
            if !q.is_finite() {
                return c;
            }
            let k = soft_inside(q, b.depth.max(1.0)) * 0.6;
            BlendMode::Normal.apply(with_alpha_f(b.dark, k), c, 255)
        };
        if !(sd <= 0.5 && -sd < b.depth) {
            return behind(c);
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
            return behind(c);
        }
        // Rims whose outward normal faces the light are lit; hard edges are flat facets,
        // soft ones fade to flat at the bevel's depth.
        let lit = (gx * lx + gy * ly) / n;
        let (fall, edge) = if b.hard {
            (1.0, if sd <= 0.0 { 1.0 } else { 0.0 })
        } else {
            (1.0 - smooth(-sd / b.depth), soft_inside(sd, 1.0))
        };
        let k = (lit * fall * b.strength * 0.6).clamp(-1.0, 1.0) * edge;
        let target = if k > 0.0 { b.highlight } else { b.dark };
        let out = BlendMode::Normal.apply(c.with_alpha(255), with_alpha_f(target, k.abs()), 255);
        behind(out.with_alpha(c.a))
    }))
}

fn bevel_props() -> Vec<PropDef> {
    vec![
        PropDef::angle("Angle", 135.0, 0.0, 360.0),
        PropDef::int("Depth", 5, 1, 100),
        PropDef::float("Strength", 1.0, 0.0, 3.0, 2),
        PropDef::bool("Hard edges", false),
        PropDef::bool("Add drop shadow", false),
        PropDef::color("Highlight", ColorDefault::Fixed(ColorBgra::WHITE), false)
            .in_section("Colors"),
        PropDef::color("Shadow", ColorDefault::Fixed(ColorBgra::BLACK), false).in_section("Colors"),
    ]
}

fn bevel_args(v: &[Value]) -> Bevel {
    Bevel {
        angle: v[0].float(),
        depth: v[1].int() as f32,
        strength: v[2].float() as f32,
        hard: v[3].bool(),
        shadow: v[4].bool(),
        highlight: v[5].color(),
        dark: v[6].color(),
    }
}

fn inner_shadow_props() -> Vec<PropDef> {
    vec![
        PropDef::int("Radius", 6, 1, 100),
        PropDef::int("Distance", 4, 0, 100),
        PropDef::angle("Angle", 135.0, 0.0, 360.0),
        PropDef::int("Opacity", 60, 0, 100),
        PropDef::color("Color", ColorDefault::Primary, false),
    ]
}

/// (offset of the shadow, radius, opacity, colour) from Inner Shadow's values.
fn shadow_args(v: &[Value]) -> ([i64; 2], f32, f32, ColorBgra) {
    let (lx, ly) = light(v[2].float());
    let dist = v[1].int() as f32;
    (
        // The shadow is cast away from the light.
        [(-lx * dist).round() as i64, (-ly * dist).round() as i64],
        v[0].int() as f32,
        v[3].int() as f32 / 100.0,
        v[4].color(),
    )
}

/// A layer's pixel at (x, y), transparent if the layer or point doesn't exist.
fn layer_px(env: &Env, layer: usize, x: u32, y: u32) -> ColorBgra {
    env.layers
        .get(layer)
        .filter(|(_, s)| x < s.width() && y < s.height())
        .map_or(ColorBgra::TRANSPARENT, |(_, s)| s.get(x, y))
}

pub fn def(name: &str, category: Category) -> Option<EffectDef> {
    let d = |name: &'static str, props: Vec<PropDef>, build| EffectDef {
        name,
        props,
        link: None,
        unclipped: false,
        build,
    };
    let selection = category == Category::Selection;
    Some(match (name, selection) {
        ("Outline Object", _) => d(
            "Outline Object",
            vec![
                PropDef::int("Width", 10, 1, 100),
                PropDef::color("Color", ColorDefault::Primary, false),
                PropDef::bool("Antialiasing", true),
                PropDef::int("Blur radius", 0, 0, 50),
            ],
            |v: &[Value], src, _| {
                outline_object(
                    Shape::object(src, 128),
                    v[0].int() as f32,
                    v[1].color(),
                    v[2].bool(),
                    v[3].int() as f32,
                )
            },
        ),
        ("Outline Selection", _) => EffectDef {
            unclipped: true,
            ..d(
                "Outline Selection",
                vec![
                    PropDef::int("Width", 5, 1, 100),
                    PropDef::color("Color", ColorDefault::Primary, true),
                    PropDef::choice("Location", 1, LOCATIONS),
                    PropDef::choice("Corners", 0, CORNERS),
                    PropDef::choice("Line style", 0, LINE_STYLES),
                    PropDef::bool("Antialiasing", true),
                ],
                |v: &[Value], src, env: &Env| {
                    let (w, h) = (src.width(), src.height());
                    let cov = stroke_selection(
                        &env.selection_contours,
                        w,
                        h,
                        &OutlineStyle {
                            width: v[0].int() as f32,
                            location: v[2].choice(),
                            corner: v[3].choice(),
                            line: v[4].choice(),
                            antialias: v[5].bool(),
                        },
                    )
                    .unwrap_or_default();
                    let color = v[1].color();
                    Box::new(At(move |src: &Surface, x: u32, y: u32| {
                        let c = src.get(x, y);
                        let k = cov.get((y * w + x) as usize).copied().unwrap_or(0);
                        if k == 0 {
                            return c;
                        }
                        BlendMode::Normal.apply(c, with_alpha_f(color, k as f32 / 255.0), 255)
                    }))
                },
            )
        },
        ("Inner Shadow", sel) => {
            let build: crate::framework::Build = if sel {
                |v, src, env| {
                    let (o, r, op, c) = shadow_args(v);
                    inner_shadow(Shape::selection(src, env), r, o, op, c)
                }
            } else {
                |v, src, _| {
                    let (o, r, op, c) = shadow_args(v);
                    inner_shadow(Shape::object(src, 128), r, o, op, c)
                }
            };
            d("Inner Shadow", inner_shadow_props(), build)
        }
        ("Bevel Object", _) | ("Bevel Selection", _) => {
            if selection {
                d("Bevel Selection", bevel_props(), |v, src, env| {
                    bevel(Shape::selection(src, env), bevel_args(v))
                })
            } else {
                d("Bevel Object", bevel_props(), |v, src, _| {
                    bevel(Shape::object(src, 128), bevel_args(v))
                })
            }
        }
        ("Switch Alpha to Gray", _) => d(
            "Switch Alpha to Gray",
            vec![
                PropDef::layer("Mask of layer"),
                PropDef::bool("Invert mask", false),
            ],
            |v: &[Value], _, env: &Env| {
                let (layer, inv, env) = (v[0].choice(), v[1].bool(), env.clone());
                Box::new(At(move |_: &Surface, x: u32, y: u32| {
                    let a = layer_px(&env, layer, x, y).a;
                    let g = if inv { 255 - a } else { a };
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
        ("Apply Alpha Mask", _) => d(
            "Apply Alpha Mask",
            vec![
                PropDef::layer("Mask layer"),
                PropDef::choice("Mask type", 0, &["Shades of gray", "Transparency"]),
                PropDef::choice("Method", 0, &["Replace", "Multiply"]),
                PropDef::bool("Invert mask", false),
            ],
            |v: &[Value], _, env: &Env| {
                let (layer, alpha_type, multiply, inv) = (
                    v[0].choice(),
                    v[1].choice() == 1,
                    v[2].choice() == 1,
                    v[3].bool(),
                );
                let env = env.clone();
                Box::new(At(move |src: &Surface, x: u32, y: u32| {
                    let c = src.get(x, y);
                    let m = layer_px(&env, layer, x, y);
                    // Gray masks: white keeps, black clears (transparent mask pixels clear).
                    let mut k = if alpha_type {
                        m.a
                    } else {
                        ((intensity(m) as u32 * m.a as u32 + 127) / 255) as u8
                    };
                    if inv {
                        k = 255 - k;
                    }
                    let a = if multiply {
                        ((c.a as u32 * k as u32 + 127) / 255) as u8
                    } else {
                        k
                    };
                    c.with_alpha(a)
                }))
            },
        ),
        ("Transparency", _) => d(
            "Transparency",
            vec![
                PropDef::int("Transparent ↔ Opaque", 0, -100, 100),
                PropDef::bool("Ignore transparent pixels", false),
                PropDef::bool("Replace transparent pixels with the secondary color", false),
            ],
            |v: &[Value], _, env: &Env| {
                let (amount, ignore, replace) = (v[0].int(), v[1].bool(), v[2].bool());
                let secondary = env.secondary;
                Box::new(crate::framework::PixelOp(move |c: ColorBgra| {
                    if c.a == 0 && ignore {
                        return c;
                    }
                    let base = if c.a == 0 && replace {
                        secondary.with_alpha(0)
                    } else {
                        c
                    };
                    let a = base.a as i32;
                    // Negative: towards transparent; positive: towards opaque.
                    let out = if amount < 0 {
                        a * (100 + amount) / 100
                    } else {
                        a + (255 - a) * amount / 100
                    };
                    base.with_alpha(out.clamp(0, 255) as u8)
                }))
            },
        ),
        ("Color Balance", _) => d(
            "Color Balance",
            vec![
                PropDef::int("Cyan ↔ Red", 0, -100, 100),
                PropDef::int("Magenta ↔ Green", 0, -100, 100),
                PropDef::int("Yellow ↔ Blue", 0, -100, 100),
                PropDef::int("Brightness", 0, -100, 100),
                PropDef::int("Contrast", 0, -100, 100),
                PropDef::bool("Preserve luminosity", true),
            ],
            |v: &[Value], _, _| {
                let (r, g, b, keep) = (v[0].int(), v[1].int(), v[2].int(), v[5].bool());
                let bc = (v[3].int() != 0 || v[4].int() != 0)
                    .then(|| crate::adjustments::BrightnessContrast::new(v[3].int(), v[4].int()));
                Box::new(crate::framework::PixelOp(move |c: ColorBgra| {
                    // Each slider shifts its channel by up to ±64 levels; preserving
                    // luminosity moves all three back to the original intensity.
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
                    let out = ColorBgra::from_rgba(
                        ch[0].clamp(0, 255) as u8,
                        ch[1].clamp(0, 255) as u8,
                        ch[2].clamp(0, 255) as u8,
                        c.a,
                    );
                    bc.as_ref().map_or(out, |bc| bc.apply(out))
                }))
            },
        ),
        _ => return None,
    })
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

    /// A selection's coverage and outline.
    type Sel = (Arc<pnn_core::Mask>, Vec<Vec<[f64; 2]>>);

    fn env_for(src: &Surface, sel: Option<Sel>) -> Env {
        let all = Rect {
            x: 0,
            y: 0,
            width: src.width(),
            height: src.height(),
        };
        let (selection, contours) = match sel {
            Some((m, c)) => (Some(m), c),
            None => (None, Vec::new()),
        };
        Env {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            bounds: all,
            selection,
            layers: vec![("Layer".to_owned(), src.clone())],
            active_layer: 0,
            selection_contours: Arc::new(contours),
        }
    }

    fn run_env(
        name: &str,
        cat: Category,
        v: Vec<Value>,
        src: &Surface,
        env: &Env,
    ) -> Vec<ColorBgra> {
        let d = def(name, cat).unwrap();
        render_rect(d.renderer(&v, src, env).as_ref(), src, env.bounds)
    }

    fn run(name: &str, cat: Category, v: Vec<Value>, src: &Surface) -> Vec<ColorBgra> {
        run_env(name, cat, v, src, &env_for(src, None))
    }

    fn defaults(name: &str, cat: Category, src: &Surface) -> Vec<Value> {
        def(name, cat).unwrap().defaults_in(&env_for(src, None))
    }

    #[test]
    fn outline_object_draws_a_band_outside() {
        let src = square();
        let v = |aa, blur| {
            vec![
                Value::Int(4),
                Value::Color(ColorBgra::BLACK),
                Value::Bool(aa),
                Value::Int(blur),
            ]
        };
        let out = run("Outline Object", Category::Object, v(true, 0), &src);
        assert_eq!(
            out[30 * 60 + 17],
            ColorBgra::BLACK,
            "3 px outside the left edge"
        );
        assert_eq!(out[30 * 60 + 14].a, 0, "beyond the width");
        assert_eq!(out[30 * 60 + 30], src.get(30, 30), "object untouched");
        // Hard edge: fully on at 4 px, off at 5; blur spreads it further out.
        let hard = run("Outline Object", Category::Object, v(false, 0), &src);
        assert_eq!((hard[30 * 60 + 16].a, hard[30 * 60 + 15].a), (255, 0));
        let soft = run("Outline Object", Category::Object, v(true, 3), &src);
        assert!(soft[30 * 60 + 14].a > 0 && soft[30 * 60 + 14].a < 255);
        // Defaults: 10 px in the primary colour.
        let d = run(
            "Outline Object",
            Category::Object,
            defaults("Outline Object", Category::Object, &src),
            &src,
        );
        assert_eq!(d[30 * 60 + 11], ColorBgra::BLACK);
    }

    fn square_selection() -> Sel {
        let mut m = pnn_core::Mask::new(40, 40, 0);
        for y in 10..30 {
            for x in 10..30 {
                m.set(x, y, 255);
            }
        }
        let c = vec![vec![[10.0, 10.0], [30.0, 10.0], [30.0, 30.0], [10.0, 30.0]]];
        (Arc::new(m), c)
    }

    #[test]
    fn outline_selection_strokes_the_outline() {
        let src = Surface::new_transparent(40, 40);
        let env = env_for(&src, Some(square_selection()));
        let v = |location, style| {
            vec![
                Value::Int(3),
                Value::Color(ColorBgra::BLACK),
                Value::Choice(location),
                Value::Choice(0),
                Value::Choice(style),
                Value::Bool(true),
            ]
        };
        assert!(
            def("Outline Selection", Category::Selection)
                .unwrap()
                .unclipped
        );
        // On the selection: 1.5 px either side of x = 10.
        let on = run_env(
            "Outline Selection",
            Category::Selection,
            v(1, 0),
            &src,
            &env,
        );
        assert_eq!((on[20 * 40 + 9].a, on[20 * 40 + 10].a), (255, 255));
        assert_eq!((on[20 * 40 + 7].a, on[20 * 40 + 20].a), (0, 0));
        // Inside: x 10..13; outside: x 7..10.
        let inside = run_env(
            "Outline Selection",
            Category::Selection,
            v(0, 0),
            &src,
            &env,
        );
        assert_eq!((inside[20 * 40 + 12].a, inside[20 * 40 + 9].a), (255, 0));
        let outside = run_env(
            "Outline Selection",
            Category::Selection,
            v(2, 0),
            &src,
            &env,
        );
        assert_eq!((outside[20 * 40 + 8].a, outside[20 * 40 + 10].a), (255, 0));
        // Dots leave gaps along an edge.
        let dots = run_env(
            "Outline Selection",
            Category::Selection,
            v(1, 2),
            &src,
            &env,
        );
        let row: Vec<u8> = (11..29).map(|x| dots[10 * 40 + x].a).collect();
        assert!(row.contains(&0) && row.contains(&255), "{row:?}");
    }

    #[test]
    fn inner_shadow_and_bevel_stay_inside() {
        let src = square();
        let sh = run(
            "Inner Shadow",
            Category::Object,
            defaults("Inner Shadow", Category::Object, &src),
            &src,
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
            defaults("Bevel Object", Category::Object, &src),
            &src,
        );
        assert!(bv[21 * 60 + 30].g > src.get(30, 21).g, "lit top rim");
        assert!(bv[38 * 60 + 30].r < src.get(30, 38).r, "shaded bottom rim");
        assert_eq!(bv[30 * 60 + 30], src.get(30, 30), "flat middle");
        assert_eq!(bv[30 * 60 + 5].a, 0);
        // Add drop shadow: something appears down-right of the object.
        let mut v = defaults("Bevel Object", Category::Object, &src);
        v[4] = Value::Bool(true);
        let ds = run("Bevel Object", Category::Object, v, &src);
        assert!(ds[42 * 60 + 42].a > 0 && ds[17 * 60 + 17].a == 0);
    }

    #[test]
    fn selection_variants_follow_the_selection() {
        let src = Surface::new_transparent(40, 40);
        let mut src2 = src.clone();
        for y in 0..40 {
            for x in 0..40 {
                src2.set(x, y, ColorBgra::from_rgba(200, 200, 200, 255));
            }
        }
        let env = env_for(&src2, Some(square_selection()));
        let bv = run_env(
            "Bevel Selection",
            Category::Selection,
            defaults("Bevel Selection", Category::Selection, &src2),
            &src2,
            &env,
        );
        assert!(bv[11 * 40 + 20].g > 200, "lit top rim inside the selection");
        assert_eq!(bv[20 * 40 + 20], src2.get(20, 20));
    }

    #[test]
    fn masks_and_adjustments() {
        let src = square();
        let g = run(
            "Switch Alpha to Gray",
            Category::Object,
            vec![Value::Choice(0), Value::Bool(false)],
            &src,
        );
        assert_eq!((g[30 * 60 + 30].r, g[5].r, g[5].a), (255, 0, 255));
        // Apply Alpha Mask from a gray layer: mid gray halves the alpha.
        let mut mask = Surface::new_transparent(60, 60);
        for y in 0..60 {
            for x in 0..60 {
                mask.set(x, y, ColorBgra::from_rgba(128, 128, 128, 255));
            }
        }
        let mut env = env_for(&src, None);
        env.layers.push(("Mask".to_owned(), mask));
        let am = run_env(
            "Apply Alpha Mask",
            Category::Object,
            vec![
                Value::Choice(1),
                Value::Choice(0),
                Value::Choice(0),
                Value::Bool(false),
            ],
            &src,
            &env,
        );
        assert_eq!(am[30 * 60 + 30].a, 128);
        assert_eq!(am[30 * 60 + 30].r, 220);
        let t = run(
            "Transparency",
            Category::Adjustment,
            vec![Value::Int(-50), Value::Bool(false), Value::Bool(false)],
            &src,
        );
        assert_eq!(t[30 * 60 + 30].a, 127);
        let up = |ignore| {
            run(
                "Transparency",
                Category::Adjustment,
                vec![Value::Int(50), Value::Bool(ignore), Value::Bool(true)],
                &src,
            )
        };
        assert_eq!(
            up(false)[5],
            ColorBgra::WHITE.with_alpha(127),
            "secondary shows through"
        );
        assert_eq!(up(true)[5].a, 0, "transparent pixels left alone");
        let cb = run(
            "Color Balance",
            Category::Adjustment,
            vec![
                Value::Int(0),
                Value::Int(0),
                Value::Int(100),
                Value::Int(0),
                Value::Int(0),
                Value::Bool(false),
            ],
            &src,
        );
        assert_eq!(cb[30 * 60 + 30].b, 30 + 64);
    }
}
