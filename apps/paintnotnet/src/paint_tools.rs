//! Paintbrush and Eraser: pointer input → `pnn_tools::brush` stroke mask → layer.
//!
//! As paint.net, the left button paints the primary colour and the right the secondary
//! (`PaintBrushTool.cs`); the Eraser erases with either. A stroke is one history step named
//! after the tool.

use std::sync::Arc;

use pnn_core::{ColorBgra, Mask, Surface};
use pnn_tools::brush::{self, BrushShape, StrokeMask, StrokeOp};
use pnn_tools::{FillStyle, ToolBlend, ToolKind};

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::icons::Icon;

pub struct BrushStroke {
    key: u64,
    tool: ToolKind,
    layer: usize,
    before: Surface,
    stroke: StrokeMask,
    selection: Option<Arc<Mask>>,
    op: StrokeOp,
    fore: ColorBgra,
    back: ColorBgra,
    fill: FillStyle,
}

impl BrushStroke {
    fn apply(&mut self, tab: &mut crate::workspace::DocumentTab, dirty: Option<pnn_core::Rect>) {
        let Some(r) = dirty else {
            return;
        };
        let Self {
            before,
            stroke,
            selection,
            op,
            fore,
            back,
            fill,
            ..
        } = self;
        let (fore, back, fill) = (*fore, *back, *fill);
        tab.doc.modify_layer(self.layer, Some(r), |l| {
            brush::composite(
                &mut l.surface,
                before,
                &stroke.mask,
                selection.as_deref(),
                r,
                *op,
                |x, y| match fill {
                    FillStyle::Solid => fore,
                    FillStyle::Hatch(i) => {
                        if brush::hatch_on(i, x, y) {
                            fore
                        } else {
                            back
                        }
                    }
                },
            )
        });
    }
}

impl PaintNotNetApp {
    pub(crate) fn brush_tool_event(&mut self, event: ToolEvent) {
        match event {
            ToolEvent::Down { pos, button, .. } => {
                self.finish_brush_stroke();
                let antialias = self.antialiased_selection();
                let s = &self.settings;
                let (fore, back) = match button {
                    ToolButton::Primary => (s.primary, s.secondary),
                    ToolButton::Secondary => (s.secondary, s.primary),
                };
                let op = match (self.tool, s.blend) {
                    (ToolKind::Eraser, _) => StrokeOp::Erase,
                    (_, ToolBlend::Overwrite) => StrokeOp::Overwrite,
                    (_, ToolBlend::Blend(m)) => StrokeOp::Blend(m),
                };
                let (shape, smoothing, fill, tool) =
                    (BrushShape::from_settings(s), s.smoothing, s.fill, self.tool);
                let Some(tab) = self.workspace.active_mut() else {
                    return;
                };
                let layer = tab.doc.active_index();
                let mut b = BrushStroke {
                    key: tab.key,
                    tool,
                    layer,
                    before: tab.doc.layer(layer).surface.clone(),
                    stroke: StrokeMask::new(tab.doc.width(), tab.doc.height(), shape, smoothing),
                    selection: tab.selection_mask(antialias),
                    op,
                    fore,
                    back,
                    fill,
                };
                let dirty = b.stroke.add_point([pos[0] as f32, pos[1] as f32]);
                b.apply(tab, dirty);
                self.brush = Some(b);
            }
            ToolEvent::Move { pos } => {
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
                if let Some(b) = self.brush.as_mut()
                    && let Some(tab) = self.workspace.tab_mut(b.key)
                {
                    let dirty = brush::union(
                        b.stroke.add_point([pos[0] as f32, pos[1] as f32]),
                        b.stroke.finish(),
                    );
                    b.apply(tab, dirty);
                }
                self.finish_brush_stroke();
            }
        }
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
            _ => ("Paintbrush", Icon::Paintbrush),
        };
        tab.commit(name, icon);
    }
}
