//! Move Selected Pixels and Move Selection.
//!
//! Behaviour from paint.net 5's shortcut table: drag inside to move; drag a corner/edge nub
//! to scale (Shift keeps proportions, Alt scales about the centre); drag with the right
//! button, or the left button outside the selection, to rotate (Shift snaps to 15°); Ctrl
//! when grabbing moves a copy; arrow keys nudge 1 px (Ctrl: 10 px). History steps use paint.net
//! 3.36's names: Move/Scale/Rotate Pixels (or Selection) and Finish Pixels (Selection).
//!
//! Lifting copies the selected pixels (scaled by selection coverage) out of the layer and
//! keeps the document as it was before; every change redraws the lifted pixels, transformed,
//! onto that snapshot. With nothing selected the whole layer moves, as in paint.net.

use pnn_core::transform::{self, Affine, IDENTITY, Sampling};
use pnn_core::{ColorBgra, Document, Rect, Selection, Surface};
use pnn_tools::{Resampling, ToolKind};

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::icons::Icon;
use crate::selection_tools::fill_masked;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Pixels,
    Selection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragMode {
    Move,
    /// Nub index: 0 TL, 1 T, 2 TR, 3 R, 4 BR, 5 B, 6 BL, 7 L.
    Scale(usize),
    Rotate {
        center: [f64; 2],
        start_angle: f64,
    },
}

struct MoveDrag {
    mode: DragMode,
    start: [f64; 2],
    m0: Affine,
}

pub struct MoveSession {
    pub key: u64,
    pub kind: MoveKind,
    layer: usize,
    /// The document with the lifted pixels removed (or untouched, when moving a copy).
    base: Document,
    /// Lifted pixels in image coordinates (only meaningful inside `rect`).
    pixels: Surface,
    rect: Rect,
    selection: Selection,
    /// Original selection bounds (handles are placed on these, transformed).
    bounds: [f64; 4],
    pub m: Affine,
    drag: Option<MoveDrag>,
    /// Something moved since lifting (Finish records a step only then).
    pub changed: bool,
}

impl MoveSession {
    /// Nub positions in image coordinates.
    pub fn handles(&self) -> [[f64; 2]; 8] {
        let [x0, y0, x1, y1] = self.bounds;
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        [
            [x0, y0],
            [mx, y0],
            [x1, y0],
            [x1, my],
            [x1, y1],
            [mx, y1],
            [x0, y1],
            [x0, my],
        ]
        .map(|p| transform::apply(&self.m, p))
    }

    fn center(&self) -> [f64; 2] {
        let [x0, y0, x1, y1] = self.bounds;
        transform::apply(&self.m, [(x0 + x1) / 2.0, (y0 + y1) / 2.0])
    }

    pub fn current_selection(&self) -> Selection {
        self.selection.transform(self.m)
    }
}

fn sampling(r: Resampling) -> Sampling {
    match r {
        Resampling::Bicubic => Sampling::Bicubic,
        Resampling::Bilinear => Sampling::Bilinear,
        Resampling::NearestNeighbor => Sampling::NearestNeighbor,
    }
}

fn names(kind: MoveKind, mode: DragMode) -> (&'static str, Icon) {
    let icon = if kind == MoveKind::Pixels {
        Icon::MoveSelectedPixels
    } else {
        Icon::MoveSelection
    };
    let name = match (kind, mode) {
        (MoveKind::Pixels, DragMode::Move) => "Move Pixels",
        (MoveKind::Pixels, DragMode::Scale(_)) => "Scale Pixels",
        (MoveKind::Pixels, DragMode::Rotate { .. }) => "Rotate Pixels",
        (MoveKind::Selection, DragMode::Move) => "Move Selection",
        (MoveKind::Selection, DragMode::Scale(_)) => "Scale Selection",
        (MoveKind::Selection, DragMode::Rotate { .. }) => "Rotate Selection",
    };
    (name, icon)
}

impl PaintNotNetApp {
    fn move_kind(&self) -> Option<MoveKind> {
        match self.tool {
            ToolKind::MoveSelectedPixels => Some(MoveKind::Pixels),
            ToolKind::MoveSelection => Some(MoveKind::Selection),
            _ => None,
        }
    }

    /// Start a session on the active document: lift the selected pixels (unless only the
    /// selection moves). `copy` leaves the original pixels in place (Ctrl).
    pub(crate) fn lift(&mut self, kind: MoveKind, copy: bool) -> bool {
        let antialias = self.antialiased_selection();
        let Some(tab) = self.workspace.active_mut() else {
            return false;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        let mut selection = tab.doc.selection().clone();
        if selection.is_empty() {
            selection = Selection::all(w, h);
        }
        let Some(bounds) = selection.bounds() else {
            return false;
        };
        let rect = selection.pixel_bounds(w, h).unwrap_or(Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        });
        let layer = tab.doc.active_index();
        let mut base = tab.doc.clone();
        base.set_selection(selection.clone());
        let mut pixels = Surface::new_transparent(w, h);
        if kind == MoveKind::Pixels {
            let mask = selection.rasterize(w, h, antialias);
            let src = &tab.doc.layer(layer).surface;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    let m = mask.get(x, y) as u32;
                    if m == 0 {
                        continue;
                    }
                    let p = src.get(x, y);
                    pixels.set(x, y, p.with_alpha((p.a as u32 * m / 255) as u8));
                }
            }
            if !copy {
                base.modify_layer(layer, Some(rect), |l| {
                    fill_masked(&mut l.surface, &mask, ColorBgra::TRANSPARENT)
                });
            }
        }
        self.move_session = Some(MoveSession {
            key: tab.key,
            kind,
            layer,
            base,
            pixels,
            rect,
            selection,
            bounds,
            m: IDENTITY,
            drag: None,
            changed: false,
        });
        true
    }

    /// Redraw the session's result into its document.
    fn render_move(&mut self) {
        let resampling = sampling(self.settings.resampling);
        let Some(s) = self.move_session.as_ref() else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(s.key) else {
            return;
        };
        let mut doc = s.base.clone();
        if s.kind == MoveKind::Pixels {
            doc.modify_layer(s.layer, None, |l| {
                transform::draw_transformed(&mut l.surface, &s.pixels, s.rect, &s.m, resampling)
            });
        }
        doc.set_selection(s.current_selection());
        tab.doc = doc;
    }

    /// End the session (tool change, Enter, other commands). Records "Finish Pixels" /
    /// "Finish Selection" if anything moved.
    pub(crate) fn finish_move(&mut self) {
        let Some(s) = self.move_session.take() else {
            return;
        };
        if !s.changed {
            return;
        }
        if let Some(tab) = self.workspace.tab_mut(s.key) {
            let (name, icon) = match s.kind {
                MoveKind::Pixels => ("Finish Pixels", Icon::MoveSelectedPixels),
                MoveKind::Selection => ("Finish Selection", Icon::MoveSelection),
            };
            tab.commit(name, icon);
        }
    }

    /// Drop the session without recording anything (Undo / Redo take over).
    pub(crate) fn abandon_move(&mut self) {
        self.move_session = None;
    }

    /// Arrow-key nudge (1 px, or 10 px with Ctrl).
    pub(crate) fn nudge(&mut self, dx: f64, dy: f64) {
        let Some(kind) = self.move_kind() else { return };
        if self
            .move_session
            .as_ref()
            .is_none_or(|s| self.workspace.active().is_none_or(|t| t.key != s.key))
            && !self.lift(kind, false)
        {
            return;
        }
        let Some(s) = self.move_session.as_mut() else {
            return;
        };
        s.m = transform::compose(&transform::translation(dx, dy), &s.m);
        s.changed = true;
        let (name, icon) = names(kind, DragMode::Move);
        self.render_move();
        if let Some(tab) = self.workspace.active_mut() {
            tab.commit(name, icon);
        }
    }

    pub(crate) fn move_tool_event(&mut self, event: ToolEvent) {
        let Some(kind) = self.move_kind() else { return };
        let zoom = self
            .workspace
            .active()
            .and_then(|t| t.view)
            .map_or(1.0, |v| v.zoom);
        match event {
            ToolEvent::Down {
                pos,
                button,
                modifiers,
            } => {
                let same_doc = self
                    .move_session
                    .as_ref()
                    .is_some_and(|s| self.workspace.active().is_some_and(|t| t.key == s.key));
                // Ctrl-drag with pixels already floating stamps them and moves a fresh copy.
                if same_doc && modifiers.ctrl && kind == MoveKind::Pixels {
                    self.finish_move();
                }
                if !same_doc || self.move_session.is_none() {
                    self.finish_move();
                    if !self.lift(kind, modifiers.ctrl) {
                        return;
                    }
                }
                let Some(s) = self.move_session.as_mut() else {
                    return;
                };
                let reach = 6.0 / zoom;
                let handle = s
                    .handles()
                    .iter()
                    .position(|h| (h[0] - pos[0]).abs() <= reach && (h[1] - pos[1]).abs() <= reach);
                let inside = s.current_selection().contains(pos);
                let mode = match (button, handle, inside) {
                    (ToolButton::Primary, Some(i), _) => DragMode::Scale(i),
                    (ToolButton::Primary, None, true) => DragMode::Move,
                    _ => {
                        let c = s.center();
                        DragMode::Rotate {
                            center: c,
                            start_angle: (pos[1] - c[1]).atan2(pos[0] - c[0]),
                        }
                    }
                };
                s.drag = Some(MoveDrag {
                    mode,
                    start: pos,
                    m0: s.m,
                });
            }
            ToolEvent::Move { pos } => {
                let (shift, alt) = (self.modifiers.shift, self.modifiers.alt);
                let Some(s) = self.move_session.as_mut() else {
                    return;
                };
                let Some(d) = s.drag.as_ref() else { return };
                s.m = match d.mode {
                    DragMode::Move => transform::compose(
                        &transform::translation(pos[0] - d.start[0], pos[1] - d.start[1]),
                        &d.m0,
                    ),
                    DragMode::Rotate {
                        center,
                        start_angle,
                    } => {
                        let mut a = (pos[1] - center[1]).atan2(pos[0] - center[0]) - start_angle;
                        if shift {
                            let step = 15f64.to_radians();
                            a = (a / step).round() * step;
                        }
                        transform::compose(&transform::rotation_about(a, center), &d.m0)
                    }
                    DragMode::Scale(i) => {
                        // Scale in the selection's own (untransformed) frame.
                        let Some(inv) = transform::invert(&d.m0) else {
                            return;
                        };
                        let p = transform::apply(&inv, pos);
                        let start = transform::apply(&inv, d.start);
                        let [x0, y0, x1, y1] = s.bounds;
                        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                        let opposite = [
                            [x1, y1],
                            [mx, y1],
                            [x0, y1],
                            [x0, my],
                            [x0, y0],
                            [mx, y0],
                            [x1, y0],
                            [x1, my],
                        ][i];
                        let anchor = if alt { [mx, my] } else { opposite };
                        let affects_x = !matches!(i, 1 | 5);
                        let affects_y = !matches!(i, 3 | 7);
                        let f = |p: f64, s0: f64, a: f64| {
                            if (s0 - a).abs() < 1e-9 {
                                1.0
                            } else {
                                (p - a) / (s0 - a)
                            }
                        };
                        let mut sx = if affects_x {
                            f(p[0], start[0], anchor[0])
                        } else {
                            1.0
                        };
                        let mut sy = if affects_y {
                            f(p[1], start[1], anchor[1])
                        } else {
                            1.0
                        };
                        if shift && affects_x && affects_y {
                            let k = sx.abs().max(sy.abs());
                            sx = k * sx.signum();
                            sy = k * sy.signum();
                        }
                        let (sx, sy) = (
                            if sx.abs() < 1e-3 { 1e-3 } else { sx },
                            if sy.abs() < 1e-3 { 1e-3 } else { sy },
                        );
                        transform::compose(&d.m0, &transform::scale_about(sx, sy, anchor))
                    }
                };
                s.changed = true;
                self.render_move();
            }
            ToolEvent::Up { .. } => {
                let Some(s) = self.move_session.as_mut() else {
                    return;
                };
                let Some(d) = s.drag.take() else { return };
                if s.m == d.m0 {
                    return;
                }
                let (name, icon) = names(s.kind, d.mode);
                let key = s.key;
                if let Some(tab) = self.workspace.tab_mut(key) {
                    tab.commit(name, icon);
                }
            }
        }
    }

    /// Nubs to draw on the canvas for the active session.
    pub(crate) fn move_handles(&self) -> Option<[[f64; 2]; 8]> {
        let s = self.move_session.as_ref()?;
        (self.workspace.active()?.key == s.key && self.move_kind().is_some()).then(|| s.handles())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_names_follow_paint_net() {
        assert_eq!(names(MoveKind::Pixels, DragMode::Move).0, "Move Pixels");
        assert_eq!(
            names(MoveKind::Selection, DragMode::Scale(0)).0,
            "Scale Selection"
        );
        assert_eq!(
            names(
                MoveKind::Pixels,
                DragMode::Rotate {
                    center: [0.0; 2],
                    start_angle: 0.0
                }
            )
            .0,
            "Rotate Pixels"
        );
    }
}
