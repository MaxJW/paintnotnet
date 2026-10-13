//! Rectangle / Ellipse / Lasso Select and Magic Wand, plus the Edit menu's selection
//! commands. Modifier chords follow paint.net 5's shortcut table: Ctrl+drag adds, Alt+drag
//! subtracts, Ctrl+right-drag inverts (xor), Alt+right-drag intersects; Shift constrains to a
//! square/circle; Shift with the Magic Wand toggles global flood mode.

use std::sync::Arc;

use pnn_core::selection::flood_stencil;
use pnn_core::{CombineMode, Mask, Selection, flatten};
use pnn_tools::{
    FloodMode, SamplingMode, SelectionDrawMode, SelectionMode, SelectionQuality,
    ToleranceAlphaMode, ToolKind,
};

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::commands::Command;
use crate::icons::Icon;
use crate::workspace::DocumentTab;

/// Cached marching-ant outlines.
pub struct AntsCache {
    revision: u64,
    geometry: u64,
    computed: std::time::Instant,
    contours: Arc<Vec<pnn_core::selection::Contour>>,
    /// Outline being offset on a worker thread while the feather slider moves:
    /// (selection revision, result once done).
    pending: Option<(u64, AntsSlot)>,
}

type AntsSlot = Arc<std::sync::Mutex<Option<Arc<Vec<pnn_core::selection::Contour>>>>>;

/// A selection being drawn.
pub struct SelectDrag {
    key: u64,
    tool: ToolKind,
    mode: CombineMode,
    start: [f64; 2],
    points: Vec<[f64; 2]>,
    base: Selection,
    moved: bool,
}

fn combine_mode(
    toolbar: SelectionMode,
    button: ToolButton,
    m: eframe::egui::Modifiers,
) -> CombineMode {
    match (button, m.ctrl, m.alt) {
        (ToolButton::Primary, true, false) => CombineMode::Union,
        (ToolButton::Primary, false, true) => CombineMode::Exclude,
        (ToolButton::Secondary, true, false) => CombineMode::Xor,
        (ToolButton::Secondary, false, true) => CombineMode::Intersect,
        _ => match toolbar {
            SelectionMode::Replace => CombineMode::Replace,
            SelectionMode::Add => CombineMode::Union,
            SelectionMode::Subtract => CombineMode::Exclude,
            SelectionMode::Intersect => CombineMode::Intersect,
            SelectionMode::Invert => CombineMode::Xor,
        },
    }
}

fn history_name(tool: ToolKind) -> (&'static str, Icon) {
    match tool {
        ToolKind::RectangleSelect => ("Rectangle Select", Icon::RectangleSelect),
        ToolKind::EllipseSelect => ("Ellipse Select", Icon::EllipseSelect),
        ToolKind::LassoSelect => ("Lasso Select", Icon::LassoSelect),
        _ => ("Magic Wand", Icon::MagicWand),
    }
}

impl DocumentTab {
    /// Coverage mask of the selection (cached per selection and quality). `None` when
    /// nothing is selected.
    pub fn selection_mask(&mut self, antialias: bool) -> Option<Arc<Mask>> {
        if self.doc.selection().is_empty() {
            return None;
        }
        let rev = self.doc.selection_revision();
        if let Some((r, aa, m)) = &self.mask_cache
            && *r == rev
            && *aa == antialias
        {
            return Some(m.clone());
        }
        let (w, h) = (self.doc.width(), self.doc.height());
        let sel = self.doc.selection();
        let m = if sel.feather().is_none() {
            sel.rasterize_hard(w, h, antialias)
        } else {
            // The distance field depends only on the geometry: reuse it while the feather
            // radius changes (live slider).
            let field = match &self.feather_field {
                Some(f) if sel.field_matches(f, antialias) => f.clone(),
                _ => {
                    let f = Arc::new(sel.distance_field(w, h, antialias)?);
                    self.feather_field = Some(f.clone());
                    f
                }
            };
            sel.shape_field(&field, w, h)
        };
        let m = Arc::new(m);
        self.mask_cache = Some((rev, antialias, m.clone()));
        Some(m)
    }
}

impl DocumentTab {
    /// Marching-ant outlines (the feather's 50% contour), cached per selection.
    ///
    /// Offsetting the outline costs milliseconds on large selections, so while only the
    /// feather is changing (live slider) it runs on a worker thread and the previous ants
    /// stay up until it finishes.
    pub fn ant_contours(
        &mut self,
        ctx: &eframe::egui::Context,
    ) -> Arc<Vec<pnn_core::selection::Contour>> {
        let rev = self.doc.selection_revision();
        let geometry = self.doc.selection().geometry_id();
        let now = std::time::Instant::now();
        if let Some(cache) = &mut self.ants_cache {
            if let Some((job_rev, slot)) = &cache.pending {
                let done = slot.lock().ok().and_then(|mut s| s.take());
                if let Some(c) = done {
                    cache.revision = *job_rev;
                    cache.computed = now;
                    cache.contours = c;
                    cache.pending = None;
                }
            }
            if cache.revision == rev {
                return cache.contours.clone();
            }
            let live = now.duration_since(cache.computed) < std::time::Duration::from_millis(250);
            if cache.geometry == geometry && live {
                if cache.pending.is_none() {
                    let slot = AntsSlot::default();
                    let (sel, out, ctx) = (self.doc.selection().clone(), slot.clone(), ctx.clone());
                    rayon::spawn(move || {
                        let c = Arc::new(sel.ant_contours());
                        if let Ok(mut s) = out.lock() {
                            *s = Some(c);
                        }
                        ctx.request_repaint();
                    });
                    cache.pending = Some((rev, slot));
                }
                return cache.contours.clone();
            }
        }
        let c = Arc::new(self.doc.selection().ant_contours());
        self.ants_cache = Some(AntsCache {
            revision: rev,
            geometry,
            computed: now,
            contours: c.clone(),
            pending: None,
        });
        c
    }
}

impl PaintNotNetApp {
    pub(crate) fn antialiased_selection(&self) -> bool {
        self.settings.selection_quality == SelectionQuality::Antialiased
    }

    /// The shape between the drag start and `end` for the current tool and toolbar settings.
    fn drag_shape(&self, drag: &SelectDrag, end: [f64; 2], shift: bool) -> Selection {
        if drag.tool == ToolKind::LassoSelect {
            return Selection::polygon(drag.points.clone());
        }
        // Rectangle and ellipse select snap to whole pixels.
        let (sx, sy) = (drag.start[0].round(), drag.start[1].round());
        let (mut ex, mut ey) = (end[0].round(), end[1].round());
        match (drag.tool, self.settings.selection_draw) {
            (ToolKind::RectangleSelect, SelectionDrawMode::FixedSize { width, height }) => {
                return Selection::rect(ex, ey, ex + width as f64, ey + height as f64);
            }
            (ToolKind::RectangleSelect, SelectionDrawMode::FixedRatio { width, height }) => {
                let dx = ex - sx;
                let h = (dx.abs() * height as f64 / width.max(0.01) as f64).round();
                ey = sy + h * if ey < sy { -1.0 } else { 1.0 };
            }
            _ if shift => {
                let side = (ex - sx).abs().max((ey - sy).abs());
                ex = sx + side * (ex - sx).signum();
                ey = sy + side * (ey - sy).signum();
            }
            _ => {}
        }
        if drag.tool == ToolKind::EllipseSelect {
            Selection::ellipse(sx, sy, ex, ey)
        } else {
            Selection::rect(sx, sy, ex, ey)
        }
    }

    pub(crate) fn selection_tool_event(&mut self, event: ToolEvent) {
        match event {
            ToolEvent::Down {
                pos,
                button,
                modifiers,
            } => {
                let Some(tab) = self.workspace.active() else {
                    return;
                };
                let mode = combine_mode(self.settings.selection_mode, button, modifiers);
                if self.tool == ToolKind::MagicWand {
                    let key = tab.key;
                    let global = (self.settings.flood_mode == FloodMode::Global) != modifiers.shift;
                    self.magic_wand(key, pos, mode, !global);
                    return;
                }
                self.select_drag = Some(SelectDrag {
                    key: tab.key,
                    tool: self.tool,
                    mode,
                    start: pos,
                    points: vec![pos],
                    base: tab.doc.selection().clone(),
                    moved: false,
                });
            }
            ToolEvent::Move { pos } => {
                let Some(mut drag) = self.select_drag.take() else {
                    return;
                };
                drag.moved |=
                    (pos[0] - drag.start[0]).abs() >= 0.5 || (pos[1] - drag.start[1]).abs() >= 0.5;
                drag.points.push(pos);
                if drag.moved {
                    let shift = self.modifiers.shift;
                    let shape = self
                        .drag_shape(&drag, pos, shift)
                        .with_feather(self.settings.feather);
                    let combined = drag.base.combine(&shape, drag.mode);
                    if let Some(tab) = self.workspace.tab_mut(drag.key) {
                        tab.doc.set_selection(combined);
                    }
                }
                self.select_drag = Some(drag);
            }
            ToolEvent::Up { .. } => {
                let Some(drag) = self.select_drag.take() else {
                    return;
                };
                let Some(tab) = self.workspace.tab_mut(drag.key) else {
                    return;
                };
                if drag.moved {
                    let (name, icon) = history_name(drag.tool);
                    tab.commit(name, icon);
                } else if !drag.base.is_empty() && drag.mode == CombineMode::Replace {
                    // A click without dragging deselects, as in paint.net.
                    tab.doc.set_selection(Selection::none());
                    tab.commit("Deselect", Icon::Deselect);
                }
            }
        }
    }

    fn magic_wand(&mut self, key: u64, pos: [f64; 2], mode: CombineMode, contiguous: bool) {
        let feather = self.settings.feather;
        let (tolerance, premultiplied, sampling) = (
            self.settings.tolerance,
            self.settings.tolerance_alpha == ToleranceAlphaMode::Premultiplied,
            self.settings.sampling,
        );
        let Some(tab) = self.workspace.tab_mut(key) else {
            return;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        if pos[0] < 0.0 || pos[1] < 0.0 || pos[0] >= w as f64 || pos[1] >= h as f64 {
            return;
        }
        let pixels = match sampling {
            SamplingMode::Layer => tab.doc.layer(tab.doc.active_index()).surface.to_pixels(),
            SamplingMode::Image => flatten(&tab.doc).to_pixels(),
        };
        let stencil = flood_stencil(
            &pixels,
            w,
            h,
            (pos[0] as u32, pos[1] as u32),
            tolerance,
            premultiplied,
            contiguous,
        );
        let shape = Selection::from_stencil(&stencil, w, h).with_feather(feather);
        let combined = tab.doc.selection().combine(&shape, mode);
        tab.doc.set_selection(combined);
        tab.commit("Magic Wand", Icon::MagicWand);
    }

    /// Edit menu selection commands. Returns false if `command` isn't one.
    pub(crate) fn selection_command(&mut self, command: Command) -> bool {
        let antialias = self.antialiased_selection();
        let (primary, secondary) = (self.settings.primary, self.settings.secondary);
        let vp = self.canvas.viewport;
        let Some(tab) = self.workspace.active_mut() else {
            return matches!(
                command,
                Command::SelectAll
                    | Command::Deselect
                    | Command::InvertSelection
                    | Command::EraseSelection
                    | Command::FillSelection
                    | Command::FillSelectionSecondary
                    | Command::CropToSelection
                    | Command::ZoomToSelection
            );
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        let empty = tab.doc.selection().is_empty();
        match command {
            Command::SelectAll => {
                tab.doc.set_selection(Selection::all(w, h));
                tab.commit("Select All", Icon::SelectAll);
            }
            Command::Deselect => {
                if !empty {
                    tab.doc.set_selection(Selection::none());
                    tab.commit("Deselect", Icon::Deselect);
                }
            }
            Command::InvertSelection => {
                let inverted = tab.doc.selection().invert(w, h);
                tab.doc.set_selection(inverted);
                tab.commit("Invert Selection", Icon::InvertSelection);
            }
            Command::EraseSelection | Command::FillSelection | Command::FillSelectionSecondary => {
                let Some(mask) = tab.selection_mask(antialias) else {
                    return true;
                };
                let (fill, name, icon) = match command {
                    Command::EraseSelection => (
                        pnn_core::ColorBgra::TRANSPARENT,
                        "Erase Selection",
                        Icon::EraseSelection,
                    ),
                    Command::FillSelection => (primary, "Fill Selection", Icon::FillSelection),
                    _ => (secondary, "Fill Selection", Icon::FillSelection),
                };
                let bounds = tab.doc.selection().pixel_bounds(w, h);
                let a = tab.doc.active_index();
                tab.doc
                    .modify_layer(a, bounds, |l| fill_masked(&mut l.surface, &mask, fill));
                tab.commit(name, icon);
            }
            Command::CropToSelection => {
                let Some(r) = tab.doc.selection().pixel_bounds(w, h) else {
                    return true;
                };
                let Some(mask) = tab.selection_mask(antialias) else {
                    return true;
                };
                let selection = tab.doc.selection().translate(-(r.x as f64), -(r.y as f64));
                let layers =
                    crate::doc_dialogs::map_layers(&tab.doc, |l| crop_masked(&l.surface, &mask, r));
                tab.doc.replace(r.width, r.height, layers);
                tab.doc.set_selection(selection);
                tab.commit("Crop to Selection", Icon::Crop);
                tab.view = None;
            }
            Command::ZoomToSelection => {
                let Some([x0, y0, x1, y1]) = tab.doc.selection().bounds() else {
                    return true;
                };
                let img = tab.image_size();
                if let Some(view) = tab.view.as_mut() {
                    // Fit the selection's bounds with paint.net's 10 px margin; may enlarge.
                    let (sw, sh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
                    let zoom = ((vp[0] - 10.0) / sw).min((vp[1] - 10.0) / sh);
                    view.zoom_centered(
                        zoom.clamp(pnn_render::view::MIN_ZOOM, pnn_render::view::MAX_ZOOM),
                        img,
                        vp,
                    );
                    view.center = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
                    view.clamp(img, vp);
                }
            }
            _ => return false,
        }
        true
    }
}

/// Fill selected pixels with `color` by coverage: full coverage overwrites (paint.net's Fill
/// and Erase Selection), partial coverage at antialiased edges interpolates in premultiplied
/// space.
pub fn fill_masked(surface: &mut pnn_core::Surface, mask: &Mask, color: pnn_core::ColorBgra) {
    for c in mask.tile_coords().collect::<Vec<_>>() {
        let Some(mtile) = mask.tile(c) else { continue };
        let tile = surface.tile_mut(c);
        for (p, &m) in tile.pixels_mut().iter_mut().zip(mtile.pixels()) {
            if m == 0 {
                continue;
            }
            *p = if m == 255 {
                color
            } else {
                lerp_premultiplied(*p, color, m)
            };
        }
    }
}

pub fn lerp_premultiplied(
    a: pnn_core::ColorBgra,
    b: pnn_core::ColorBgra,
    t: u8,
) -> pnn_core::ColorBgra {
    let t = t as u32;
    let (aa, ba) = (a.a as u32, b.a as u32);
    let out_a = (aa * (255 - t) + ba * t) / 255;
    if out_a == 0 {
        return pnn_core::ColorBgra::TRANSPARENT;
    }
    let ch = |x: u8, y: u8| {
        ((x as u32 * aa * (255 - t) + y as u32 * ba * t) / (out_a * 255)).min(255) as u8
    };
    pnn_core::ColorBgra::from_bgra(ch(a.b, b.b), ch(a.g, b.g), ch(a.r, b.r), out_a as u8)
}

/// Crop a layer to `r`, clearing pixels outside the selection to transparent.
fn crop_masked(surface: &pnn_core::Surface, mask: &Mask, r: pnn_core::Rect) -> pnn_core::Surface {
    let mut out = pnn_core::Surface::new_transparent(r.width, r.height);
    let mut px = Vec::with_capacity((r.width * r.height) as usize);
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            let p = surface.get(x, y);
            let m = mask.get(x, y) as u32;
            px.push(p.with_alpha((p.a as u32 * m / 255) as u8));
        }
    }
    out.write_pixels(&px);
    out.compact();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnn_core::ColorBgra;

    #[test]
    fn modifier_chords() {
        use eframe::egui::Modifiers;
        let m = |ctrl, alt| Modifiers {
            ctrl,
            alt,
            ..Default::default()
        };
        let r = SelectionMode::Replace;
        assert_eq!(
            combine_mode(r, ToolButton::Primary, m(false, false)),
            CombineMode::Replace
        );
        assert_eq!(
            combine_mode(r, ToolButton::Primary, m(true, false)),
            CombineMode::Union
        );
        assert_eq!(
            combine_mode(r, ToolButton::Primary, m(false, true)),
            CombineMode::Exclude
        );
        assert_eq!(
            combine_mode(r, ToolButton::Secondary, m(true, false)),
            CombineMode::Xor
        );
        assert_eq!(
            combine_mode(r, ToolButton::Secondary, m(false, true)),
            CombineMode::Intersect
        );
        assert_eq!(
            combine_mode(SelectionMode::Add, ToolButton::Primary, m(false, false)),
            CombineMode::Union
        );
    }

    #[test]
    fn premultiplied_lerp_keeps_colour_when_fading_out() {
        let red = ColorBgra::from_rgba(255, 0, 0, 255);
        let half = lerp_premultiplied(red, ColorBgra::TRANSPARENT, 128);
        assert_eq!((half.r, half.g, half.b), (255, 0, 0));
        assert_eq!(half.a, 127);
        assert_eq!(
            lerp_premultiplied(red, ColorBgra::WHITE, 255),
            ColorBgra::WHITE
        );
    }
}
