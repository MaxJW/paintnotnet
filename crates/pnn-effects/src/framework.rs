//! The effect framework: typed properties (from which dialogs are generated, as paint.net's
//! `PropertyBasedEffect`), and renderers that fill rows of the output from the source layer.
//!
//! Renderers are prepared once per set of values (so whole-image work such as Auto-Level's
//! histogram happens once) and then render rows in parallel, tile by tile. Selection
//! clipping is the caller's: the output is mixed with the source by selection coverage.

use pnn_core::{ColorBgra, Rect, Surface};

/// A property's type and range.
#[derive(Clone, Debug, PartialEq)]
pub enum PropKind {
    Int {
        min: i32,
        max: i32,
    },
    Float {
        min: f64,
        max: f64,
        decimals: usize,
    },
    Bool,
    Choice(&'static [&'static str]),
    /// A colour well (`alpha` adds an opacity control), starting from `default`.
    Color {
        alpha: bool,
        default: ColorDefault,
    },
    /// An angle in degrees on a dial: 0° points right, angles grow counter-clockwise
    /// (paint.net's angle chooser).
    Angle {
        min: f64,
        max: f64,
    },
    /// One of the document's layers (index from the bottom); starts on the active layer.
    Layer,
}

/// Where a colour property starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorDefault {
    Primary,
    Secondary,
    Fixed(ColorBgra),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f64),
    Bool(bool),
    Choice(usize),
    Color(ColorBgra),
}

impl Value {
    pub fn int(self) -> i32 {
        match self {
            Self::Int(v) => v,
            Self::Float(v) => v.round() as i32,
            Self::Bool(b) => b as i32,
            Self::Choice(c) => c as i32,
            Self::Color(_) => 0,
        }
    }
    pub fn float(self) -> f64 {
        match self {
            Self::Float(v) => v,
            other => other.int() as f64,
        }
    }
    pub fn bool(self) -> bool {
        self.int() != 0
    }
    pub fn choice(self) -> usize {
        self.int().max(0) as usize
    }
    pub fn color(self) -> ColorBgra {
        match self {
            Self::Color(c) => c,
            _ => ColorBgra::BLACK,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropDef {
    pub name: &'static str,
    pub kind: PropKind,
    pub default: Value,
    /// Collapsible section the control sits in (closed until opened); `None` = always shown,
    /// above the sections.
    pub section: Option<&'static str>,
}

impl PropDef {
    pub const fn int(name: &'static str, default: i32, min: i32, max: i32) -> Self {
        Self {
            name,
            kind: PropKind::Int { min, max },
            default: Value::Int(default),
            section: None,
        }
    }
    pub const fn float(
        name: &'static str,
        default: f64,
        min: f64,
        max: f64,
        decimals: usize,
    ) -> Self {
        Self {
            name,
            kind: PropKind::Float { min, max, decimals },
            default: Value::Float(default),
            section: None,
        }
    }
    pub const fn bool(name: &'static str, default: bool) -> Self {
        Self {
            name,
            kind: PropKind::Bool,
            default: Value::Bool(default),
            section: None,
        }
    }
    pub const fn choice(
        name: &'static str,
        default: usize,
        options: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            kind: PropKind::Choice(options),
            default: Value::Choice(default),
            section: None,
        }
    }
    pub const fn color(name: &'static str, default: ColorDefault, alpha: bool) -> Self {
        Self {
            name,
            kind: PropKind::Color { alpha, default },
            default: Value::Color(match default {
                ColorDefault::Fixed(c) => c,
                ColorDefault::Primary => ColorBgra::BLACK,
                ColorDefault::Secondary => ColorBgra::WHITE,
            }),
            section: None,
        }
    }
    pub const fn angle(name: &'static str, default: f64, min: f64, max: f64) -> Self {
        Self {
            name,
            kind: PropKind::Angle { min, max },
            default: Value::Float(default),
            section: None,
        }
    }
    pub const fn layer(name: &'static str) -> Self {
        Self {
            name,
            kind: PropKind::Layer,
            default: Value::Choice(0),
            section: None,
        }
    }
    /// Put the control in a collapsible section.
    pub const fn in_section(mut self, section: &'static str) -> Self {
        self.section = Some(section);
        self
    }
    /// The starting value in `env`: colours from the palette, layers at the active one.
    pub fn default_in(&self, env: &Env) -> Value {
        match self.kind {
            PropKind::Color {
                default: ColorDefault::Primary,
                ..
            } => Value::Color(env.primary),
            PropKind::Color {
                default: ColorDefault::Secondary,
                ..
            } => Value::Color(env.secondary),
            PropKind::Layer => Value::Choice(env.active_layer),
            _ => self.default,
        }
    }
}

/// paint.net's `LinkValuesBasedOnBooleanRule`: while `toggle` is on, editing any of `values`
/// sets them all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    pub toggle: usize,
    pub values: &'static [usize],
}

/// What a renderer may need besides the source.
#[derive(Clone, Debug)]
pub struct Env {
    pub primary: ColorBgra,
    pub secondary: ColorBgra,
    /// The area being rendered (selection bounds, or the whole layer).
    pub bounds: Rect,
    /// The selection's coverage, if anything is selected (Selection effects outline it).
    pub selection: Option<std::sync::Arc<pnn_core::Mask>>,
    /// The document's layers, bottom first, for effects that read another layer (masks).
    pub layers: Vec<(String, Surface)>,
    pub active_layer: usize,
    /// The selection's outline (even-odd polygons, image pixels); empty = no selection.
    pub selection_contours: std::sync::Arc<Vec<Vec<[f64; 2]>>>,
}

/// Fills rows of the effect's output.
pub trait Renderer: Send + Sync {
    /// Write output pixels for row `y`, columns `x0 .. x0 + out.len()`.
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]);

    /// Write `rect` (row-major, `rect.width` wide). Effects that share work across rows (e.g.
    /// separable blurs) override this; the default renders row by row.
    fn render_rect(&self, src: &Surface, rect: Rect, out: &mut [ColorBgra]) {
        let w = rect.width as usize;
        for (j, row) in out.chunks_mut(w.max(1)).enumerate() {
            self.render_row(src, rect.x, rect.y + j as u32, row);
        }
    }

    /// The same effect as a GPU job, if it has an exact GPU port (`pnn-render`'s
    /// `effects.wgsl`) for these settings and `src`.
    fn gpu(&self, src: &Surface) -> Option<GpuJob> {
        let _ = src;
        None
    }
}

/// An effect the GPU renders exactly as the CPU would: the parameters its shader needs,
/// in the CPU renderer's own fixed-point units.
#[derive(Clone, Debug, PartialEq)]
pub enum GpuJob {
    /// Motion Blur: sample offsets, averaged by `blurs::blend`.
    Motion { points: Vec<[f32; 2]> },
    /// Zoom Blur: amount and centre (16.16 fixed point).
    Zoom { amount: i32, fcx: i32, fcy: i32 },
    /// Radial Blur: steps each way, step angle and centre (16.16 fixed point).
    Radial {
        n: i32,
        fsr: i32,
        fcx: i32,
        fcy: i32,
    },
}

/// A per-pixel operation (most adjustments).
pub struct PixelOp<F: Fn(ColorBgra) -> ColorBgra + Send + Sync>(pub F);

impl<F: Fn(ColorBgra) -> ColorBgra + Send + Sync> Renderer for PixelOp<F> {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = (self.0)(src.get(x0 + i as u32, y));
        }
    }
}

/// A per-pixel operation that also needs the pixel's position (e.g. seeded noise).
pub struct PixelOpXY<F: Fn(ColorBgra, u32, u32) -> ColorBgra + Send + Sync>(pub F);

impl<F: Fn(ColorBgra, u32, u32) -> ColorBgra + Send + Sync> Renderer for PixelOpXY<F> {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            let x = x0 + i as u32;
            *o = (self.0)(src.get(x, y), x, y);
        }
    }
}

/// A per-channel lookup table on B, G, R (alpha kept): 3.36 `UnaryPixelOps.ChannelCurve`.
pub struct ChannelCurve {
    pub b: [u8; 256],
    pub g: [u8; 256],
    pub r: [u8; 256],
}

impl Renderer for ChannelCurve {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        for (i, o) in out.iter_mut().enumerate() {
            let c = src.get(x0 + i as u32, y);
            *o = ColorBgra::from_bgra(
                self.b[c.b as usize],
                self.g[c.g as usize],
                self.r[c.r as usize],
                c.a,
            );
        }
    }
}

pub type Build = fn(&[Value], &Surface, &Env) -> Box<dyn Renderer>;

/// An effect or adjustment: its properties and how to build a renderer for given values.
pub struct EffectDef {
    pub name: &'static str,
    /// No properties = applied directly, no dialog.
    pub props: Vec<PropDef>,
    pub link: Option<Link>,
    /// Renders over the whole layer instead of only inside the selection (effects that draw
    /// along or outside the selection's edge).
    pub unclipped: bool,
    pub build: Build,
}

impl EffectDef {
    pub fn defaults(&self) -> Vec<Value> {
        self.props.iter().map(|p| p.default).collect()
    }
    /// Build the renderer; missing trailing values (e.g. from before an effect gained
    /// options) take their defaults.
    pub fn renderer(&self, values: &[Value], src: &Surface, env: &Env) -> Box<dyn Renderer> {
        if values.len() >= self.props.len() {
            return (self.build)(values, src, env);
        }
        let mut v = values.to_vec();
        v.extend(self.props[values.len()..].iter().map(|p| p.default_in(env)));
        (self.build)(&v, src, env)
    }

    /// Starting values in `env` (see [`PropDef::default_in`]).
    pub fn defaults_in(&self, env: &Env) -> Vec<Value> {
        self.props.iter().map(|p| p.default_in(env)).collect()
    }
}

/// Render `rect` of the effect (row-major, `rect.width` wide) in parallel 256 px tiles, the
/// way the app renders: effects that share work across rows (separable blurs) do it once per
/// tile, not once per row. Used by tests, benchmarks and synchronous application.
pub fn render_rect(r: &dyn Renderer, src: &Surface, rect: Rect) -> Vec<ColorBgra> {
    use rayon::prelude::*;
    let t = pnn_core::TILE_SIZE;
    let w = rect.width as usize;
    let tiles: Vec<Rect> = (0..rect.height.div_ceil(t))
        .flat_map(|ty| {
            (0..rect.width.div_ceil(t)).map(move |tx| Rect {
                x: rect.x + tx * t,
                y: rect.y + ty * t,
                width: t.min(rect.width - tx * t),
                height: t.min(rect.height - ty * t),
            })
        })
        .collect();
    let rendered: Vec<(Rect, Vec<ColorBgra>)> = tiles
        .into_par_iter()
        .map(|tile| {
            let mut out = vec![ColorBgra::TRANSPARENT; (tile.width * tile.height) as usize];
            r.render_rect(src, tile, &mut out);
            (tile, out)
        })
        .collect();
    let mut out = vec![ColorBgra::TRANSPARENT; w * rect.height as usize];
    for (tile, pixels) in rendered {
        let tw = tile.width as usize;
        for (j, row) in pixels.chunks(tw.max(1)).enumerate() {
            let y = (tile.y - rect.y) as usize + j;
            let x = (tile.x - rect.x) as usize;
            out[y * w + x..y * w + x + tw].copy_from_slice(row);
        }
    }
    out
}

/// 3.36 `ColorBgra.GetIntensityByte`.
pub fn intensity(c: ColorBgra) -> u8 {
    ((7471 * c.b as u32 + 38470 * c.g as u32 + 19595 * c.r as u32) >> 16) as u8
}
