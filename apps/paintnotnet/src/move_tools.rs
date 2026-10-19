//! Move Selected Pixels and Move Selection.
//!
//! Behaviour from paint.net 5's shortcut table: drag inside to move; drag a corner/edge nub
//! to scale (Shift keeps proportions, Alt scales about the centre); drag with the right
//! button to rotate (Shift snaps to 15°); Ctrl when grabbing moves a copy; arrow keys nudge
//! 1 px (Ctrl: 10 px). History steps use paint.net 3.36's names: Move/Scale/Rotate Pixels (or
//! Selection) and Finish Pixels (Selection).
//!
//! paintnotnet's own (user, 2026-10-19): rotation turns about a pivot, drawn at the centre,
//! that can be dragged (it snaps to the centre and the nubs); the left button rotates only
//! from just outside a corner, and anywhere else outside the selection moves it. paint.net
//! 3.36 rotates about the centre from anywhere outside (`MoveToolBase.cs`).
//!
//! Lifting copies the selected pixels (scaled by selection coverage) out of the layer and
//! keeps the document as it was before; every change redraws the lifted pixels, transformed,
//! onto that snapshot. With nothing selected the whole layer moves, as in paint.net.

use std::sync::Arc;

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
    /// Dragging the rotation pivot.
    Pivot,
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
    /// Rotation pivot in the selection's own coordinates (`None`: its centre).
    pivot: Option<[f64; 2]>,
    /// Something moved since lifting (Finish records a step only then).
    pub changed: bool,
    /// The selection's mask when lifted, with its identity: moved by whole pixels, the
    /// marching ants reuse it shifted instead of rasterising the moved outline each frame.
    pub base_mask: Option<(u64, Arc<pnn_core::Mask>)>,
    /// Names these lifted pixels for the GPU (uploaded once).
    pub serial: u64,
    /// A whole-pixel drag is being drawn by the GPU: the document holds the base without the
    /// pixels, which are drawn into it when the drag ends (`render_move`).
    pub gpu_drag: bool,
}

/// `mask` moved by (dx, dy) whole pixels (what leaves the image is dropped). Rows in parallel.
fn shift_mask(mask: &pnn_core::Mask, dx: i64, dy: i64) -> pnn_core::Mask {
    use rayon::prelude::*;
    let (w, h) = (mask.width() as i64, mask.height() as i64);
    let src = mask.to_pixels();
    let mut out = vec![0u8; (w * h) as usize];
    out.par_chunks_mut(w as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let sy = y as i64 - dy;
            if sy < 0 || sy >= h {
                return;
            }
            let (x0, x1) = (dx.max(0), (w + dx).min(w));
            if x1 <= x0 {
                return;
            }
            let s = (sy * w + x0 - dx) as usize;
            row[x0 as usize..x1 as usize].copy_from_slice(&src[s..s + (x1 - x0) as usize]);
        });
    let mut m = pnn_core::Mask::new(w as u32, h as u32, 0);
    m.write_pixels(&out);
    m.compact();
    m
}

/// What a left-button drag from a point does; see `MoveFrame::hit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveHover {
    /// Scale by nub `i` (NW clockwise).
    Handle(usize),
    Pivot,
    Rotate,
    Move,
}

/// A selection's bounds, by (tab, selection revision).
pub(crate) type BoundsCache = ((u64, u64), Option<[f64; 4]>);

/// How near a nub or the pivot the pointer grabs it, in screen pixels.
const REACH: f64 = 6.0;
/// How far from a corner, in screen pixels, a drag outside the selection's box rotates.
const ROTATE_CORNER: f64 = 24.0;
/// The pivot, at the centre, would cover too much of a selection smaller than this on screen
/// (pixels, its shorter side): it isn't shown, and the middle moves the selection.
const PIVOT_MIN_BOX: f64 = 48.0;
/// The pivot's radius on screen, as drawn; it is grabbed within this.
const PIVOT_RADIUS: f64 = 6.5;

/// The move tools' frame on the canvas: the selection's box in its own (untransformed)
/// coordinates, the transform placing it, and the rotation pivot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoveFrame {
    bounds: [f64; 4],
    m: Affine,
    /// In the selection's own coordinates.
    pivot: [f64; 2],
    /// Drawn on the canvas (the pivot can only be grabbed then).
    pub shown: bool,
}

impl MoveFrame {
    fn new(bounds: [f64; 4], m: Affine, pivot: Option<[f64; 2]>, shown: bool) -> Self {
        let [x0, y0, x1, y1] = bounds;
        let pivot = pivot.unwrap_or([(x0 + x1) / 2.0, (y0 + y1) / 2.0]);
        Self {
            bounds,
            m,
            pivot,
            shown,
        }
    }

    /// The selection's own nub positions: corners and edge middles, NW clockwise.
    fn own_handles(&self) -> [[f64; 2]; 8] {
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
    }

    /// Nub positions in image coordinates.
    pub fn handles(&self) -> [[f64; 2]; 8] {
        self.own_handles().map(|p| transform::apply(&self.m, p))
    }

    /// The pivot in image coordinates.
    pub fn pivot(&self) -> [f64; 2] {
        transform::apply(&self.m, self.pivot)
    }

    /// Whether the pivot is drawn and can be grabbed at `zoom`.
    pub fn pivot_shown(&self, zoom: f64) -> bool {
        let h = self.handles();
        let side = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) * zoom;
        self.shown && side(h[0], h[2]).min(side(h[0], h[6])) >= PIVOT_MIN_BOX
    }

    /// What a left-button drag from `pos` (image coordinates) does at `zoom` screen pixels
    /// per image pixel; `inside` says whether `pos` is in the selection. The pivot comes
    /// before the nubs, so that one snapped onto a nub can still be dragged off it.
    fn hit(&self, pos: [f64; 2], zoom: f64, inside: impl FnOnce() -> bool) -> MoveHover {
        let reach = REACH / zoom;
        let dist = |p: [f64; 2]| (p[0] - pos[0]).hypot(p[1] - pos[1]);
        if self.pivot_shown(zoom) && dist(self.pivot()) * zoom <= PIVOT_RADIUS {
            return MoveHover::Pivot;
        }
        let handles = self.handles();
        if let Some(i) = handles
            .iter()
            .position(|h| (h[0] - pos[0]).abs() <= reach && (h[1] - pos[1]).abs() <= reach)
        {
            return MoveHover::Handle(i);
        }
        if inside() {
            return MoveHover::Move;
        }
        // Just outside a corner of the (transformed) box rotates; elsewhere moves.
        let Some(inv) = transform::invert(&self.m) else {
            return MoveHover::Move;
        };
        let [qx, qy] = transform::apply(&inv, pos);
        let [x0, y0, x1, y1] = self.bounds;
        let outside_box = qx < x0 || qx > x1 || qy < y0 || qy > y1;
        let near_corner = [0, 2, 4, 6]
            .iter()
            .any(|&i| dist(handles[i]) * zoom <= ROTATE_CORNER);
        if outside_box && near_corner {
            MoveHover::Rotate
        } else {
            MoveHover::Move
        }
    }

    /// The pivot (own coordinates) for the pointer at `pos`: snapped to the centre or a nub
    /// within reach.
    fn pivot_at(&self, pos: [f64; 2], zoom: f64) -> [f64; 2] {
        let [x0, y0, x1, y1] = self.bounds;
        let centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let snap = std::iter::once(centre)
            .chain(self.own_handles())
            .map(|p| (p, transform::apply(&self.m, p)))
            .map(|(p, q)| (p, (q[0] - pos[0]).hypot(q[1] - pos[1])))
            .filter(|(_, d)| d * zoom <= REACH * 1.5)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match (snap, transform::invert(&self.m)) {
            (Some((p, _)), _) => p,
            (None, Some(inv)) => transform::apply(&inv, pos),
            (None, None) => self.pivot,
        }
    }
}

fn next_serial() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn is_whole_pixel_translation(m: &Affine) -> bool {
    m[0] == 1.0
        && m[1] == 0.0
        && m[3] == 0.0
        && m[4] == 1.0
        && m[2].fract() == 0.0
        && m[5].fract() == 0.0
}

impl MoveSession {
    /// The ants mask and where to read it for an image point (the inverse of the session's
    /// transform), so the GPU ants follow a selection being moved, rotated or scaled.
    pub fn ants_transform(&self) -> Option<(u64, Arc<pnn_core::Mask>, Affine)> {
        let (id, mask) = self.base_mask.clone()?;
        Some((id, mask, transform::invert(&self.m)?))
    }

    /// The ants mask and how far it has moved, while the session is a whole-pixel move.
    pub fn ants_shift(&self) -> Option<(u64, Arc<pnn_core::Mask>, [f64; 2])> {
        let m = self.m;
        let (id, mask) = self.base_mask.clone()?;
        is_whole_pixel_translation(&m).then_some((id, mask, [m[2], m[5]]))
    }

    /// What the GPU draws while `gpu_drag`.
    pub fn floating_preview(
        &self,
        doc: &Document,
        nearest: bool,
    ) -> Option<crate::canvas::FloatingPreview> {
        if !self.gpu_drag {
            return None;
        }
        let m = self.m;
        let inverse = if is_whole_pixel_translation(&m) {
            None
        } else {
            let inv = transform::invert(&m)?;
            let (w, h) = (doc.width() as i64, doc.height() as i64);
            let clip = transform::transformed_bounds(self.rect, &m, w, h)
                .map_or([0; 4], |b| b.map(|v| v as i32));
            Some(pnn_render::FloatingTransform {
                inverse: inv.map(|v| v as f32),
                nearest,
                clip,
            })
        };
        Some(crate::canvas::FloatingPreview {
            layer: doc.layer(self.layer).id(),
            serial: self.serial,
            pixels: self.pixels.clone(),
            rect: self.rect,
            offset: [m[2] as i32, m[5] as i32],
            inverse,
        })
    }

    /// The nubs and pivot where the session has put them.
    pub(crate) fn frame(&self) -> MoveFrame {
        MoveFrame::new(self.bounds, self.m, self.pivot, true)
    }

    /// Whether image point `pos` is in the moved selection: a lookup in the lifted mask when
    /// there is one (a Magic Wand outline has ~700k points), else the outline.
    fn contains(&self, pos: [f64; 2], w: f64, h: f64) -> bool {
        match self.ants_transform() {
            Some((_, mask, inv)) => {
                let [x, y] = transform::apply(&inv, pos);
                x >= 0.0 && y >= 0.0 && x < w && y < h && mask.get(x as u32, y as u32) >= 128
            }
            None => self.current_selection().contains(pos),
        }
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
        (MoveKind::Pixels, DragMode::Move | DragMode::Pivot) => "Move Pixels",
        (MoveKind::Pixels, DragMode::Scale(_)) => "Scale Pixels",
        (MoveKind::Pixels, DragMode::Rotate { .. }) => "Rotate Pixels",
        (MoveKind::Selection, DragMode::Move | DragMode::Pivot) => "Move Selection",
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
        let base_mask = tab
            .selection_mask(antialias)
            .map(|m| (tab.doc.selection_revision() * 2 + u64::from(antialias), m));
        let mut base = tab.doc.clone();
        base.set_selection(selection.clone());
        let mut pixels = Surface::new_transparent(w, h);
        if kind == MoveKind::Pixels {
            // The cached mask when there is a selection (a Magic Wand selection's comes free);
            // tiles in parallel (a pixel at a time took ~0.5 s on a 24 MP photo).
            let mask = match &base_mask {
                Some((_, m)) => m.clone(),
                None => Arc::new(selection.rasterize(w, h, antialias)),
            };
            let src = &tab.doc.layer(layer).surface;
            let coords: Vec<_> = mask.tile_coords().collect();
            let lifted: Vec<_> = {
                use rayon::prelude::*;
                coords
                    .into_par_iter()
                    .filter_map(|c| {
                        let mt = mask.tile(c)?;
                        let st = src.tile(c)?;
                        let mut t = (**st).clone();
                        for (p, &m) in t.pixels_mut().iter_mut().zip(mt.pixels()) {
                            *p = p.with_alpha((p.a as u32 * m as u32 / 255) as u8);
                        }
                        Some((c, Arc::new(t)))
                    })
                    .collect()
            };
            for (c, t) in lifted {
                pixels.set_tile(c, Some(t));
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
            pivot: None,
            changed: false,
            base_mask,
            serial: next_serial(),
            gpu_drag: false,
        });
        true
    }

    /// Start floating `pixels` (pasted) over the document without lifting anything.
    pub(crate) fn begin_float(
        &mut self,
        key: u64,
        pixels: Surface,
        rect: Rect,
        selection: Selection,
    ) {
        let Some(tab) = self.workspace.tab_mut(key) else {
            return;
        };
        let mut base = tab.doc.clone();
        base.set_selection(selection.clone());
        let bounds =
            selection
                .bounds()
                .unwrap_or([0.0, 0.0, rect.width as f64, rect.height as f64]);
        self.move_session = Some(MoveSession {
            key,
            kind: MoveKind::Pixels,
            layer: base.active_index(),
            base,
            pixels,
            rect,
            selection,
            bounds,
            m: IDENTITY,
            drag: None,
            pivot: None,
            changed: true,
            base_mask: None,
            serial: next_serial(),
            gpu_drag: false,
        });
        self.render_move();
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
        self.end_gpu_drag();
        let antialias = self.antialiased_selection();
        let Some(s) = self.move_session.take() else {
            return;
        };
        // The moved selection's mask is the lifted one shifted by whole pixels: keep that
        // rather than rasterise the moved outline (0.1 s for a Magic Wand selection).
        if let Some((_, mask, [dx, dy])) = s.ants_shift()
            && let Some(tab) = self.workspace.tab_mut(s.key)
            && !tab.doc.selection().is_empty()
        {
            let shifted = shift_mask(&mask, dx as i64, dy as i64);
            tab.prime_mask(antialias, Arc::new(shifted));
        }
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

    /// A GPU-drawn drag ends: draw the pixels into the document.
    fn end_gpu_drag(&mut self) {
        if let Some(s) = self.move_session.as_mut()
            && s.gpu_drag
        {
            s.gpu_drag = false;
            self.render_move();
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
                let (w, h) = (s.base.width() as f64, s.base.height() as f64);
                let frame = s.frame();
                let mode = match (button, frame.hit(pos, zoom, || s.contains(pos, w, h))) {
                    (ToolButton::Primary, MoveHover::Handle(i)) => DragMode::Scale(i),
                    (ToolButton::Primary, MoveHover::Pivot) => DragMode::Pivot,
                    (ToolButton::Primary, MoveHover::Move) => DragMode::Move,
                    _ => {
                        let c = frame.pivot();
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
                // Moving, rotating or scaling lifted pixels is drawn by the GPU while the drag
                // lasts, when they fit in one of its buffers: no CPU drawing or uploads per
                // frame (a rotation redrew a 24 MP layer on every core, ~0.45 s a frame).
                let bytes = s.rect.width as u64 * s.rect.height as u64 * 4;
                if s.kind == MoveKind::Pixels
                    && mode != DragMode::Pivot
                    && bytes <= self.gpu_float_limit
                {
                    s.gpu_drag = true;
                    let selection = s.current_selection();
                    let mut doc = s.base.clone();
                    doc.set_selection(selection);
                    if let Some(tab) = self.workspace.tab_mut(s.key) {
                        tab.doc = doc;
                    }
                }
            }
            ToolEvent::Move { pos } => {
                let (shift, alt) = (self.modifiers.shift, self.modifiers.alt);
                let Some(s) = self.move_session.as_mut() else {
                    return;
                };
                let Some(d) = s.drag.as_ref() else { return };
                if d.mode == DragMode::Pivot {
                    // Not a change to the document: no history, nothing redrawn.
                    s.pivot = Some(s.frame().pivot_at(pos, zoom));
                    return;
                }
                s.m = match d.mode {
                    DragMode::Pivot => return,
                    // Whole pixels, as paint.net (3.36 `MoveToolBase`: the offset is the
                    // difference of integer image positions): a fractional move would blur
                    // the pixels, and resample all of them on every frame of the drag.
                    DragMode::Move => transform::compose(
                        &transform::translation(
                            (pos[0] - d.start[0]).round(),
                            (pos[1] - d.start[1]).round(),
                        ),
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
                if s.gpu_drag {
                    // Only the outline moves here; the GPU draws the pixels.
                    let (key, selection) = (s.key, s.current_selection());
                    if let Some(tab) = self.workspace.tab_mut(key) {
                        tab.doc.set_selection(selection);
                    }
                } else {
                    self.render_move();
                }
            }
            ToolEvent::Up { .. } => {
                // A large GPU-drawn drag: the pixels are drawn into the document on the effect
                // threads (a rotation of a 24 MP layer takes ~0.45 s) while the GPU keeps
                // showing them; the history step lands with them.
                let resampling = sampling(self.settings.resampling);
                if let Some(s) = self.move_session.as_mut()
                    && s.gpu_drag
                    && s.rect.width as u64 * s.rect.height as u64
                        > crate::selection_tools::CLICK_AT_ONCE
                    && let Some(d) = s.drag.take()
                {
                    if s.m == d.m0 {
                        s.gpu_drag = false;
                        self.render_move();
                        return;
                    }
                    let (name, icon) = names(s.kind, d.mode);
                    let (key, layer, rect, m) = (s.key, s.layer, s.rect, s.m);
                    let (base, pixels, selection) =
                        (s.base.clone(), s.pixels.clone(), s.current_selection());
                    let antialias = self.antialiased_selection();
                    let work = move || {
                        let mut doc = base;
                        doc.modify_layer(layer, None, |l| {
                            transform::draw_transformed(
                                &mut l.surface,
                                &pixels,
                                rect,
                                &m,
                                resampling,
                            )
                        });
                        // The moved outline's mask, here rather than on the UI thread later
                        // (0.1–0.2 s for a Magic Wand selection).
                        let (w, h) = (doc.width(), doc.height());
                        let mask = (selection.feather().is_none() && !selection.is_empty())
                            .then(|| Arc::new(selection.rasterize_hard(w, h, antialias)));
                        doc.set_selection(selection);
                        (doc, mask)
                    };
                    self.click_job = Some(crate::selection_tools::ClickJob::spawn(
                        self.ctx.clone(),
                        work,
                        move |app, (doc, mask)| {
                            if let Some(s) = app.move_session.as_mut().filter(|s| s.key == key) {
                                s.gpu_drag = false;
                            }
                            if let Some(tab) = app.workspace.tab_mut(key) {
                                tab.doc = doc;
                                tab.commit(name, icon);
                                if let Some(mask) = mask {
                                    tab.prime_mask(antialias, mask);
                                }
                            }
                        },
                    ));
                    return;
                }
                self.end_gpu_drag();
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

    /// What dragging from the pointer would do with a move tool, for its cursor. Before a
    /// move starts the frame sits on the selection's bounds, where the move will put it.
    /// Inside is a mask lookup for complex selections.
    pub(crate) fn move_hover(&mut self) -> Option<MoveHover> {
        let pos = self.canvas.cursor?;
        let antialias = self.antialiased_selection();
        let frame = self.move_frame()?;
        let tab = self.workspace.active_mut()?;
        let zoom = tab.view.map_or(1.0, |v| v.zoom);
        let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
        let in_image = pos[0] >= 0.0 && pos[1] >= 0.0 && pos[0] < w && pos[1] < h;
        let inside = match self.move_session.as_ref().filter(|s| s.key == tab.key) {
            Some(s) => s.contains(pos, w, h),
            // Nothing selected: the whole layer moves.
            None if tab.doc.selection().is_empty() => in_image,
            None if tab
                .doc
                .selection()
                .contours()
                .iter()
                .map(Vec::len)
                .sum::<usize>()
                > crate::canvas::GPU_ANTS_POINTS =>
            {
                in_image
                    && tab
                        .selection_mask(antialias)
                        .is_some_and(|m| m.get(pos[0] as u32, pos[1] as u32) >= 128)
            }
            None => tab.doc.selection().contains(pos),
        };
        Some(frame.hit(pos, zoom, || inside))
    }

    /// The move tools' frame on the active document: the session's, or before a move starts,
    /// on the selection's bounds (the whole image, not drawn, with nothing selected). The
    /// bounds are kept per selection revision (a Magic Wand outline has ~700k points).
    pub(crate) fn move_frame(&mut self) -> Option<MoveFrame> {
        let tab = self.workspace.active()?;
        if let Some(s) = self.move_session.as_ref().filter(|s| s.key == tab.key) {
            return Some(s.frame());
        }
        let id = (tab.key, tab.doc.selection_revision());
        let bounds = match self.selection_bounds {
            Some((k, b)) if k == id => b,
            _ => {
                let b = tab.doc.selection().bounds();
                self.selection_bounds = Some((id, b));
                b
            }
        };
        let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
        Some(match bounds {
            Some(b) => MoveFrame::new(b, IDENTITY, None, true),
            None => MoveFrame::new([0.0, 0.0, w, h], IDENTITY, None, false),
        })
    }

    /// Nubs and pivot to draw on the canvas while a move tool is chosen.
    pub(crate) fn move_handles(&mut self) -> Option<MoveFrame> {
        self.move_kind()?;
        self.move_frame().filter(|f| f.shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifting_a_mask_moves_it_and_drops_what_leaves() {
        let (w, h) = (300u32, 280u32);
        let mut m = pnn_core::Mask::new(w, h, 0);
        for y in 10..40 {
            for x in 250..290 {
                m.set(x, y, 200);
            }
        }
        let s = shift_mask(&m, 30, -15);
        assert_eq!(s.get(285, 0), 200, "moved");
        assert_eq!(s.get(280, 24), 200);
        assert_eq!(s.get(279, 24), 0, "left of where it moved to");
        assert_eq!(s.get(285, 25), 0, "below it");
        let back = shift_mask(&s, -30, 15);
        assert_eq!(back.get(255, 20), 200);
        assert_eq!(back.get(275, 20), 0, "the part that left the image is gone");
    }

    #[test]
    fn left_drags_rotate_only_just_outside_a_corner() {
        let f = MoveFrame::new([0.0, 0.0, 100.0, 80.0], IDENTITY, None, true);
        let hit = |p: [f64; 2], inside: bool| f.hit(p, 1.0, || inside);
        assert_eq!(hit([50.0, 40.0], true), MoveHover::Pivot, "the centre");
        assert_eq!(hit([20.0, 20.0], true), MoveHover::Move, "inside");
        assert_eq!(hit([1.0, -2.0], false), MoveHover::Handle(0), "a nub");
        assert_eq!(
            hit([-10.0, -10.0], false),
            MoveHover::Rotate,
            "outside a corner"
        );
        assert_eq!(hit([110.0, 92.0], false), MoveHover::Rotate);
        assert_eq!(
            hit([50.0, -10.0], false),
            MoveHover::Move,
            "outside an edge"
        );
        assert_eq!(
            hit([-30.0, -30.0], false),
            MoveHover::Move,
            "far from the corner"
        );
        assert_eq!(
            hit([10.0, 10.0], false),
            MoveHover::Move,
            "in the box but not the selection (an ellipse's corner)"
        );
        // The zones turn with the selection: a quarter turn about the centre puts the
        // top-left corner at (90, -10) and the bottom-left at (10, -10).
        let r = MoveFrame::new(
            [0.0, 0.0, 100.0, 80.0],
            transform::rotation_about(std::f64::consts::FRAC_PI_2, [50.0, 40.0]),
            None,
            true,
        );
        assert_eq!(r.hit([100.0, -20.0], 1.0, || false), MoveHover::Rotate);
        assert_eq!(r.hit([50.0, -20.0], 1.0, || false), MoveHover::Move);
        // At 4× the reaches are a quarter as far in image pixels.
        assert_eq!(f.hit([-5.0, -5.0], 4.0, || false), MoveHover::Move);
        assert_eq!(f.hit([-3.0, -3.0], 4.0, || false), MoveHover::Rotate);
    }

    #[test]
    fn a_small_selection_has_no_pivot_to_grab() {
        let f = MoveFrame::new([0.0, 0.0, 40.0, 100.0], IDENTITY, None, true);
        assert!(!f.pivot_shown(1.0));
        assert_eq!(f.hit([20.0, 50.0], 1.0, || true), MoveHover::Move);
        assert!(f.pivot_shown(2.0), "80 screen pixels at 2×");
        assert_eq!(f.hit([20.0, 50.0], 2.0, || true), MoveHover::Pivot);
        let hidden = MoveFrame::new([0.0, 0.0, 400.0, 400.0], IDENTITY, None, false);
        assert_eq!(hidden.hit([200.0, 200.0], 1.0, || true), MoveHover::Move);
    }

    #[test]
    fn the_pivot_snaps_and_follows_the_selection() {
        let m = transform::translation(10.0, 5.0);
        let f = MoveFrame::new([0.0, 0.0, 100.0, 80.0], m, None, true);
        assert_eq!(f.pivot(), [60.0, 45.0], "the moved centre");
        assert_eq!(f.pivot_at([33.0, 21.0], 1.0), [23.0, 16.0], "free");
        assert_eq!(
            f.pivot_at([112.0, 87.0], 1.0),
            [100.0, 80.0],
            "snapped to a corner"
        );
        assert_eq!(
            f.pivot_at([58.0, 47.0], 1.0),
            [50.0, 40.0],
            "snapped to the centre"
        );
        let moved = MoveFrame::new([0.0, 0.0, 100.0, 80.0], m, Some([100.0, 80.0]), true);
        assert_eq!(moved.pivot(), [110.0, 85.0]);
    }

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
