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

/// See `move_hover`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveHover {
    Handle(usize),
    Inside,
    Outside,
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
    /// The ants mask and how far it has moved, while the session is a whole-pixel move.
    pub fn ants_shift(&self) -> Option<(u64, Arc<pnn_core::Mask>, [f64; 2])> {
        let m = self.m;
        let (id, mask) = self.base_mask.clone()?;
        is_whole_pixel_translation(&m).then_some((id, mask, [m[2], m[5]]))
    }

    /// What the GPU draws while `gpu_drag`.
    pub fn floating_preview(&self, doc: &Document) -> Option<crate::canvas::FloatingPreview> {
        self.gpu_drag.then(|| crate::canvas::FloatingPreview {
            layer: doc.layer(self.layer).id(),
            serial: self.serial,
            pixels: self.pixels.clone(),
            rect: self.rect,
            offset: [self.m[2] as i32, self.m[5] as i32],
        })
    }

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
                // A whole-pixel move of lifted pixels is drawn by the GPU while it lasts, when
                // they fit in one of its buffers: no CPU drawing or uploads per frame.
                let bytes = s.rect.width as u64 * s.rect.height as u64 * 4;
                if mode == DragMode::Move
                    && s.kind == MoveKind::Pixels
                    && bytes <= self.gpu_float_limit
                    && is_whole_pixel_translation(&s.m)
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
                s.m = match d.mode {
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

    /// What dragging from the pointer would do with a move tool, for its cursor: scale by
    /// handle `i` (paint.net's nubs, NW clockwise), move (inside the selection) or rotate
    /// (outside). Before a move starts the handles sit on the selection's bounds, where the
    /// move will put them. Inside is a mask lookup for complex selections.
    pub(crate) fn move_hover(&mut self) -> Option<MoveHover> {
        let pos = self.canvas.cursor?;
        let antialias = self.antialiased_selection();
        let tab = self.workspace.active_mut()?;
        let zoom = tab.view.map_or(1.0, |v| v.zoom);
        let reach = 6.0 / zoom;
        let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
        let session = self.move_session.as_ref().filter(|s| s.key == tab.key);
        let complex = |sel: &Selection| {
            sel.contours().iter().map(Vec::len).sum::<usize>() > crate::canvas::GPU_ANTS_POINTS
        };
        let (handles, inside) = match session {
            Some(s) => {
                let inside = match s.ants_shift() {
                    Some((_, mask, [dx, dy])) => {
                        let (x, y) = (pos[0] - dx, pos[1] - dy);
                        x >= 0.0
                            && y >= 0.0
                            && x < w
                            && y < h
                            && mask.get(x as u32, y as u32) >= 128
                    }
                    None => s.current_selection().contains(pos),
                };
                (s.handles(), inside)
            }
            None => {
                let sel = tab.doc.selection().clone();
                let [x0, y0, x1, y1] = sel.bounds().unwrap_or([0.0, 0.0, w, h]);
                let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let handles = [
                    [x0, y0],
                    [mx, y0],
                    [x1, y0],
                    [x1, my],
                    [x1, y1],
                    [mx, y1],
                    [x0, y1],
                    [x0, my],
                ];
                let inside = if sel.is_empty() {
                    // Nothing selected: the whole layer moves.
                    pos[0] >= 0.0 && pos[1] >= 0.0 && pos[0] < w && pos[1] < h
                } else if complex(&sel) {
                    tab.selection_mask(antialias).is_some_and(|m| {
                        pos[0] >= 0.0
                            && pos[1] >= 0.0
                            && pos[0] < w
                            && pos[1] < h
                            && m.get(pos[0] as u32, pos[1] as u32) >= 128
                    })
                } else {
                    sel.contains(pos)
                };
                (handles, inside)
            }
        };
        if let Some(i) = handles
            .iter()
            .position(|p| (p[0] - pos[0]).abs() <= reach && (p[1] - pos[1]).abs() <= reach)
        {
            return Some(MoveHover::Handle(i));
        }
        Some(if inside {
            MoveHover::Inside
        } else {
            MoveHover::Outside
        })
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
