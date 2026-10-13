//! Line/Curve and Shapes: an editable drawing that stays live until committed.
//!
//! Behaviour from paint.net 5's docs (`LineCurveTool.html`, `ShapeTools.html`; keyboard
//! table in `docs/reference/keybindings.md`):
//! - Drag to draw; left uses the primary colour, right the secondary (Shapes' "filled with
//!   outline" mode fills with the other colour). While drawing, Shift snaps a line to 15° or
//!   keeps a shape square; Alt draws centred on the press point.
//! - Lines get four nubs (either button) and a move icon past the end point; right-drag
//!   rotates about the centre.
//! - Shapes get corner nubs (either button; the opposite corner anchors, Shift keeps the
//!   aspect ratio, Alt resizes about the centre, dragging past the opposite corner flips),
//!   a move icon, dragging inside moves, a rotation point (movable, starts at the centre)
//!   that right-drag rotates about, and left-drag just outside rotates too. Shift snaps
//!   rotation to 15°.
//! - Arrows move the drawing 1 px (Ctrl: 10 px).
//! - Enter, Esc, toolbar Finish, a press away from it (which starts a new one), a tool change
//!   or another command commit it. Undo takes back a live drawing. Toolbar changes re-render
//!   it, except the shape type, which applies to the next shape.

use std::sync::Arc;

use pnn_core::{Mask, Rect, Surface};
use pnn_tools::brush;
use pnn_tools::vector::{self, DrawOp, Ink, Part, Pen};
use pnn_tools::{CurveType, LineCap, ShapeDrawType, ShapeType, ToolBlend};

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::icons::Icon;

/// Nub grab radius in screen pixels (as the move tools).
const NUB_RADIUS: f64 = 6.0;
/// The move icon sits this far (screen px) right of and below its anchor point.
const MOVE_ICON_OFFSET: f64 = 16.0;
/// How far outside a shape (screen px) a left-drag rotates instead of starting a new one.
const ROTATE_BAND: f64 = 24.0;

/// Affine map image ← local: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine([f64; 6]);

impl Affine {
    const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    fn invert(&self, p: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        let (x, y) = (p[0] - e, p[1] - f);
        [(d * x - c * y) / det, (-b * x + a * y) / det]
    }

    /// Rotate by `angle` about `centre`, after this map.
    fn rotated(&self, centre: [f64; 2], angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        let r = |p: [f64; 2]| {
            let (dx, dy) = (p[0] - centre[0], p[1] - centre[1]);
            [centre[0] + dx * c - dy * s, centre[1] + dx * s + dy * c]
        };
        let o = r(self.apply([0.0, 0.0]));
        let ux = r(self.apply([1.0, 0.0]));
        let uy = r(self.apply([0.0, 1.0]));
        Self([
            ux[0] - o[0],
            ux[1] - o[1],
            uy[0] - o[0],
            uy[1] - o[1],
            o[0],
            o[1],
        ])
    }

    fn translated(&self, d: [f64; 2]) -> Self {
        let mut m = *self;
        m.0[4] += d[0];
        m.0[5] += d[1];
        m
    }

    /// Rotation angle of the map.
    fn angle(&self) -> f64 {
        self.0[1].atan2(self.0[0])
    }
}

/// A live shape: `rect` (x0, y0, x1, y1; x1 < x0 flips) in a local frame placed by `map`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeGeom {
    /// Fixed when drawn: the 5.x docs list size, orientation, fill, colour, antialiasing and
    /// blend mode as editable, not the shape; A / Shift+A pick the next one.
    shape: ShapeType,
    rect: [f64; 4],
    map: Affine,
    /// Rotation point in image coordinates; `None` = the shape's centre.
    pivot: Option<[f64; 2]>,
}

impl ShapeGeom {
    fn corner_local(&self, i: usize) -> [f64; 2] {
        let [x0, y0, x1, y1] = self.rect;
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]][i]
    }
    fn corners(&self) -> [[f64; 2]; 4] {
        [0, 1, 2, 3].map(|i| self.map.apply(self.corner_local(i)))
    }
    fn centre(&self) -> [f64; 2] {
        let [x0, y0, x1, y1] = self.rect;
        self.map.apply([(x0 + x1) / 2.0, (y0 + y1) / 2.0])
    }
    fn pivot(&self) -> [f64; 2] {
        self.pivot.unwrap_or_else(|| self.centre())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geom {
    Line([[f64; 2]; 4]),
    Shape(ShapeGeom),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Drawing: from `origin` to the pointer.
    New {
        origin: [f64; 2],
    },
    /// A line nub.
    Nub(usize),
    /// A shape corner; `start` is the rect when the drag began.
    Corner {
        i: usize,
        start: [f64; 4],
    },
    Move {
        last: [f64; 2],
    },
    Rotate {
        centre: [f64; 2],
        start_angle: f64,
        start: Geom,
    },
    Pivot,
}

/// What a render depends on, to re-render only when something changed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    geom: Geom,
    curve: CurveType,
    draw: ShapeDrawType,
    corner_radius: f32,
    pen: Pen,
    fore: Ink,
    back: Ink,
    antialias: bool,
    op: DrawOp,
}

pub struct VectorSession {
    key: u64,
    layer: usize,
    before: Surface,
    selection: Option<Arc<Mask>>,
    pub geom: Geom,
    button: ToolButton,
    drag: Option<Drag>,
    /// Canvas area the last render touched (restored on the next).
    last: Option<Rect>,
    rendered: Option<Look>,
}

/// Handles to draw over the canvas for a live drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorHandles {
    pub nubs: Vec<[f64; 2]>,
    pub move_icon: [f64; 2],
    pub pivot: Option<[f64; 2]>,
    /// A shape's (rotated) box.
    pub frame: Option<[[f64; 2]; 4]>,
}

fn straight(a: [f64; 2], b: [f64; 2]) -> [[f64; 2]; 4] {
    let l = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    [a, l(1.0 / 3.0), l(2.0 / 3.0), b]
}

fn f32s(n: [[f64; 2]; 4]) -> [[f32; 2]; 4] {
    n.map(|p| [p[0] as f32, p[1] as f32])
}

/// 15° snap of `b` around `a` (3.36 `ConstrainPoints`).
fn snap15(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let s = pnn_tools::gradient::constrain([a[0] as f32, a[1] as f32], [b[0] as f32, b[1] as f32]);
    [s[0] as f64, s[1] as f64]
}

fn snap_angle(a: f64) -> f64 {
    let step = std::f64::consts::PI / 12.0;
    (a / step).round() * step
}

/// Rect from `origin` to `p`: Shift keeps it square, Alt centres it on `origin`.
fn drawn_rect(origin: [f64; 2], p: [f64; 2], square: bool, centred: bool) -> [f64; 4] {
    let (mut dx, mut dy) = (p[0] - origin[0], p[1] - origin[1]);
    if square {
        let m = dx.abs().max(dy.abs());
        dx = m.copysign(if dx == 0.0 { 1.0 } else { dx });
        dy = m.copysign(if dy == 0.0 { 1.0 } else { dy });
    }
    if centred {
        [
            origin[0] - dx,
            origin[1] - dy,
            origin[0] + dx,
            origin[1] + dy,
        ]
    } else {
        [origin[0], origin[1], origin[0] + dx, origin[1] + dy]
    }
}

/// The parts that draw `look`.
fn parts(look: &Look) -> Vec<Part> {
    match look.geom {
        Geom::Line(nubs) => {
            let nubs = f32s(nubs);
            let Some(path) = vector::line_path(&nubs, look.curve) else {
                return Vec::new();
            };
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
                ink: look.fore,
                ends: Some(([pts[0], start_from], [pts[n - 1], end_from])),
            }]
        }
        Geom::Shape(g) => {
            let [x0, y0, x1, y1] = g.rect;
            if x0 == x1 || y0 == y1 {
                return Vec::new();
            }
            let map = |u: [f64; 2]| g.map.apply([x0 + u[0] * (x1 - x0), y0 + u[1] * (y1 - y0)]);
            let Some(path) = pnn_tools::shapes::shape_path(
                g.shape,
                [x1 - x0, y1 - y0],
                look.corner_radius as f64,
                &map,
            ) else {
                return Vec::new();
            };
            // Shape outlines have no caps.
            let pen = Pen {
                start_cap: LineCap::Flat,
                end_cap: LineCap::Flat,
                ..look.pen
            };
            let outline = |ink| Part::Outline {
                path: path.clone(),
                pen,
                ink,
                ends: None,
            };
            match look.draw {
                ShapeDrawType::Outline => vec![outline(look.fore)],
                ShapeDrawType::Interior => vec![Part::Fill(path.clone(), look.fore)],
                ShapeDrawType::Both => {
                    vec![Part::Fill(path.clone(), look.back), outline(look.fore)]
                }
            }
        }
    }
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
            geom: s.geom,
            curve: t.curve_type,
            draw: t.shape_draw,
            corner_radius: t.corner_radius,
            pen: Pen {
                width: t.brush_width,
                dash: t.dash,
                start_cap: t.start_cap,
                end_cap: t.end_cap,
            },
            fore: Ink {
                fore,
                back,
                fill: t.fill,
            },
            back: Ink {
                fore: back,
                back: fore,
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
        let parts = parts(&look);
        let bounds = vector::parts_bounds(&parts, w, h);
        let drawn = bounds.map(|b| vector::draw(&parts, b, look.antialias));
        if let Some(r) = brush::union(s.last, bounds) {
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

    /// Pointer input for Line/Curve and Shapes.
    pub(crate) fn vector_event(&mut self, event: ToolEvent) {
        let zoom = self.vector_zoom();
        let (shift, alt) = (self.modifiers.shift, self.modifiers.alt);
        let shape_tool = self.tool == pnn_tools::ToolKind::Shapes;
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
                let geom = if shape_tool {
                    Geom::Shape(ShapeGeom {
                        shape: self.settings.shape,
                        rect: [pos[0], pos[1], pos[0], pos[1]],
                        map: Affine::IDENTITY,
                        pivot: None,
                    })
                } else {
                    Geom::Line([pos; 4])
                };
                self.vector = Some(VectorSession {
                    key: tab.key,
                    layer,
                    before: tab.doc.layer(layer).surface.clone(),
                    selection: tab.selection_mask(antialias),
                    geom,
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
                match (s.drag, &mut s.geom) {
                    (Some(Drag::New { origin }), Geom::Line(nubs)) => {
                        let end = if shift { snap15(origin, pos) } else { pos };
                        let start = if alt {
                            [2.0 * origin[0] - end[0], 2.0 * origin[1] - end[1]]
                        } else {
                            origin
                        };
                        *nubs = straight(start, end);
                    }
                    (Some(Drag::New { origin }), Geom::Shape(g)) => {
                        g.rect = drawn_rect(origin, pos, shift, alt);
                    }
                    (Some(Drag::Nub(i)), Geom::Line(nubs)) => nubs[i] = pos,
                    (Some(Drag::Corner { i, start }), Geom::Shape(g)) => {
                        let q = g.map.invert(pos);
                        let [x0, y0, x1, y1] = start;
                        let corner = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                        let anchor = if alt {
                            [(x0 + x1) / 2.0, (y0 + y1) / 2.0]
                        } else {
                            corner[(i + 2) % 4]
                        };
                        let (mut dx, mut dy) = (q[0] - anchor[0], q[1] - anchor[1]);
                        if shift {
                            // Keep the starting aspect ratio.
                            let (w0, h0) = ((x1 - x0).abs().max(1e-9), (y1 - y0).abs().max(1e-9));
                            let k = (dx.abs() / w0).max(dy.abs() / h0);
                            let (sx, sy) = (corner[i][0] - anchor[0], corner[i][1] - anchor[1]);
                            dx = (w0 * k).copysign(if dx == 0.0 { sx } else { dx });
                            dy = (h0 * k).copysign(if dy == 0.0 { sy } else { dy });
                            if alt {
                                dx /= 2.0;
                                dy /= 2.0;
                            }
                        }
                        let far = [anchor[0] + dx, anchor[1] + dy];
                        let near = if alt {
                            [anchor[0] - dx, anchor[1] - dy]
                        } else {
                            anchor
                        };
                        // Corner i becomes `far`; its opposite becomes `near`.
                        let (mut nx0, mut ny0, mut nx1, mut ny1) = (x0, y0, x1, y1);
                        let set = |i: usize, p: [f64; 2], r: &mut (f64, f64, f64, f64)| match i {
                            0 => (r.0, r.1) = (p[0], p[1]),
                            1 => (r.2, r.1) = (p[0], p[1]),
                            2 => (r.2, r.3) = (p[0], p[1]),
                            _ => (r.0, r.3) = (p[0], p[1]),
                        };
                        let mut r = (nx0, ny0, nx1, ny1);
                        set(i, far, &mut r);
                        set((i + 2) % 4, near, &mut r);
                        (nx0, ny0, nx1, ny1) = r;
                        g.rect = [nx0, ny0, nx1, ny1];
                    }
                    (Some(Drag::Move { last }), geom) => {
                        let d = [pos[0] - last[0], pos[1] - last[1]];
                        translate(geom, d);
                        s.drag = Some(Drag::Move { last: pos });
                    }
                    (
                        Some(Drag::Rotate {
                            centre,
                            start_angle,
                            start,
                        }),
                        geom,
                    ) => {
                        let mut a = (pos[1] - centre[1]).atan2(pos[0] - centre[0]) - start_angle;
                        *geom = start;
                        if let Geom::Shape(g) = geom {
                            // Snap the resulting angle, not the change.
                            if shift {
                                let base = g.map.angle();
                                a = snap_angle(base + a) - base;
                            }
                            g.map = g.map.rotated(centre, a);
                        } else if let Geom::Line(nubs) = geom {
                            if shift {
                                a = snap_angle(a);
                            }
                            let (sin, cos) = a.sin_cos();
                            *nubs = nubs.map(|p| {
                                let (dx, dy) = (p[0] - centre[0], p[1] - centre[1]);
                                [
                                    centre[0] + dx * cos - dy * sin,
                                    centre[1] + dx * sin + dy * cos,
                                ]
                            });
                        }
                    }
                    (Some(Drag::Pivot), Geom::Shape(g)) => g.pivot = Some(pos),
                    _ => {}
                }
                if up {
                    s.drag = None;
                    // A click without a drag draws nothing.
                    let empty = match s.geom {
                        Geom::Line(n) => n.iter().all(|p| *p == n[0]),
                        Geom::Shape(g) => g.rect[0] == g.rect[2] || g.rect[1] == g.rect[3],
                    };
                    if empty {
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
        let handles = self.vector_handles_for(s, zoom);
        // Nearest nub within reach, either button.
        if let Some((i, _)) = handles
            .nubs
            .iter()
            .enumerate()
            .map(|(i, n)| (i, d(*n)))
            .filter(|(_, dist)| *dist <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return Some(match s.geom {
                Geom::Line(_) => Drag::Nub(i),
                Geom::Shape(g) => Drag::Corner { i, start: g.rect },
            });
        }
        if button == ToolButton::Primary {
            if handles.pivot.is_some_and(|p| d(p) <= reach * 1.5) {
                return Some(Drag::Pivot);
            }
            if d(handles.move_icon) <= reach * 1.5 {
                return Some(Drag::Move { last: pos });
            }
        }
        let rotate = |centre: [f64; 2]| Drag::Rotate {
            centre,
            start_angle: (pos[1] - centre[1]).atan2(pos[0] - centre[0]),
            start: s.geom,
        };
        match s.geom {
            Geom::Line(nubs) => {
                // Inside the bounds (plus the pen): right-drag rotates.
                let pts = vector::curve_points(&f32s(nubs), self.settings.curve_type, 16);
                let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
                for p in pts.iter().map(|p| [p[0] as f64, p[1] as f64]).chain(nubs) {
                    for k in 0..2 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                let pad = (self.settings.brush_width as f64 / 2.0).max(reach);
                let inside = pos[0] >= lo[0] - pad
                    && pos[1] >= lo[1] - pad
                    && pos[0] <= hi[0] + pad
                    && pos[1] <= hi[1] + pad;
                (inside && button == ToolButton::Secondary)
                    .then(|| rotate([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]))
            }
            Geom::Shape(g) => {
                if button == ToolButton::Secondary {
                    return Some(rotate(g.pivot()));
                }
                // Distance outside the local box, in image px (0 inside).
                let q = g.map.invert(pos);
                let [x0, y0, x1, y1] = g.rect;
                let (lx, hx) = (x0.min(x1), x0.max(x1));
                let (ly, hy) = (y0.min(y1), y0.max(y1));
                let ox = (lx - q[0]).max(q[0] - hx).max(0.0);
                let oy = (ly - q[1]).max(q[1] - hy).max(0.0);
                let out = ox.hypot(oy);
                if out == 0.0 {
                    Some(Drag::Move { last: pos })
                } else if out * zoom <= ROTATE_BAND {
                    Some(rotate(g.pivot()))
                } else {
                    None
                }
            }
        }
    }

    fn vector_handles_for(&self, s: &VectorSession, zoom: f64) -> VectorHandles {
        let off = MOVE_ICON_OFFSET / zoom;
        match s.geom {
            Geom::Line(nubs) => VectorHandles {
                nubs: nubs.to_vec(),
                move_icon: [nubs[3][0] + off, nubs[3][1] + off],
                pivot: None,
                frame: None,
            },
            Geom::Shape(g) => {
                let corners = g.corners();
                // Below-right of the box's lowest-rightmost corner.
                let anchor = corners
                    .iter()
                    .copied()
                    .max_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])))
                    .unwrap_or(corners[2]);
                VectorHandles {
                    nubs: corners.to_vec(),
                    move_icon: [anchor[0] + off, anchor[1] + off],
                    pivot: Some(g.pivot()),
                    frame: Some(corners),
                }
            }
        }
    }

    /// Handles to draw for the live drawing on the active tab.
    pub(crate) fn vector_handles(&self) -> Option<VectorHandles> {
        let s = self.vector.as_ref()?;
        if self.workspace.active().map(|t| t.key) != Some(s.key)
            || matches!(s.drag, Some(Drag::New { .. }))
        {
            return None;
        }
        Some(self.vector_handles_for(s, self.vector_zoom()))
    }

    /// Arrow keys: move the live drawing.
    pub(crate) fn nudge_vector(&mut self, dx: f64, dy: f64) -> bool {
        let Some(s) = self.vector.as_mut() else {
            return false;
        };
        translate(&mut s.geom, [dx, dy]);
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
            match s.geom {
                Geom::Line(_) => tab.commit("Line / Curve", Icon::LineCurve),
                Geom::Shape(_) => tab.commit("Shapes", Icon::Shapes),
            }
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

fn translate(geom: &mut Geom, d: [f64; 2]) {
    match geom {
        Geom::Line(nubs) => {
            for n in nubs {
                n[0] += d[0];
                n[1] += d[1];
            }
        }
        Geom::Shape(g) => {
            g.map = g.map.translated(d);
            if let Some(p) = &mut g.pivot {
                p[0] += d[0];
                p[1] += d[1];
            }
        }
    }
}
