//! Brush-driven tools: Paintbrush, Eraser, Recolor and Clone Stamp share the
//! `pnn_tools::brush` stroke mask; plus the Color Picker.
//!
//! From paint.net 3.36 (MIT, see NOTICE):
//! - `PaintBrushTool.cs`: left paints the primary colour, right the secondary; the Eraser
//!   erases with either.
//! - `RecoloringTool.cs`: left replaces the secondary colour's neighbourhood with the primary,
//!   right the reverse; Ctrl+click picks the colour (keeping its alpha); Shift+drag picks one
//!   colour and moves the other by the same RGB difference (`AdjustDrawingColor`).
//! - `CloneStampTool.cs`: Ctrl+click sets the source (point and layer); the offset is fixed by
//!   the first stroke and kept for later strokes (aligned), until the next Ctrl+click.
//! - `ColorPickerTool.cs`: left sets the primary, right the secondary, while dragging too;
//!   the after-click tool switch happens on release.
//!
//! paint.net 5 additions from its docs: Color Picker sampling (layer/image, square sizes;
//! Ctrl samples the image). Averaging a square is done in premultiplied space (a guess).
//!
//! Each stroke is one history step named after the tool.

use std::sync::Arc;

use pnn_core::{ColorBgra, LayerId, Mask, Surface, TILE_SIZE, TileCoord};
use pnn_tools::brush::{self, BrushShape, StrokeMask, StrokeOp};
use pnn_tools::{ColorPickerAfterClick, FillStyle, SamplingMode, ToolBlend, ToolKind};

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::icons::Icon;
use crate::workspace::DocumentTab;

/// Where a stroke's paint comes from.
enum Paint {
    /// Solid or hatched colour.
    Fill {
        fore: ColorBgra,
        back: ColorBgra,
        fill: FillStyle,
    },
    /// Clone Stamp: pixels from `source`, `offset` px up-left of the destination.
    Clone { source: Surface, offset: [i64; 2] },
}

impl Paint {
    fn at(&self, x: u32, y: u32) -> ColorBgra {
        match self {
            Self::Fill { fore, back, fill } => match fill {
                FillStyle::Hatch(i) if !brush::hatch_on(*i, x, y) => *back,
                _ => *fore,
            },
            Self::Clone { source, offset } => {
                let (sx, sy) = (x as i64 - offset[0], y as i64 - offset[1]);
                if sx < 0 || sy < 0 || sx >= source.width() as i64 || sy >= source.height() as i64 {
                    ColorBgra::TRANSPARENT
                } else {
                    source.get(sx as u32, sy as u32)
                }
            }
        }
    }
}

pub struct BrushStroke {
    key: u64,
    tool: ToolKind,
    layer: usize,
    before: Surface,
    stroke: StrokeMask,
    selection: Option<Arc<Mask>>,
    op: StrokeOp,
    paint: Paint,
}

impl BrushStroke {
    fn apply(&mut self, tab: &mut DocumentTab, dirty: Option<pnn_core::Rect>) {
        let Some(r) = dirty else {
            return;
        };
        let Self {
            before,
            stroke,
            selection,
            op,
            paint,
            ..
        } = self;
        tab.doc.modify_layer(self.layer, Some(r), |l| {
            brush::composite(
                &mut l.surface,
                before,
                &stroke.mask,
                selection.as_deref(),
                r,
                *op,
                |x, y| paint.at(x, y),
            )
        });
    }
}

/// The Clone Stamp's source, set by Ctrl+click.
pub struct CloneSource {
    key: u64,
    layer: LayerId,
    point: [f64; 2],
    /// Destination − source, fixed by the first stroke after the source was set.
    offset: Option<[f64; 2]>,
}

impl PaintNotNetApp {
    pub(crate) fn brush_tool_event(&mut self, event: ToolEvent) {
        match event {
            ToolEvent::Down {
                pos,
                button,
                modifiers,
            } => {
                self.finish_brush_stroke();
                match self.tool {
                    ToolKind::CloneStamp if modifiers.ctrl => {
                        self.set_clone_source(pos);
                        return;
                    }
                    ToolKind::CloneStamp if button != ToolButton::Primary => return,
                    ToolKind::Recolor if modifiers.ctrl || modifiers.shift => {
                        self.recolor_pick(pos, button, modifiers.shift);
                        self.recolor_picking = Some((button, modifiers.shift));
                        return;
                    }
                    _ => {}
                }
                self.begin_stroke(pos, button);
            }
            ToolEvent::Move { pos } => {
                if let Some((button, adjust)) = self.recolor_picking {
                    self.recolor_pick(pos, button, adjust);
                    return;
                }
                let Some(b) = self.brush.as_mut() else {
                    return;
                };
                let Some(tab) = self.workspace.tab_mut(b.key) else {
                    return;
                };
                let dirty = b.stroke.add_point([pos[0] as f32, pos[1] as f32]);
                b.apply(tab, dirty);
            }
            ToolEvent::Up { pos } => {
                self.recolor_picking = None;
                if let Some(b) = self.brush.as_mut()
                    && let Some(tab) = self.workspace.tab_mut(b.key)
                {
                    let dirty = b.stroke.add_point([pos[0] as f32, pos[1] as f32]);
                    b.apply(tab, dirty);
                }
                self.finish_brush_stroke();
            }
        }
    }

    fn begin_stroke(&mut self, pos: [f64; 2], button: ToolButton) {
        let antialias = self.antialiased_selection();
        let s = &self.settings;
        let (fore, back) = match button {
            ToolButton::Primary => (s.primary, s.secondary),
            ToolButton::Secondary => (s.secondary, s.primary),
        };
        let blend_op = match s.blend {
            ToolBlend::Overwrite => StrokeOp::Overwrite,
            ToolBlend::Blend(m) => StrokeOp::Blend(m),
        };
        let mut shape = BrushShape::from_settings(s);
        let op = match self.tool {
            ToolKind::Eraser => StrokeOp::Erase,
            // Left: secondary → primary; right: primary → secondary.
            ToolKind::Recolor => StrokeOp::Recolor {
                from: back,
                to: fore,
                tolerance: brush::recolor_tolerance(s.tolerance, back, fore),
            },
            ToolKind::CloneStamp => StrokeOp::Blend(pnn_core::BlendMode::Normal),
            _ => blend_op,
        };
        if self.tool == ToolKind::Recolor {
            // Recolor's toolbar has no hardness: a hard (antialiased) round brush.
            shape.hardness = 1.0;
        }
        if matches!(self.tool, ToolKind::Recolor | ToolKind::CloneStamp) {
            // No Spacing option on these toolbars: a continuous stroke.
            shape.spacing = 0.15;
        }
        if self.tool == ToolKind::Recolor && fore == back {
            return; // 3.36: nothing to do when the colours match.
        }
        let (smoothing, fill, tool) = (s.smoothing, s.fill, self.tool);
        let Some(tab) = self.workspace.active_mut() else {
            return;
        };
        let layer = tab.doc.active_index();
        let before = tab.doc.layer(layer).surface.clone();
        let paint = if tool == ToolKind::CloneStamp {
            let Some(src) = self.clone_source.as_mut().filter(|c| c.key == tab.key) else {
                return; // No source yet: Ctrl+click sets one.
            };
            let Some(source) = tab.doc.layers().iter().find(|l| l.id() == src.layer) else {
                return;
            };
            let offset = *src
                .offset
                .get_or_insert([pos[0] - src.point[0], pos[1] - src.point[1]]);
            Paint::Clone {
                // The source as it was before this stroke, even when it's the same layer.
                source: source.surface.clone(),
                offset: [offset[0].round() as i64, offset[1].round() as i64],
            }
        } else {
            Paint::Fill { fore, back, fill }
        };
        let mut b = BrushStroke {
            key: tab.key,
            tool,
            layer,
            before,
            stroke: StrokeMask::new(tab.doc.width(), tab.doc.height(), shape, smoothing),
            selection: tab.selection_mask(antialias),
            op,
            paint,
        };
        let dirty = b.stroke.add_point([pos[0] as f32, pos[1] as f32]);
        b.apply(tab, dirty);
        self.brush = Some(b);
    }

    /// Commit the stroke in progress, if any (pointer up, tool change).
    pub(crate) fn finish_brush_stroke(&mut self) {
        let Some(mut b) = self.brush.take() else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(b.key) else {
            return;
        };
        let dirty = b.stroke.finish();
        b.apply(tab, dirty);
        let (name, icon) = match b.tool {
            ToolKind::Eraser => ("Eraser", Icon::Eraser),
            ToolKind::Recolor => ("Recolor", Icon::Recolor),
            ToolKind::CloneStamp => ("Clone Stamp", Icon::CloneStamp),
            _ => ("Paintbrush", Icon::Paintbrush),
        };
        tab.commit(name, icon);
    }

    fn set_clone_source(&mut self, pos: [f64; 2]) {
        let Some(tab) = self.workspace.active() else {
            return;
        };
        let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
        if pos[0] < 0.0 || pos[1] < 0.0 || pos[0] >= w || pos[1] >= h {
            return;
        }
        self.clone_source = Some(CloneSource {
            key: tab.key,
            layer: tab.doc.layer(tab.doc.active_index()).id(),
            point: pos,
            offset: None,
        });
    }

    /// Where the Clone Stamp will copy from for a pointer at `cursor` (image coordinates).
    pub(crate) fn clone_source_marker(&self, cursor: Option<[f64; 2]>) -> Option<[f64; 2]> {
        let src = self.clone_source.as_ref()?;
        if self.workspace.active().map(|t| t.key) != Some(src.key) {
            return None;
        }
        match (src.offset, cursor) {
            (Some(o), Some(c)) => Some([c[0] - o[0], c[1] - o[1]]),
            _ => Some(src.point),
        }
    }

    /// Recolor Ctrl+click (pick) or Shift+drag (pick and adjust the other colour).
    fn recolor_pick(&mut self, pos: [f64; 2], button: ToolButton, adjust: bool) {
        let Some(tab) = self.workspace.active() else {
            return;
        };
        let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
        if pos[0] < 0.0 || pos[1] < 0.0 || pos[0] >= w || pos[1] >= h {
            return;
        }
        let lifted = tab
            .doc
            .layer(tab.doc.active_index())
            .surface
            .get(pos[0] as u32, pos[1] as u32);
        let s = &mut self.settings;
        let (picked, other) = match button {
            ToolButton::Primary => (&mut s.primary, &mut s.secondary),
            ToolButton::Secondary => (&mut s.secondary, &mut s.primary),
        };
        let old = *picked;
        *picked = lifted.with_alpha(old.a);
        if adjust {
            // 3.36 `AdjustColorDifference`: move the other colour by the same RGB step.
            let ch = |o: u8, n: u8, b: u8| (b as i32 + n as i32 - o as i32).clamp(0, 255) as u8;
            *other = ColorBgra::from_bgra(
                ch(old.b, picked.b, other.b),
                ch(old.g, picked.g, other.g),
                ch(old.r, picked.r, other.r),
                other.a,
            );
        }
    }

    pub(crate) fn color_picker_event(&mut self, event: ToolEvent) {
        let (pos, button) = match event {
            ToolEvent::Down {
                pos,
                button,
                modifiers,
            } => {
                self.picker = Some((button, modifiers.ctrl));
                (pos, button)
            }
            ToolEvent::Move { pos } => match self.picker {
                Some((b, _)) => (pos, b),
                None => return,
            },
            ToolEvent::Up { .. } => {
                if self.picker.take().is_some() {
                    match self.settings.picker_after_click {
                        ColorPickerAfterClick::DoNotSwitch => {}
                        ColorPickerAfterClick::SwitchToPrevious => {
                            self.set_tool(self.previous_tool)
                        }
                        ColorPickerAfterClick::SwitchToPencil => self.set_tool(ToolKind::Pencil),
                    }
                }
                return;
            }
        };
        let image = self.settings.sampling == SamplingMode::Image
            || self.picker.is_some_and(|(_, ctrl)| ctrl);
        let size = self.settings.picker_sampling.size();
        let Some(tab) = self.workspace.active() else {
            return;
        };
        let Some(c) = sample(tab, pos, size, image) else {
            return;
        };
        match button {
            ToolButton::Primary => self.settings.primary = c,
            ToolButton::Secondary => self.settings.secondary = c,
        }
    }
}

/// Average colour of the `size`×`size` square around `pos` (premultiplied average), from the
/// active layer or the composite image. `None` outside the image.
pub fn sample(tab: &DocumentTab, pos: [f64; 2], size: u32, image: bool) -> Option<ColorBgra> {
    let (w, h) = (tab.doc.width() as i64, tab.doc.height() as i64);
    let (cx, cy) = (pos[0].floor() as i64, pos[1].floor() as i64);
    if cx < 0 || cy < 0 || cx >= w || cy >= h {
        return None;
    }
    let half = (size / 2) as i64;
    let layer = &tab.doc.layer(tab.doc.active_index()).surface;
    let mut composite: Vec<(TileCoord, Option<Arc<pnn_core::Tile<ColorBgra>>>)> = Vec::new();
    let mut get = |x: u32, y: u32| -> ColorBgra {
        if !image {
            return layer.get(x, y);
        }
        let c = TileCoord {
            tx: x / TILE_SIZE,
            ty: y / TILE_SIZE,
        };
        let tile = match composite.iter().find(|(k, _)| *k == c) {
            Some((_, t)) => t.clone(),
            None => {
                let t = pnn_core::compositor::composite_tile(&tab.doc, c);
                composite.push((c, t.clone()));
                t
            }
        };
        tile.map_or(ColorBgra::TRANSPARENT, |t| {
            t.get(x % TILE_SIZE, y % TILE_SIZE)
        })
    };
    let (mut sum, mut n) = ([0u64; 4], 0u64);
    for y in (cy - half).max(0)..=(cy + half).min(h - 1) {
        for x in (cx - half).max(0)..=(cx + half).min(w - 1) {
            let p = get(x as u32, y as u32);
            let a = p.a as u64;
            sum[0] += p.b as u64 * a;
            sum[1] += p.g as u64 * a;
            sum[2] += p.r as u64 * a;
            sum[3] += a;
            n += 1;
        }
    }
    if size == 1 {
        return Some(get(cx as u32, cy as u32));
    }
    if sum[3] == 0 {
        return Some(ColorBgra::TRANSPARENT);
    }
    let ch = |v: u64| ((v + sum[3] / 2) / sum[3]) as u8;
    Some(ColorBgra::from_bgra(
        ch(sum[0]),
        ch(sum[1]),
        ch(sum[2]),
        ((sum[3] + n / 2) / n) as u8,
    ))
}
