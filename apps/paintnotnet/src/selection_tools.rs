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

/// What a selection mask depends on: the outline (its geometry id), the feather, the
/// quality and the image size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskKey {
    geometry: u64,
    feather: pnn_core::selection::Feather,
    antialias: bool,
    size: (u32, u32),
}

/// Masks kept per image (each one byte per pixel).
const MASKS_KEPT: usize = 4;

/// A selection's distance field being built on the effect threads.
pub struct FieldJob {
    geometry: u64,
    antialias: bool,
    size: (u32, u32),
    rx: std::sync::mpsc::Receiver<Option<Arc<pnn_core::selection::FeatherField>>>,
}

impl DocumentTab {
    fn mask_key(&self, antialias: bool) -> MaskKey {
        let sel = self.doc.selection();
        MaskKey {
            geometry: sel.geometry_id(),
            feather: sel.feather(),
            antialias,
            size: (self.doc.width(), self.doc.height()),
        }
    }

    /// Keep `mask` as the current selection's.
    pub fn prime_mask(&mut self, antialias: bool, mask: Arc<Mask>) {
        let key = self.mask_key(antialias);
        self.mask_cache.retain(|(k, _)| *k != key);
        self.mask_cache.push((key, mask));
        if self.mask_cache.len() > MASKS_KEPT {
            self.mask_cache.remove(0);
        }
    }

    /// Coverage mask of the selection (cached per selection and quality). `None` when
    /// nothing is selected.
    pub fn selection_mask(&mut self, antialias: bool) -> Option<Arc<Mask>> {
        if self.doc.selection().is_empty() {
            return None;
        }
        let key = self.mask_key(antialias);
        if let Some(i) = self.mask_cache.iter().position(|(k, _)| *k == key) {
            let entry = self.mask_cache.remove(i);
            let m = entry.1.clone();
            self.mask_cache.push(entry);
            return Some(m);
        }
        let (w, h) = (self.doc.width(), self.doc.height());
        if !self.doc.selection().feather().is_none() {
            self.poll_field_job(antialias, true);
        }
        let sel = self.doc.selection();
        let m = if sel.feather().is_none() {
            sel.rasterize_hard(w, h, antialias)
        } else {
            // The distance field depends only on the geometry: reuse it while the feather
            // radius changes (live slider).
            let field = match &self.feather_field {
                Some(f) if sel.field_matches(f, antialias) => f.clone(),
                _ => {
                    let hard = self.hard_mask(antialias);
                    let sel = self.doc.selection();
                    let f = Arc::new(sel.distance_field_from(w, h, antialias, hard.as_deref())?);
                    self.feather_field = Some(f.clone());
                    f
                }
            };
            sel.shape_field(&field, w, h)
        };
        let m = Arc::new(m);
        self.prime_mask(antialias, m.clone());
        Some(m)
    }

    /// The selection's unfeathered mask, if kept (saves rasterising the outline again).
    fn hard_mask(&self, antialias: bool) -> Option<Arc<Mask>> {
        let hard_key = MaskKey {
            feather: pnn_core::selection::Feather::NONE,
            ..self.mask_key(antialias)
        };
        self.mask_cache
            .iter()
            .find(|(k, _)| *k == hard_key)
            .map(|(_, m)| m.clone())
    }

    /// Take a finished field job's result. `wait`: block for one building this selection's
    /// field rather than build it a second time.
    fn poll_field_job(&mut self, antialias: bool, wait: bool) {
        use std::sync::mpsc::TryRecvError;
        let key = self.mask_key(antialias);
        let Some(job) = &self.field_job else { return };
        let ours =
            job.geometry == key.geometry && job.antialias == antialias && job.size == key.size;
        let result = if wait && ours {
            job.rx.recv().map_err(|_| TryRecvError::Disconnected)
        } else {
            job.rx.try_recv()
        };
        match result {
            Ok(field) => {
                if let Some(f) = field {
                    self.feather_field = Some(f);
                }
                self.field_job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.field_job = None,
        }
    }

    /// The selection mask for the canvas (overlay, GPU ants) with its upload id. On a large
    /// image a feathered outline's distance field (0.14 s for a Magic Wand selection of a
    /// photo) is built on the effect threads; meanwhile the outline's last mask (with another
    /// feather, under an id of its own) stands in, or nothing.
    pub fn display_mask(
        &mut self,
        antialias: bool,
        ctx: &eframe::egui::Context,
    ) -> Option<(u64, Arc<Mask>)> {
        let sel = self.doc.selection();
        if sel.is_empty() {
            return None;
        }
        let rev = self.doc.selection_revision() * 2 + u64::from(antialias);
        let key = self.mask_key(antialias);
        let (w, h) = key.size;
        let cached = self.mask_cache.iter().any(|(k, _)| *k == key);
        if cached || sel.feather().is_none() || w as u64 * h as u64 <= CLICK_AT_ONCE {
            return self.selection_mask(antialias).map(|m| (rev, m));
        }
        self.poll_field_job(antialias, false);
        let sel = self.doc.selection();
        if self
            .feather_field
            .as_ref()
            .is_some_and(|f| sel.field_matches(f, antialias))
        {
            // Only the shaping is left (~10 ms).
            return self.selection_mask(antialias).map(|m| (rev, m));
        }
        let building = self.field_job.as_ref().is_some_and(|j| {
            j.geometry == key.geometry && j.antialias == antialias && j.size == key.size
        });
        if !building {
            let (sel, hard) = (sel.clone(), self.hard_mask(antialias));
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ctx.clone();
            crate::effect_runner::effect_pool().spawn(move || {
                let field = sel.distance_field_from(w, h, antialias, hard.as_deref());
                let _ = tx.send(field.map(Arc::new));
                ctx.request_repaint();
            });
            self.field_job = Some(FieldJob {
                geometry: key.geometry,
                antialias,
                size: key.size,
                rx,
            });
        }
        self.mask_cache
            .iter()
            .rev()
            .find(|(k, _)| {
                k.geometry == key.geometry && k.antialias == antialias && k.size == key.size
            })
            .map(|(_, m)| (Arc::as_ptr(m) as u64 | 1 << 63, m.clone()))
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

    pub(crate) fn magic_wand(
        &mut self,
        key: u64,
        pos: [f64; 2],
        mode: CombineMode,
        contiguous: bool,
    ) {
        let feather = self.settings.feather;
        let tolerance = pnn_core::selection::Tolerance {
            pct: self.settings.tolerance,
            premultiplied: self.settings.tolerance_alpha == ToleranceAlphaMode::Premultiplied,
            perceptual: self.settings.tolerance_space == pnn_tools::ToleranceSpace::Perceptual,
        };
        let sampling = self.settings.sampling;
        let antialias = self.antialiased_selection();
        let Some(tab) = self.workspace.tab_mut(key) else {
            return;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        if pos[0] < 0.0 || pos[1] < 0.0 || pos[0] >= w as f64 || pos[1] >= h as f64 {
            return;
        }
        // The fill, outline and combine work on a snapshot (in the background on a large
        // image, see `run_click_job`); the result is set when it's done.
        let doc = tab.doc.clone();
        let size = w as u64 * h as u64;
        let seed = (pos[0] as u32, pos[1] as u32);
        let work = move || {
            let pixels = match sampling {
                SamplingMode::Layer => doc.layer(doc.active_index()).surface.to_pixels(),
                SamplingMode::Image => flatten(&doc).to_pixels(),
            };
            let stencil = flood_stencil(&pixels, w, h, seed, tolerance, contiguous);
            let shape = Selection::from_stencil(&stencil, w, h).with_feather(feather);
            let combined = doc.selection().combine(&shape, mode);
            // A new selection's mask is the stencil itself (its outline runs along pixel
            // edges, so rasterising it, antialiased or not, gives back exactly these
            // pixels): keep it rather than rasterise tens of thousands of outlines again
            // (~0.1 s on a 24 MP photo).
            let mask = (mode == CombineMode::Replace && feather.is_none()).then(|| {
                use rayon::prelude::*;
                let bytes: Vec<u8> = stencil
                    .par_iter()
                    .map(|&s| if s { 255 } else { 0 })
                    .collect();
                let mut mask = Mask::new(w, h, 0);
                mask.write_pixels(&bytes);
                mask.compact();
                Arc::new(mask)
            });
            (combined, mask)
        };
        self.run_click_job(size, work, move |app, (combined, mask)| {
            let Some(tab) = app.workspace.tab_mut(key) else {
                return;
            };
            tab.doc.set_selection(combined);
            tab.commit("Magic Wand", Icon::MagicWand);
            if let Some(mask) = mask {
                tab.prime_mask(antialias, mask);
            }
        });
    }

    /// Edit › Invert Selection. On a large image (a Magic Wand selection of a photo is
    /// hundreds of thousands of points; the polygon XOR took 0.2 s) it works on a snapshot in
    /// the background; the cached mask, inverted, comes along so nothing is rasterised again.
    fn invert_selection(&mut self) {
        let antialias = self.antialiased_selection();
        let Some(tab) = self.workspace.active_mut() else {
            return;
        };
        let (w, h, key) = (tab.doc.width(), tab.doc.height(), tab.key);
        let selection = tab.doc.selection().clone();
        let mask = (selection.feather().is_none())
            .then(|| tab.selection_mask(antialias))
            .flatten();
        let work = move || {
            let inverted = selection.invert(w, h);
            let mask = mask.map(|m| {
                use rayon::prelude::*;
                let full = Arc::new(pnn_core::Tile::filled(255u8));
                let tiles: Vec<_> = m
                    .tile_coords()
                    .collect::<Vec<_>>()
                    .into_par_iter()
                    .map(|c| match m.tile(c) {
                        None => (c, Some(full.clone())),
                        Some(t) => {
                            let mut t = (**t).clone();
                            t.pixels_mut().iter_mut().for_each(|v| *v = 255 - *v);
                            (c, Some(Arc::new(t)))
                        }
                    })
                    .collect();
                let mut out = Mask::new(w, h, 0);
                for (c, t) in tiles {
                    out.set_tile(c, t);
                }
                out.compact();
                Arc::new(out)
            });
            (inverted, mask)
        };
        self.run_click_job(w as u64 * h as u64, work, move |app, (inverted, mask)| {
            let Some(tab) = app.workspace.tab_mut(key) else {
                return;
            };
            tab.doc.set_selection(inverted);
            tab.commit("Invert Selection", Icon::InvertSelection);
            if let Some(mask) = mask {
                tab.prime_mask(antialias, mask);
            }
        });
    }

    /// Edit menu selection commands. Returns false if `command` isn't one.
    pub(crate) fn selection_command(&mut self, command: Command) -> bool {
        if command == Command::InvertSelection && self.workspace.active().is_some() {
            self.invert_selection();
            return true;
        }
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
                crate::actions::change_active_layer(
                    tab,
                    move |s| {
                        let mut s = s.clone();
                        fill_masked(&mut s, &mask, fill);
                        s
                    },
                    bounds,
                    (name, icon),
                    &mut self.click_job,
                    self.ctx.clone(),
                );
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
    use rayon::prelude::*;
    let absent = surface.absent();
    let all = pnn_core::Rect {
        x: 0,
        y: 0,
        width: surface.width(),
        height: surface.height(),
    };
    // Tiles in parallel (one at a time took ~0.1 s on a 24 MP photo).
    surface.slots_in(all).into_par_iter().for_each(|(c, slot)| {
        let Some(mtile) = mask.tile(c) else { return };
        let mut tile = slot
            .as_deref()
            .cloned()
            .unwrap_or_else(|| pnn_core::Tile::filled(absent));
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
        *slot = Some(Arc::new(tile));
    });
}

pub use pnn_core::color::lerp_premultiplied;

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

/// A tool click's work running in the background (Magic Wand, Paint Bucket on a large image):
/// what to do with the result, sent back when it's done.
pub struct ClickJob {
    rx: std::sync::mpsc::Receiver<ApplyResult>,
}

/// Sets a finished click job's result on the app.
type ApplyResult = Box<dyn FnOnce(&mut PaintNotNetApp) + Send>;

impl ClickJob {
    /// Run `work` on the effect threads; `apply` its result on the UI thread when it's done.
    pub(crate) fn spawn<T: Send + 'static>(
        ctx: Option<eframe::egui::Context>,
        work: impl FnOnce() -> T + Send + 'static,
        apply: impl FnOnce(&mut PaintNotNetApp, T) + Send + 'static,
    ) -> ClickJob {
        let (tx, rx) = std::sync::mpsc::channel();
        crate::effect_runner::effect_pool().spawn(move || {
            let start = std::time::Instant::now();
            let t = work();
            log::info!(
                "background job done in {:.0} ms",
                start.elapsed().as_secs_f64() * 1e3
            );
            let done: ApplyResult = Box::new(move |app| apply(app, t));
            let _ = tx.send(done);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        ClickJob { rx }
    }
}

/// Images up to this many pixels run a click's work at once (and tests stay synchronous).
pub(crate) const CLICK_AT_ONCE: u64 = 1_000_000;

impl PaintNotNetApp {
    /// Run `work` (on a snapshot) and then `apply` its result here. On a large image `work`
    /// runs on the effect threads and the window keeps drawing; tool input and commands wait
    /// until `apply` has run (`take_click_job`), so nothing changes under it.
    pub(crate) fn run_click_job<T: Send + 'static>(
        &mut self,
        pixels: u64,
        work: impl FnOnce() -> T + Send + 'static,
        apply: impl FnOnce(&mut PaintNotNetApp, T) + Send + 'static,
    ) {
        if pixels <= CLICK_AT_ONCE {
            let t = work();
            apply(self, t);
            return;
        }
        self.click_job = Some(ClickJob::spawn(self.ctx.clone(), work, apply));
    }

    /// Apply a finished click job (every frame). True while one is still running.
    pub(crate) fn take_click_job(&mut self) -> bool {
        let Some(job) = &self.click_job else {
            return false;
        };
        match job.rx.try_recv() {
            Ok(apply) => {
                self.click_job = None;
                apply(self);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.click_job = None;
                false
            }
        }
    }
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
