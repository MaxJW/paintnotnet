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
    Int { min: i32, max: i32 },
    Float { min: f64, max: f64, decimals: usize },
    Bool,
    Choice(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f64),
    Bool(bool),
    Choice(usize),
}

impl Value {
    pub fn int(self) -> i32 {
        match self {
            Self::Int(v) => v,
            Self::Float(v) => v.round() as i32,
            Self::Bool(b) => b as i32,
            Self::Choice(c) => c as i32,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropDef {
    pub name: &'static str,
    pub kind: PropKind,
    pub default: Value,
}

impl PropDef {
    pub const fn int(name: &'static str, default: i32, min: i32, max: i32) -> Self {
        Self {
            name,
            kind: PropKind::Int { min, max },
            default: Value::Int(default),
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
        }
    }
    pub const fn bool(name: &'static str, default: bool) -> Self {
        Self {
            name,
            kind: PropKind::Bool,
            default: Value::Bool(default),
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
    pub build: Build,
}

impl EffectDef {
    pub fn defaults(&self) -> Vec<Value> {
        self.props.iter().map(|p| p.default).collect()
    }
}

/// Render `rect` of the effect into `dst` rows (row-major, `rect.width` wide). Used for tests
/// and synchronous application; the app renders tile by tile instead.
pub fn render_rect(r: &dyn Renderer, src: &Surface, rect: Rect) -> Vec<ColorBgra> {
    use rayon::prelude::*;
    let w = rect.width as usize;
    let mut out = vec![ColorBgra::TRANSPARENT; w * rect.height as usize];
    out.par_chunks_mut(w.max(1))
        .enumerate()
        .for_each(|(j, row)| r.render_row(src, rect.x, rect.y + j as u32, row));
    out
}

/// 3.36 `ColorBgra.GetIntensityByte`.
pub fn intensity(c: ColorBgra) -> u8 {
    ((7471 * c.b as u32 + 38470 * c.g as u32 + 19595 * c.r as u32) >> 16) as u8
}
