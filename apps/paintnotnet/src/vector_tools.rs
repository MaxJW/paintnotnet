//! Line/Curve (and, in 7b, Shapes): an editable drawing that stays live until committed.
//!
//! Behaviour from paint.net 5's docs (`LineCurveTool.html`; keyboard table in
//! `docs/reference/keybindings.md`):
//! - Drag to draw; left uses the primary colour, right the secondary. Shift (held at release)
//!   snaps to 15°, Alt draws centred on the press point.
//! - Afterwards four nubs edit the curve (either button), the move icon past the end point
//!   moves it (left button), right-drag elsewhere rotates it about its centre (Shift: 15°
//!   steps), arrows move it 1 px (Ctrl: 10 px).
//! - Enter, Esc, toolbar Finish, clicking outside its bounds (which starts a new one), a tool
//!   change or another command commit it. Undo takes back a live drawing.
//! - Toolbar changes (width, caps, dash, curve type, colours, fill, antialiasing, blend)
//!   re-render the live drawing.

use std::sync::Arc;

use pnn_core::{Mask, Rect, Surface};
use pnn_tools::brush;
use pnn_tools::vector::{self, DrawOp, Ink, Part, Pen};
use pnn_tools::{CurveType, ToolBlend};

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::icons::Icon;

/// Nub grab radius in screen pixels (as the move tools).
const NUB_RADIUS: f64 = 6.0;
/// The move icon sits this far (screen px) right and below the end nub.
const MOVE_ICON_OFFSET: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Drawing: the end follows the pointer from `origin`.
    New {
        origin: [f64; 2],
    },
    Nub(usize),
    Move {
        last: [f64; 2],
    },
    Rotate {
        centre: [f64; 2],
        start_angle: f64,
        start: [[f64; 2]; 4],
    },
}

/// What a render depends on, to re-render only when something changed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    nubs: [[f64; 2]; 4],
    curve: CurveType,
    pen: Pen,
    ink: Ink,
    antialias: bool,
    op: DrawOp,
}

pub struct VectorSession {
    key: u64,
    layer: usize,
    before: Surface,
    selection: Option<Arc<Mask>>,
    pub nubs: [[f64; 2]; 4],
    button: ToolButton,
    drag: Option<Drag>,
    /// Canvas area the last render touched (restored on the next).
    last: Option<Rect>,
    rendered: Option<Look>,
}

fn straight(a: [f64; 2], b: [f64; 2]) -> [[f64; 2]; 4] {
    let l = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    [a, l(1.0 / 3.0), l(2.0 / 3.0), b]
}

fn f32s(n: [[f64; 2]; 4]) -> [[f32; 2]; 4] {
    n.map(|p| [p[0] as f32, p[1] as f32])
}

/// 3.36 `GradientTool.ConstrainPoints`-style 15° snap of `b` around `a`.
fn snap15(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let s = pnn_tools::gradient::constrain([a[0] as f32, a[1] as f32], [b[0] as f32, b[1] as f32]);
    [s[0] as f64, s[1] as f64]
}

impl PaintNotNetApp {
    fn vector_zoom(&self) -> f64 {
        self.workspace
            .active()
            .and_then(|t| t.view)
            .map_or(1.0, |v| v.zoom)
    }

    fn vector_look(&self, s: &VectorSession) -> Look {
        let t = &self.settings;
        let (fore, back) = match s.button {
            ToolButton::Primary => (t.primary, t.secondary),
            ToolButton::Secondary => (t.secondary, t.primary),
        };
        Look {
            nubs: s.nubs,
            curve: t.curve_type,
            pen: Pen {
                width: t.brush_width,
                dash: t.dash,
                start_cap: t.start_cap,
                end_cap: t.end_cap,
            },
            ink: Ink {
                fore,
                back,
                fill: t.fill,
            },
            antialias: t.antialiasing,
            op: match t.blend {
                ToolBlend::Overwrite => DrawOp::Overwrite,
                ToolBlend::Blend(m) => DrawOp::Blend(m),
            },
        }
    }

    /// Re-render the live drawing if its geometry or options changed.
    pub(crate) fn refresh_vector(&mut self) {
        let Some(s) = self.vector.as_ref() else {
            return;
        };
        let look = self.vector_look(s);
        if s.rendered == Some(look) {
            return;
        }
        let Some(s) = self.vector.as_mut() else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(s.key) else {
            self.vector = None;
            return;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        let nubs = f32s(look.nubs);
        let parts: Vec<Part> = vector::line_path(&nubs, look.curve)
            .map(|path| {
                let pts = vector::curve_points(&nubs, look.curve, 16);
                let n = pts.len();
                // Tangent points: the first/last sample at least half a pixel from the end.
                let away = |end: [f32; 2], it: &[[f32; 2]]| {
                    it.iter()
                        .copied()
                        .find(|q| (q[0] - end[0]).hypot(q[1] - end[1]) > 0.5)
                        .unwrap_or(end)
                };
                let start_from = away(pts[0], &pts[1..]);
                let rev: Vec<[f32; 2]> = pts[..n - 1].iter().rev().copied().collect();
                let end_from = away(pts[n - 1], &rev);
                vec![Part::Outline {
                    path,
                    pen: look.pen,
                    ink: look.ink,
                    ends: Some(([pts[0], start_from], [pts[n - 1], end_from])),
                }]
            })
            .unwrap_or_default();
        let bounds = vector::parts_bounds(&parts, w, h);
        let drawn = bounds.map(|b| vector::draw(&parts, b, look.antialias));
        let area = brush::union(s.last, bounds);
        if let Some(r) = area {
            let (before, selection) = (&s.before, s.selection.as_deref());
            tab.doc.modify_layer(s.layer, Some(r), |l| {
                vector::composite(
                    &mut l.surface,
                    before,
                    selection,
                    drawn.as_ref(),
                    r,
                    look.op,
                )
            });
        }
        s.last = bounds;
        s.rendered = Some(look);
    }

    pub(crate) fn line_event(&mut self, event: ToolEvent) {
        let zoom = self.vector_zoom();
        let (shift, alt) = (self.modifiers.shift, self.modifiers.alt);
        match event {
            ToolEvent::Down { pos, button, .. } => {
                if let Some(drag) = self.vector_hit(pos, button, zoom) {
                    if let Some(s) = self.vector.as_mut() {
                        s.drag = Some(drag);
                    }
                    return;
                }
                self.commit_vector();
                let antialias = self.antialiased_selection();
                let Some(tab) = self.workspace.active_mut() else {
                    return;
                };
                let layer = tab.doc.active_index();
                self.vector = Some(VectorSession {
                    key: tab.key,
                    layer,
                    before: tab.doc.layer(layer).surface.clone(),
                    selection: tab.selection_mask(antialias),
                    nubs: [pos; 4],
                    button,
                    drag: Some(Drag::New { origin: pos }),
                    last: None,
                    rendered: None,
                });
            }
            ToolEvent::Move { pos } | ToolEvent::Up { pos } => {
                let up = matches!(event, ToolEvent::Up { .. });
                let Some(s) = self.vector.as_mut() else {
                    return;
                };
                match s.drag {
                    Some(Drag::New { origin }) => {
                        let mut end = pos;
                        if shift {
                            end = snap15(origin, end);
                        }
                        let start = if alt {
                            [2.0 * origin[0] - end[0], 2.0 * origin[1] - end[1]]
                        } else {
                            origin
                        };
                        s.nubs = straight(start, end);
                    }
                    Some(Drag::Nub(i)) => s.nubs[i] = pos,
                    Some(Drag::Move { last }) => {
                        let d = [pos[0] - last[0], pos[1] - last[1]];
                        for n in &mut s.nubs {
                            n[0] += d[0];
                            n[1] += d[1];
                        }
                        s.drag = Some(Drag::Move { last: pos });
                    }
                    Some(Drag::Rotate {
                        centre,
                        start_angle,
                        start,
                    }) => {
                        let mut a = (pos[1] - centre[1]).atan2(pos[0] - centre[0]) - start_angle;
                        if shift {
                            let step = std::f64::consts::PI / 12.0;
                            a = (a / step).round() * step;
                        }
                        let (sin, cos) = a.sin_cos();
                        s.nubs = start.map(|p| {
                            let (dx, dy) = (p[0] - centre[0], p[1] - centre[1]);
                            [
                                centre[0] + dx * cos - dy * sin,
                                centre[1] + dx * sin + dy * cos,
                            ]
                        });
                    }
                    None => {}
                }
                if up {
                    s.drag = None;
                    // A click without a drag draws nothing.
                    if s.nubs.iter().all(|n| *n == s.nubs[0]) {
                        self.cancel_vector();
                    }
                }
            }
        }
    }

    /// What a press at `pos` grabs on the live drawing, if anything.
    fn vector_hit(&self, pos: [f64; 2], button: ToolButton, zoom: f64) -> Option<Drag> {
        let s = self.vector.as_ref()?;
        if self.workspace.active().map(|t| t.key) != Some(s.key) {
            return None;
        }
        let reach = NUB_RADIUS / zoom;
        let d = |a: [f64; 2]| (a[0] - pos[0]).hypot(a[1] - pos[1]);
        // Nearest nub within reach, either button.
        if let Some((i, _)) = s
            .nubs
            .iter()
            .enumerate()
            .map(|(i, n)| (i, d(*n)))
            .filter(|(_, dist)| *dist <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return Some(Drag::Nub(i));
        }
        let icon = self.vector_move_icon(zoom)?;
        if button == ToolButton::Primary && d(icon) <= reach * 1.5 {
            return Some(Drag::Move { last: pos });
        }
        // Inside the bounds (plus the pen): right-drag rotates; otherwise it's a new line.
        let (lo, hi) = self.vector_bounds()?;
        let pad = (self.settings.brush_width as f64 / 2.0).max(reach);
        let inside = pos[0] >= lo[0] - pad
            && pos[1] >= lo[1] - pad
            && pos[0] <= hi[0] + pad
            && pos[1] <= hi[1] + pad;
        if inside && button == ToolButton::Secondary {
            let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            return Some(Drag::Rotate {
                centre,
                start_angle: (pos[1] - centre[1]).atan2(pos[0] - centre[0]),
                start: s.nubs,
            });
        }
        None
    }

    fn vector_bounds(&self) -> Option<([f64; 2], [f64; 2])> {
        let s = self.vector.as_ref()?;
        let pts = vector::curve_points(&f32s(s.nubs), self.settings.curve_type, 16);
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in pts.iter().map(|p| [p[0] as f64, p[1] as f64]).chain(s.nubs) {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        Some((lo, hi))
    }

    fn vector_move_icon(&self, zoom: f64) -> Option<[f64; 2]> {
        let s = self.vector.as_ref()?;
        let end = s.nubs[3];
        Some([
            end[0] + MOVE_ICON_OFFSET / zoom,
            end[1] + MOVE_ICON_OFFSET / zoom,
        ])
    }

    /// Nubs and move icon to draw (image coordinates), for the live drawing on this tab.
    pub(crate) fn vector_handles(&self) -> Option<([[f64; 2]; 4], [f64; 2])> {
        let s = self.vector.as_ref()?;
        if self.workspace.active().map(|t| t.key) != Some(s.key)
            || matches!(s.drag, Some(Drag::New { .. }))
        {
            return None;
        }
        Some((s.nubs, self.vector_move_icon(self.vector_zoom())?))
    }

    /// Arrow keys: move the live drawing.
    pub(crate) fn nudge_vector(&mut self, dx: f64, dy: f64) -> bool {
        let Some(s) = self.vector.as_mut() else {
            return false;
        };
        for n in &mut s.nubs {
            n[0] += dx;
            n[1] += dy;
        }
        true
    }

    /// Bake the live drawing into history.
    pub(crate) fn commit_vector(&mut self) {
        self.refresh_vector();
        let Some(s) = self.vector.take() else {
            return;
        };
        if s.last.is_some()
            && let Some(tab) = self.workspace.tab_mut(s.key)
        {
            tab.commit("Line / Curve", Icon::LineCurve);
        }
    }

    /// Drop the live drawing, restoring the layer (Undo).
    pub(crate) fn cancel_vector(&mut self) -> bool {
        let Some(s) = self.vector.take() else {
            return false;
        };
        if let (Some(r), Some(tab)) = (s.last, self.workspace.tab_mut(s.key)) {
            tab.doc.modify_layer(s.layer, Some(r), |l| {
                vector::composite(&mut l.surface, &s.before, None, None, r, DrawOp::Overwrite)
            });
        }
        true
    }
}
