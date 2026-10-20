//! Running adjustments and effects: an auto-generated dialog with a live preview rendered
//! tile by tile on worker threads, OK/Cancel, and Repeat (Ctrl+F).
//!
//! As paint.net: an effect applies to the active layer within the selection (soft edges mix
//! by coverage); the preview is drawn on the canvas while the dialog is open; Cancel restores
//! the layer; OK records one history step named after the effect; adjustments without
//! settings apply at once. Repeat applies the last effect again with its last settings and
//! no dialog.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

use eframe::egui;
use pnn_core::{ColorBgra, Mask, Rect, Surface, TILE_SIZE, Tile, TileCoord};
use pnn_effects::Adjustment;
use pnn_effects::curves::{self, Curves, Histogram, Levels, Points};
use pnn_effects::framework::{EffectDef, Env, PropDef, PropKind, Renderer, Value};

use crate::app::PaintNotNetApp;
use crate::icons::Icon;

/// Which menu item an effect came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectRef {
    Adjustment(Adjustment),
    Effect(pnn_effects::EffectId),
}

impl EffectRef {
    pub fn def(self) -> Option<EffectDef> {
        match self {
            // Custom dialogs: the settings live in `Custom`, not properties.
            Self::Adjustment(a @ (Adjustment::Curves | Adjustment::Levels)) => Some(EffectDef {
                name: a.name(),
                props: Vec::new(),
                unclipped: false,
                link: None,
                build: |_, _, _| Box::new(pnn_effects::framework::PixelOp(|c| c)),
            }),
            Self::Adjustment(a) => pnn_effects::adjustments::def(a),
            Self::Effect(id) if pnn_effects::effect(id).name == REMOVE_BACKGROUND => {
                Some(EffectDef {
                    name: REMOVE_BACKGROUND,
                    props: Vec::new(),
                    unclipped: false,
                    link: None,
                    build: |_, _, _| Box::new(pnn_effects::framework::PixelOp(|c| c)),
                })
            }
            Self::Effect(id) => pnn_effects::effect_def(id),
        }
    }

    /// Settings for effects with their own dialog.
    fn custom(self, src: &Surface, bounds: Rect, ai: &crate::ai::Ai) -> Option<Custom> {
        match self {
            Self::Effect(id) if pnn_effects::effect(id).name == REMOVE_BACKGROUND => {
                Some(Custom::RemoveBackground(RemoveBackground {
                    quality: ai.default_quality(),
                    ..Default::default()
                }))
            }
            Self::Adjustment(Adjustment::Curves) => Some(Custom::Curves(Curves::default())),
            Self::Adjustment(Adjustment::Levels) => Some(Custom::Levels {
                levels: Levels::default(),
                input: Box::new(Histogram::of(src, bounds)),
            }),
            _ => None,
        }
    }
    /// History icon: the menu icon, or a near one for 5.x-only adjustments.
    fn icon(self) -> Icon {
        match self {
            Self::Adjustment(a) => crate::menus::adjustment_icon(a).unwrap_or(match a {
                Adjustment::InvertAlpha => Icon::InvertColors,
                _ => Icon::BrightnessContrast,
            }),
            Self::Effect(id) => effect_icon(pnn_effects::effect(id).name),
        }
    }
}

pub const REMOVE_BACKGROUND: &str = "Remove Background";

/// Remove Background's settings (the matte itself comes from the model, once per layer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemoveBackground {
    pub quality: pnn_ai::Quality,
    /// Move the edge out (+) or in (−), pixels.
    pub shift: i32,
    /// Blur the edge, pixels (Gaussian standard deviation).
    pub softness: f64,
    /// Keep the background, remove the subject.
    pub invert: bool,
    /// Use the estimated colours of what's kept where the edge is soft (no fringe of what
    /// was removed).
    pub clean_edges: bool,
}

impl Default for RemoveBackground {
    fn default() -> Self {
        Self {
            quality: pnn_ai::Quality::Best,
            shift: 0,
            softness: 0.0,
            invert: false,
            clean_edges: true,
        }
    }
}

impl RemoveBackground {
    fn refine(&self) -> pnn_ai::Refine {
        pnn_ai::Refine {
            shift: self.shift,
            softness: self.softness as f32,
            invert: self.invert,
        }
    }
}

/// The layer with its alpha scaled by a matte and, where it's soft, the estimated colours
/// of what is kept.
struct MatteAlpha {
    coverage: Vec<u8>,
    width: u32,
    matte: Arc<crate::ai::Matte>,
    /// Use the foreground's (or, inverted, the background's) estimated colours.
    colours: Option<bool>,
}

impl Renderer for MatteAlpha {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let row = y as usize * self.width as usize;
        let colours = self.colours.and_then(|bg| {
            self.matte
                .colours
                .get()
                .map(|c| if bg { &c.background } else { &c.foreground })
        });
        for (i, o) in out.iter_mut().enumerate() {
            let x = x0 + i as u32;
            let p = row + x as usize;
            let mut c = src.get(x, y);
            let k = self.coverage[p] as u32;
            // Only opaque pixels: the estimate was made from the layer over white.
            if let Some(rgb) = colours
                && c.a == 255
                && k < 255
            {
                c = ColorBgra::from_rgba(rgb[3 * p], rgb[3 * p + 1], rgb[3 * p + 2], 255);
            }
            c.a = ((c.a as u32 * k + 127) / 255) as u8;
            *o = c;
        }
    }
}

fn matte_renderer(matte: &Arc<crate::ai::Matte>, settings: &RemoveBackground) -> Box<dyn Renderer> {
    let (w, h) = (matte.width as usize, matte.height as usize);
    Box::new(MatteAlpha {
        coverage: settings.refine().apply(&matte.coverage, w, h),
        width: matte.width,
        matte: matte.clone(),
        colours: settings.clean_edges.then_some(settings.invert),
    })
}

/// Settings of effects with a custom dialog (Curves, Levels, Remove Background).
#[derive(Clone, Debug, PartialEq)]
pub enum Custom {
    RemoveBackground(RemoveBackground),
    Curves(Curves),
    Levels {
        levels: Levels,
        /// Histogram of the source (the dialog's input histogram); boxed, it's 6 KiB.
        input: Box<Histogram>,
    },
}

type TileResult = (TileCoord, Option<Arc<Tile<ColorBgra>>>);

struct Job {
    values: Vec<Value>,
    custom: Option<Custom>,
    rx: Receiver<TileResult>,
    cancel: Arc<AtomicBool>,
    total: usize,
    received: usize,
}

pub struct EffectSession {
    key: u64,
    layer: usize,
    target: EffectRef,
    def: EffectDef,
    pub values: Vec<Value>,
    pub custom: Option<Custom>,
    /// Curves: the point being dragged in each edited channel.
    grab: Vec<Option<u8>>,
    before: Surface,
    selection: Option<Arc<Mask>>,
    env: Env,
    job: Option<Job>,
    gpu: Option<Arc<pnn_render::GpuEffects>>,
    /// Remove Background: finding the layer's subject.
    pub matte: Option<crate::ai::MatteJob>,
    /// OK was pressed before the preview finished (a large image): the rest renders in the
    /// background and the effect is committed when it's done.
    applying: bool,
    /// For repaints from the background (set when the session opens).
    ctx: Option<egui::Context>,
}

/// What building an effect's renderer needs, so it can be built on a worker thread: some
/// effects do whole-image work up front (Surface Blur's tables, Quantize's palette, the
/// distance fields of Bevel and Inner Shadow), which froze the UI for up to 1.6 s on a 24 MP
/// photo when it ran here.
struct RendererRecipe {
    build: pnn_effects::framework::Build,
    values: Vec<Value>,
    custom: Option<Custom>,
    matte: Option<Arc<crate::ai::Matte>>,
    before: Surface,
    env: Env,
    gpu: Option<Arc<pnn_render::GpuEffects>>,
}

impl RendererRecipe {
    fn make(self) -> Box<dyn Renderer> {
        let cpu = match &self.custom {
            Some(Custom::RemoveBackground(rb)) => match &self.matte {
                Some(m) => matte_renderer(m, rb),
                // No matte yet: the layer as it is.
                None => Box::new(pnn_effects::framework::PixelOp(|c| c)),
            },
            Some(Custom::Curves(c)) => c.renderer(),
            Some(Custom::Levels { levels, .. }) => levels.renderer(),
            None => (self.build)(&self.values, &self.before, &self.env),
        };
        pnn_render::gpu_renderer(self.gpu.as_ref(), cpu, &self.before, self.env.bounds)
    }
}

/// Effects render on their own threads, one fewer than the CPU has. On rayon's global pool a
/// slow effect's tiles (Surface Blur, a few seconds on a 24 MP photo) queued ahead of the
/// UI thread's own parallel work, and frames stalled for up to 2.4 s while it previewed.
pub fn effect_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(2, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads(n.saturating_sub(1).max(1))
            .thread_name(|i| format!("effect-{i}"))
            .build()
            .expect("effect threads")
    })
}

/// Images up to this many pixels apply at once on OK; larger ones finish in the background.
pub const APPLY_AT_ONCE: u64 = 1_000_000;

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Tiles overlapping `r`.
fn tiles_in(r: Rect) -> Vec<TileCoord> {
    if r.width == 0 || r.height == 0 {
        return Vec::new();
    }
    (r.y / TILE_SIZE..=(r.y + r.height - 1) / TILE_SIZE)
        .flat_map(|ty| {
            (r.x / TILE_SIZE..=(r.x + r.width - 1) / TILE_SIZE).map(move |tx| TileCoord { tx, ty })
        })
        .collect()
}

/// One output tile: the source tile with the effect applied inside `bounds`, mixed by
/// selection coverage.
fn render_tile(
    renderer: &dyn Renderer,
    src: &Surface,
    selection: Option<&Mask>,
    bounds: Rect,
    c: TileCoord,
) -> Option<Arc<Tile<ColorBgra>>> {
    let (ox, oy) = (c.tx * TILE_SIZE, c.ty * TILE_SIZE);
    let mut tile = match src.tile(c) {
        Some(t) => (**t).clone(),
        None => Tile::filled(src.absent()),
    };
    let x0 = bounds.x.max(ox);
    let x1 = (bounds.x + bounds.width)
        .min(ox + TILE_SIZE)
        .min(src.width());
    let y0 = bounds.y.max(oy);
    let y1 = (bounds.y + bounds.height)
        .min(oy + TILE_SIZE)
        .min(src.height());
    if x1 <= x0 || y1 <= y0 {
        return src.tile(c).cloned();
    }
    let rw = (x1 - x0) as usize;
    let mut rendered = vec![ColorBgra::TRANSPARENT; rw * (y1 - y0) as usize];
    let rect = Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    };
    renderer.render_rect(src, rect, &mut rendered);
    for y in y0..y1 {
        let row = &rendered[(y - y0) as usize * rw..(y - y0 + 1) as usize * rw];
        let out = tile.row_mut(y - oy);
        for (i, &new) in row.iter().enumerate() {
            let x = x0 + i as u32;
            let k = selection.map_or(255, |m| m.get(x, y));
            let o = &mut out[(x - ox) as usize];
            *o = match k {
                0 => *o,
                255 => new,
                k => pnn_core::color::lerp_premultiplied(*o, new, k),
            };
        }
    }
    if tile.pixels().iter().all(|&p| p == src.absent()) {
        None
    } else {
        Some(Arc::new(tile))
    }
}

impl EffectSession {
    fn tiles(&self) -> Vec<TileCoord> {
        tiles_in(self.env.bounds)
    }

    /// The renderer for the current values. Remove Background's (refining the matte) is
    /// built here only when rendering synchronously; previews build it on a worker.
    fn recipe(&self) -> RendererRecipe {
        // Missing trailing values (from before an effect gained options) take their defaults.
        let mut values = self.values.clone();
        if values.len() < self.def.props.len() {
            values.extend(
                self.def.props[values.len()..]
                    .iter()
                    .map(|p| p.default_in(&self.env)),
            );
        }
        RendererRecipe {
            build: self.def.build,
            values,
            custom: self.custom.clone(),
            matte: self.matte.as_ref().and_then(|j| j.matte()).cloned(),
            before: self.before.clone(),
            env: self.env.clone(),
            gpu: self.gpu.clone(),
        }
    }

    fn renderer(&self) -> Box<dyn Renderer> {
        self.recipe().make()
    }

    fn spawn(&mut self, ctx: &egui::Context) {
        // The renderer is built on the worker (see `RendererRecipe`).
        let recipe = self.recipe();
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let tiles = self.tiles();
        self.job = Some(Job {
            values: self.values.clone(),
            custom: self.custom.clone(),
            rx,
            cancel: cancel.clone(),
            total: tiles.len(),
            received: 0,
        });
        let (src, selection, bounds, ctx) = (
            self.before.clone(),
            self.selection.clone(),
            self.env.bounds,
            ctx.clone(),
        );
        effect_pool().spawn(move || {
            use rayon::prelude::*;
            let renderer = recipe.make();
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            tiles.par_iter().for_each_with(tx, |tx, &c| {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let t = render_tile(renderer.as_ref(), &src, selection.as_deref(), bounds, c);
                if tx.send((c, t)).is_ok() {
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Render everything now (OK before the preview finished, Repeat, no-dialog adjustments).
    fn render_all(&self) -> Vec<TileResult> {
        use rayon::prelude::*;
        let renderer = self.renderer();
        let (src, selection, bounds) = (&self.before, self.selection.as_deref(), self.env.bounds);
        let tiles = self.tiles();
        effect_pool().install(|| {
            tiles
                .par_iter()
                .map(|&c| (c, render_tile(renderer.as_ref(), src, selection, bounds, c)))
                .collect()
        })
    }

    /// Set the settings and restart the preview (tests, scripted use).
    pub fn set_values(&mut self, ctx: &egui::Context, mut values: Vec<Value>) {
        // Values for options added since (or left out) take their defaults.
        let defaults = self.def.defaults_in(&self.env);
        values.extend(defaults.into_iter().skip(values.len()));
        self.values = values;
        self.spawn(ctx);
    }

    /// Replace a custom dialog's settings and restart the preview (tests).
    pub fn set_custom(&mut self, ctx: &egui::Context, custom: Custom) {
        self.custom = Some(custom);
        self.spawn(ctx);
    }

    /// Smoke test: set every integer property (within its range) and re-render if it
    /// changed.
    pub fn set_all_ints(&mut self, ctx: &egui::Context, v: i32) {
        let new: Vec<Value> = self
            .values
            .iter()
            .zip(&self.def.props)
            .map(|(x, p)| match (x, &p.kind) {
                (Value::Int(_), PropKind::Int { min, max }) => Value::Int(v.clamp(*min, *max)),
                (Value::Int(_), _) => Value::Int(v),
                (other, _) => *other,
            })
            .collect();
        if new != self.values {
            self.values = new;
            self.spawn(ctx);
        }
    }

    /// Remove Background without its matte yet can't be applied.
    fn ready(&self) -> bool {
        !matches!(self.custom, Some(Custom::RemoveBackground(_)))
            || self.matte.as_ref().is_some_and(|j| j.matte().is_some())
    }

    pub fn preview_done(&self) -> bool {
        self.ready()
            && self.job.as_ref().is_some_and(|j| {
                j.values == self.values && j.custom == self.custom && j.received == j.total
            })
    }
}

fn put_tiles(tab: &mut crate::workspace::DocumentTab, layer: usize, tiles: Vec<TileResult>) {
    for (c, t) in tiles {
        let r = Rect {
            x: c.tx * TILE_SIZE,
            y: c.ty * TILE_SIZE,
            width: TILE_SIZE.min(tab.doc.width() - c.tx * TILE_SIZE),
            height: TILE_SIZE.min(tab.doc.height() - c.ty * TILE_SIZE),
        };
        tab.doc
            .modify_layer(layer, Some(r), |l| l.surface.set_tile(c, t));
    }
}

impl PaintNotNetApp {
    /// Adjustments menu / Effects menu: open the dialog, or apply at once if the effect has
    /// no settings.
    pub(crate) fn open_effect(&mut self, ctx: &egui::Context, target: EffectRef) {
        let Some(def) = target.def() else {
            return;
        };
        let Some(mut session) = self.effect_session(target, def, Vec::new(), None) else {
            return;
        };
        session.ctx = Some(ctx.clone());
        session.values = session.def.defaults_in(&session.env);
        session.custom = target.custom(&session.before, session.env.bounds, &self.ai);
        if let Some(Custom::RemoveBackground(rb)) = &session.custom {
            session.matte = Some(self.ai.start(ctx, session.before.clone(), rb.quality, true));
        }
        let interactive = !session.def.props.is_empty() || session.custom.is_some();
        self.effect = Some(session);
        if interactive {
            if let Some(s) = self.effect.as_mut() {
                s.spawn(ctx);
            }
        } else {
            self.finish_effect(true);
        }
    }

    fn effect_session(
        &mut self,
        target: EffectRef,
        def: EffectDef,
        values: Vec<Value>,
        custom: Option<Custom>,
    ) -> Option<EffectSession> {
        let antialias = self.antialiased_selection();
        let (primary, secondary) = (self.settings.primary, self.settings.secondary);
        let tab = self.workspace.active_mut()?;
        let (w, h) = (tab.doc.width(), tab.doc.height());
        let full = Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let unclipped = def.unclipped;
        let bounds = if unclipped {
            full
        } else {
            tab.doc.selection().pixel_bounds(w, h).unwrap_or(full)
        };
        let layer = tab.doc.active_index();
        let selection = tab.selection_mask(antialias);
        let layers = tab
            .doc
            .layers()
            .iter()
            .map(|l| (l.name.clone(), l.surface.clone()))
            .collect();
        Some(EffectSession {
            key: tab.key,
            layer,
            target,
            def,
            values,
            custom,
            grab: Vec::new(),
            before: tab.doc.layer(layer).surface.clone(),
            // Unclipped effects replace the whole layer; they read the selection themselves.
            selection: if unclipped { None } else { selection.clone() },
            env: Env {
                primary,
                secondary,
                bounds,
                selection,
                layers,
                active_layer: layer,
                selection_contours: Arc::new(tab.doc.selection().contours().to_vec()),
            },
            job: None,
            gpu: self.gpu_effects.clone(),
            matte: None,
            applying: false,
            ctx: None,
        })
    }

    /// Effects > Repeat (Ctrl+F): the last effect with its last settings, no dialog.
    pub(crate) fn repeat_effect(&mut self, ctx: &egui::Context) -> bool {
        let Some((target, values, custom)) = self.last_applied.clone() else {
            return false;
        };
        let Some(def) = target.def() else {
            return false;
        };
        // Remove Background reopens its dialog with the last settings: the model has to run
        // on this layer first.
        if let Some(Custom::RemoveBackground(rb)) = custom {
            if let Some(mut s) =
                self.effect_session(target, def, values, Some(Custom::RemoveBackground(rb)))
            {
                s.matte = Some(self.ai.start(ctx, s.before.clone(), rb.quality, true));
                s.spawn(ctx);
                self.effect = Some(s);
            }
            return true;
        }
        if let Some(mut s) = self.effect_session(target, def, values, custom) {
            s.ctx = Some(ctx.clone());
            self.effect = Some(s);
            self.finish_effect(true);
        }
        true
    }

    /// Show the dialog and stream preview tiles onto the canvas.
    pub(crate) fn run_effect(&mut self, ctx: &egui::Context) {
        let Some(s) = self.effect.as_mut() else {
            return;
        };
        // Remove Background: the model finished (or the download did): render with it.
        if let Some(job) = s.matte.as_mut()
            && job.poll(&self.ai, ctx)
            && job.matte().is_some()
        {
            s.spawn(ctx);
        }
        // Drain finished preview tiles.
        if let Some(job) = s.job.as_mut() {
            let tiles: Vec<TileResult> = job.rx.try_iter().collect();
            job.received += tiles.len();
            if let Some(tab) = self.workspace.tab_mut(s.key) {
                put_tiles(tab, s.layer, tiles);
            }
        }
        // OK was pressed on a large image: commit once the last tile is in; until then a
        // small progress window instead of the dialog.
        if s.applying {
            if s.preview_done() {
                self.finish_effect(true);
                return;
            }
            let (got, total) = s
                .job
                .as_ref()
                .map_or((0, 1), |j| (j.received, j.total.max(1)));
            let (name, icon) = (s.def.name, s.target.icon());
            crate::dialogs::movable_dialog(
                ctx,
                egui::Window::new(name)
                    .id(egui::Id::new(("effect_dialog", name)))
                    .collapsible(false)
                    .resizable(false)
                    .pivot(egui::Align2::CENTER_CENTER)
                    .default_pos(ctx.content_rect().center()),
                self.icons.art(icon),
                name,
                None,
                |ui| {
                    ui.set_min_width(260.0);
                    ui.label(format!("Applying {name}…"));
                    ui.add(egui::ProgressBar::new(got as f32 / total as f32));
                },
            );
            return;
        }
        let mut outcome = None;
        let mut changed = false;
        let mut download = false;
        let mut quality = None;
        let ready = s.ready();
        let matte = s.matte.as_ref();
        let models_dir = self.ai.models_dir();
        let def = &s.def;
        let values = &mut s.values;
        let custom = &mut s.custom;
        let grab = &mut s.grab;
        let (before, bounds, env) = (&s.before, s.env.bounds, &s.env);
        // Movable, so the live preview behind it can be watched; opens centred, then stays
        // where it was dragged (egui remembers the position per effect).
        crate::dialogs::movable_dialog(
            ctx,
            egui::Window::new(def.name)
                .id(egui::Id::new(("effect_dialog", def.name)))
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center()),
            self.icons.art(s.target.icon()),
            def.name,
            None,
            |ui| {
                // Generated controls fill one fixed width; Curves and Levels size their graphs.
                if matches!(custom, None | Some(Custom::RemoveBackground(_))) {
                    ui.set_width(crate::value_slider::DIALOG_WIDTH);
                } else {
                    ui.set_min_width(crate::value_slider::DIALOG_WIDTH);
                }
                match custom {
                    Some(Custom::Curves(c)) => changed |= curves_ui(ui, c, grab),
                    Some(Custom::Levels { levels, input }) => {
                        changed |= levels_ui(ui, levels, input, before, bounds)
                    }
                    Some(Custom::RemoveBackground(rb)) => {
                        let before = *rb;
                        if let Some(job) = matte {
                            download = remove_background_ui(ui, rb, job, models_dir);
                        }
                        if rb.quality != before.quality {
                            quality = Some(rb.quality);
                        } else if *rb != before {
                            changed = true;
                        }
                    }
                    None => {}
                }
                let layer_names: Vec<&str> = env.layers.iter().map(|(n, _)| n.as_str()).collect();
                // Controls outside any section first, then each section (collapsed until
                // opened), in the order the properties list them.
                let mut sections: Vec<Option<&'static str>> = vec![None];
                for p in &def.props {
                    if !sections.contains(&p.section) {
                        sections.push(p.section);
                    }
                }
                let mut edit = |ui: &mut egui::Ui, section: Option<&'static str>| {
                    for (i, p) in def.props.iter().enumerate() {
                        if p.section != section {
                            continue;
                        }
                        let before = values[i];
                        property_ui(ui, p, &mut values[i], &layer_names);
                        if values[i] != before {
                            changed = true;
                            // Linked values follow the one edited.
                            if let Some(link) = def.link
                                && values[link.toggle].bool()
                                && link.values.contains(&i)
                            {
                                let v = values[i];
                                for &j in link.values {
                                    values[j] = v;
                                }
                            }
                        }
                    }
                };
                for section in sections {
                    match section {
                        None => edit(ui, None),
                        Some(title) => {
                            egui::CollapsingHeader::new(title)
                                .id_salt(("effect_section", def.name, title))
                                .show(ui, |ui| edit(ui, Some(title)));
                        }
                    }
                }
                ui.add_space(6.0);
                if let Some(ok) = crate::dialogs::ok_cancel(ui, ready) {
                    outcome = Some(ok);
                }
            },
        );
        let (enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if outcome.is_none() && esc {
            outcome = Some(false);
        } else if outcome.is_none() && enter && ready && !ctx.egui_wants_keyboard_input() {
            outcome = Some(true);
        }
        if let Some(s) = self.effect.as_mut() {
            if let Some(job) = s.matte.as_mut() {
                if download {
                    job.download(&self.ai, ctx);
                }
                if let Some(q) = quality {
                    job.set_quality(&self.ai, ctx, q);
                    // The layer as it is until the other model has run.
                    changed = true;
                }
            }
            if changed {
                s.spawn(ctx);
            }
        }
        if let Some(ok) = outcome {
            self.finish_effect(ok);
        }
    }

    /// OK (`apply`) or Cancel.
    pub(crate) fn finish_effect(&mut self, apply: bool) {
        let Some(mut s) = self.effect.take() else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(s.key) else {
            return;
        };
        if !apply {
            let before = s.before.clone();
            tab.doc
                .modify_layer(s.layer, Some(s.env.bounds), |l| l.surface = before);
            return;
        }
        if !s.preview_done() {
            let b = s.env.bounds;
            if b.width as u64 * b.height as u64 > APPLY_AT_ONCE && s.ready() && s.ctx.is_some() {
                // Let the background finish rather than render it all here: `run_effect`
                // commits when the last tile is in.
                if !s
                    .job
                    .as_ref()
                    .is_some_and(|j| j.values == s.values && j.custom == s.custom)
                    && let Some(ctx) = s.ctx.clone()
                {
                    s.spawn(&ctx);
                }
                s.applying = true;
                self.effect = Some(s);
                return;
            }
            s.job = None;
            let tiles = s.render_all();
            put_tiles(tab, s.layer, tiles);
        }
        tab.commit(s.def.name, s.target.icon());
        self.last_applied = Some((s.target, s.values.clone(), s.custom.clone()));
    }

    /// Name for Effects > Repeat.
    pub(crate) fn last_effect_name(&self) -> Option<&'static str> {
        let (t, _, _) = self.last_applied.as_ref()?;
        match t {
            EffectRef::Adjustment(a) => Some(a.name()),
            EffectRef::Effect(id) => Some(pnn_effects::effect(*id).name),
        }
    }
}

/// Remove Background's dialog: model status (download question, progress), quality, and
/// the edge controls once there's a matte. Returns true if Download was pressed.
fn remove_background_ui(
    ui: &mut egui::Ui,
    rb: &mut RemoveBackground,
    job: &crate::ai::MatteJob,
    models_dir: &std::path::Path,
) -> bool {
    use pnn_ai::Quality;
    ui.label("Quality");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut rb.quality, Quality::Best, "Best")
            .on_hover_text("BiRefNet: finer edges, slower, a larger download");
        ui.selectable_value(&mut rb.quality, Quality::Fast, "Fast")
            .on_hover_text("BiRefNet lite: quicker, a smaller download");
    });
    ui.add_space(4.0);
    let download = crate::ai::status_ui(ui, job, models_dir);
    ui.add_space(4.0);
    ui.add_enabled_ui(job.matte().is_some(), |ui| {
        let mut shift = Value::Int(rb.shift);
        property_ui(
            ui,
            &PropDef::int("Shift edge (px)", 0, -30, 30),
            &mut shift,
            &[],
        );
        rb.shift = shift.int();
        let mut soft = Value::Float(rb.softness);
        property_ui(
            ui,
            &PropDef::float("Edge softness (px)", 0.0, 0.0, 20.0, 1),
            &mut soft,
            &[],
        );
        rb.softness = soft.float();
        crate::controls::checkbox(ui, &mut rb.invert, "Keep the background instead");
        crate::controls::checkbox(ui, &mut rb.clean_edges, "Clean up edge colours").on_hover_text(
            "Where the edge is soft (hair, fur), use the colours of what is kept, without a \
             fringe of what was removed",
        );
    });
    download
}

/// One generated control.
fn property_ui(ui: &mut egui::Ui, p: &PropDef, v: &mut Value, layers: &[&str]) {
    use crate::value_slider::ValueSlider;
    let (name, default) = (p.name, p.default);
    match &p.kind {
        PropKind::Int { min, max } => {
            let mut x = v.int();
            ValueSlider::new(name, f64::from(*min), f64::from(*max))
                .default(f64::from(default.int()))
                .show(ui, &mut x);
            *v = Value::Int(x);
        }
        PropKind::Float { min, max, decimals } => {
            let mut x = v.float();
            ValueSlider::new(name, *min, *max)
                .default(default.float())
                .decimals(*decimals)
                .show(ui, &mut x);
            *v = Value::Float(x);
        }
        PropKind::Bool => {
            let mut b = v.bool();
            crate::controls::checkbox(ui, &mut b, name);
            *v = Value::Bool(b);
        }
        PropKind::Choice(options) => {
            ui.label(name);
            let mut c = v.choice().min(options.len().saturating_sub(1));
            egui::ComboBox::from_id_salt(name)
                .width(ui.available_width())
                .selected_text(options.get(c).copied().unwrap_or(""))
                .show_ui(ui, |ui| {
                    for (i, o) in options.iter().enumerate() {
                        ui.selectable_value(&mut c, i, *o);
                    }
                });
            ui.add_space(crate::theme::space::S);
            *v = Value::Choice(c);
        }
        PropKind::Color { alpha, .. } => {
            let c = v.color();
            let mut rgba = egui::Color32::from_rgba_unmultiplied(
                c.r,
                c.g,
                c.b,
                if *alpha { c.a } else { 255 },
            );
            ui.horizontal(|ui| {
                ui.label(name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mode = if *alpha {
                        egui::color_picker::Alpha::OnlyBlend
                    } else {
                        egui::color_picker::Alpha::Opaque
                    };
                    egui::color_picker::color_edit_button_srgba(ui, &mut rgba, mode);
                });
            });
            let [r, g, b, a] = rgba.to_srgba_unmultiplied();
            *v = Value::Color(ColorBgra::from_rgba(r, g, b, a));
        }
        PropKind::Angle { min, max } => {
            let mut x = v.float();
            ValueSlider::new(name, *min, *max)
                .default(default.float())
                .decimals(2)
                .suffix("°")
                .dial()
                .show(ui, &mut x);
            *v = Value::Float(x);
        }
        PropKind::Layer => {
            ui.label(name);
            let mut c = v.choice().min(layers.len().saturating_sub(1));
            egui::ComboBox::from_id_salt(name)
                .width(ui.available_width())
                .selected_text(layers.get(c).copied().unwrap_or(""))
                .show_ui(ui, |ui| {
                    // Top layer first, as in the Layers window.
                    for (i, n) in layers.iter().enumerate().rev() {
                        ui.selectable_value(&mut c, i, *n);
                    }
                });
            ui.add_space(crate::theme::space::S);
            *v = Value::Choice(c);
        }
    }
}

/// The Curves dialog body (3.36 `CurvesEffectConfigDialog` / `CurveControl`): Luminosity or
/// RGB, a 256×256 graph, left-drag to add or move points, right-click to remove one.
fn curves_ui(ui: &mut egui::Ui, c: &mut Curves, grab: &mut Vec<Option<u8>>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Transfer map:");
        egui::ComboBox::from_id_salt("curves_mode")
            .selected_text(if c.luminosity { "Luminosity" } else { "RGB" })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut c.luminosity, true, "Luminosity")
                    .changed();
                changed |= ui
                    .selectable_value(&mut c.luminosity, false, "RGB")
                    .changed();
            });
    });
    let size = egui::vec2(256.0, 256.0);
    let (resp, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Curve graph"));
    let rect = resp.rect;
    let to_screen = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + x / 255.0 * rect.width(),
            rect.bottom() - y / 255.0 * rect.height(),
        )
    };
    let to_curve = |p: egui::Pos2| {
        let x = ((p.x - rect.left()) / rect.width() * 255.0)
            .round()
            .clamp(0.0, 255.0);
        let y = ((rect.bottom() - p.y) / rect.height() * 255.0)
            .round()
            .clamp(0.0, 255.0);
        (x as u8, y as u8)
    };
    // Grid and the identity diagonal.
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let grid = egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.4));
    for i in 1..4 {
        let t = i as f32 * 255.0 / 4.0;
        painter.line_segment([to_screen(t, 0.0), to_screen(t, 255.0)], grid);
        painter.line_segment([to_screen(0.0, t), to_screen(255.0, t)], grid);
    }
    painter.line_segment([to_screen(0.0, 0.0), to_screen(255.0, 255.0)], grid);
    // Interaction.
    if let Some(p) = resp.interact_pointer_pos() {
        let (x, y) = to_curve(p);
        if resp.drag_started_by(egui::PointerButton::Primary)
            || (resp.clicked_by(egui::PointerButton::Primary) && grab.is_empty())
        {
            *grab = c
                .edited()
                .into_iter()
                .map(|pts| {
                    Some(
                        curves::point_near(pts, x as i32, y as i32).unwrap_or_else(|| {
                            let k = x.clamp(1, 254);
                            pts.insert(k, y);
                            k
                        }),
                    )
                })
                .collect();
            changed = true;
        }
        if resp.dragged_by(egui::PointerButton::Primary) && !grab.is_empty() {
            for (pts, g) in c.edited().into_iter().zip(grab.iter_mut()) {
                if let Some(k) = g {
                    // Don't drag onto another point.
                    if *k == 0 || *k == 255 || !pts.contains_key(&x.clamp(1, 254)) || x == *k {
                        *k = curves::move_point(pts, *k, x, y);
                    }
                }
            }
            changed = true;
        }
        if resp.secondary_clicked() {
            for pts in c.edited() {
                if let Some(k) = curves::point_near(pts, x as i32, y as i32)
                    && k != 0
                    && k != 255
                {
                    pts.remove(&k);
                    changed = true;
                }
            }
        }
    }
    if resp.drag_stopped() || resp.clicked() {
        grab.clear();
    }
    // Curves and points.
    let draw = |pts: &Points, color: egui::Color32| {
        let t = curves::spline_table(pts);
        let line: Vec<egui::Pos2> = (0..256).map(|i| to_screen(i as f32, t[i] as f32)).collect();
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, color)));
        for (&px, &py) in pts {
            painter.circle_stroke(
                to_screen(px as f32, py as f32),
                3.5,
                egui::Stroke::new(1.0, color),
            );
        }
    };
    if c.luminosity {
        draw(&c.lum, ui.visuals().strong_text_color());
    } else {
        let colors = [
            egui::Color32::from_rgb(230, 60, 60),
            egui::Color32::from_rgb(60, 200, 60),
            egui::Color32::from_rgb(70, 110, 240),
        ];
        for ((pts, color), on) in c.rgb.iter().zip(colors).zip(c.mask) {
            let color = if on {
                color
            } else {
                color.gamma_multiply(0.35)
            };
            draw(pts, color);
        }
    }
    if let Some(p) = resp.hover_pos() {
        let (x, y) = to_curve(p);
        ui.label(format!("({x}, {y})"));
    } else {
        ui.label(" ");
    }
    if !c.luminosity {
        ui.horizontal(|ui| {
            for (i, name) in ["Red", "Green", "Blue"].iter().enumerate() {
                crate::controls::checkbox(ui, &mut c.mask[i], *name);
            }
        });
    }
    ui.label("Tip: drag to add or move a point; right-click a point to remove it.");
    if ui.button("Reset").clicked() {
        let mode = c.luminosity;
        *c = Curves {
            luminosity: mode,
            ..Curves::default()
        };
        changed = true;
    }
    changed
}

/// The Levels dialog body (3.36 `LevelsEffectConfigDialog`): input black/white with the input
/// histogram, output black/gamma/white with the output histogram, channel checkboxes, Auto
/// and Reset. Values show the first checked channel; edits apply to every checked one.
fn levels_ui(
    ui: &mut egui::Ui,
    l: &mut Levels,
    input: &Histogram,
    src: &Surface,
    bounds: Rect,
) -> bool {
    let mut changed = false;
    // Blue, green, red storage; shown as R, G, B.
    let order = [(2usize, "R"), (1, "G"), (0, "B")];
    let first = order
        .iter()
        .find(|(c, _)| l.mask[*c])
        .map_or(2, |(c, _)| *c);
    let output = input.mapped(&l.curve());
    let histogram = |ui: &mut egui::Ui, h: &Histogram, mask: [bool; 3]| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(130.0, 160.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
        let max = (0..3)
            .flat_map(|c| h.0[c].iter().copied())
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        let colors = [
            egui::Color32::from_rgb(70, 110, 240),
            egui::Color32::from_rgb(60, 200, 60),
            egui::Color32::from_rgb(230, 60, 60),
        ];
        for c in 0..3 {
            if !mask[c] {
                continue;
            }
            // One bar per value, 255 at the top (paint.net draws its histograms vertically);
            // square-root scaled so small counts stay visible.
            let row = rect.height() / 256.0;
            for v in 0..256 {
                let n = h.0[c][v];
                if n == 0 {
                    continue;
                }
                let y = rect.bottom() - (v as f32 + 1.0) * row;
                let w = (n as f32 / max).sqrt() * rect.width();
                p.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left(), y),
                        egui::vec2(w.max(1.0), row.max(1.0)),
                    ),
                    0.0,
                    colors[c].gamma_multiply(0.55),
                );
            }
        }
    };
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Input").strong());
            changed |= level_edit(
                ui,
                l,
                first,
                "White:",
                |l, c| l.in_hi[c] as f64,
                |l, c, v| l.in_hi[c] = (v as u8).max(l.in_lo[c].saturating_add(1)),
                1.0..=255.0,
                1.0,
                0,
            );
            histogram(ui, input, l.mask);
            changed |= level_edit(
                ui,
                l,
                first,
                "Black:",
                |l, c| l.in_lo[c] as f64,
                |l, c, v| l.in_lo[c] = (v as u8).min(l.in_hi[c].saturating_sub(1)),
                0.0..=254.0,
                1.0,
                0,
            );
        });
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Output").strong());
            changed |= level_edit(
                ui,
                l,
                first,
                "White:",
                |l, c| l.out_hi[c] as f64,
                |l, c, v| l.out_hi[c] = (v as u8).max(l.out_lo[c].saturating_add(1)),
                1.0..=255.0,
                1.0,
                0,
            );
            histogram(ui, &output, l.mask);
            changed |= level_edit(
                ui,
                l,
                first,
                "Gamma:",
                |l, c| l.gamma[c] as f64,
                |l, c, v| l.gamma[c] = v as f32,
                0.1..=10.0,
                0.01,
                2,
            );
            changed |= level_edit(
                ui,
                l,
                first,
                "Black:",
                |l, c| l.out_lo[c] as f64,
                |l, c, v| l.out_lo[c] = (v as u8).min(l.out_hi[c].saturating_sub(1)),
                0.0..=254.0,
                1.0,
                0,
            );
        });
    });
    ui.horizontal(|ui| {
        for (c, name) in order {
            crate::controls::checkbox(ui, &mut l.mask[c], name);
        }
        ui.separator();
        if ui.button("Auto").clicked() {
            let mask = l.mask;
            *l = Levels {
                mask,
                ..Levels::auto(src, bounds)
            };
            changed = true;
        }
        if ui.button("Reset").clicked() {
            let mask = l.mask;
            *l = Levels {
                mask,
                ..Levels::default()
            };
            changed = true;
        }
    });
    changed
}

/// One Levels number: shows channel `first`, writes every checked channel.
#[allow(clippy::too_many_arguments)]
fn level_edit(
    ui: &mut egui::Ui,
    l: &mut Levels,
    first: usize,
    label: &str,
    get: fn(&Levels, usize) -> f64,
    set: fn(&mut Levels, usize, f64),
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    decimals: usize,
) -> bool {
    let mut v = get(l, first);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(
                egui::DragValue::new(&mut v)
                    .range(range)
                    .speed(speed)
                    .max_decimals(decimals),
            )
            .changed()
        {
            for c in 0..3 {
                if l.mask[c] {
                    set(l, c, v);
                }
            }
            changed = true;
        }
    });
    changed
}

/// History icon for a menu effect: its 3.36 icon where one exists.
fn effect_icon(name: &str) -> Icon {
    match name {
        "Add Noise" => Icon::FxAddNoise,
        "Bulge" => Icon::FxBulge,
        "Clouds" => Icon::FxClouds,
        "Dents" => Icon::FxDents,
        "Edge Detect" => Icon::FxEdgeDetect,
        "Emboss" => Icon::FxEmboss,
        "Fragment Blur" => Icon::FxFragment,
        "Frosted Glass" => Icon::FxFrostedGlass,
        "Glow" => Icon::FxGlow,
        "Ink Sketch" => Icon::FxInkSketch,
        "Julia Fractal" => Icon::FxJulia,
        "Mandelbrot Fractal" => Icon::FxMandelbrot,
        "Median Blur" => Icon::FxMedian,
        "Motion Blur" => Icon::FxMotionBlur,
        "Oil Painting" => Icon::FxOilPainting,
        "Outline" => Icon::FxOutline,
        "Pencil Sketch" => Icon::FxPencilSketch,
        "Pixelate" => Icon::FxPixelate,
        "Polar Inversion" => Icon::FxPolarInversion,
        "Radial Blur" => Icon::FxRadialBlur,
        "Red Eye Removal" => Icon::FxRedEye,
        "Reduce Noise" => Icon::FxReduceNoise,
        "Relief" => Icon::FxRelief,
        "Sharpen" => Icon::FxSharpen,
        "Soften Portrait" => Icon::FxSoftenPortrait,
        "Surface Blur" => Icon::FxSurfaceBlur,
        "Tile Reflection" => Icon::FxTile,
        "Twist" => Icon::FxTwist,
        "Vignette" => Icon::FxVignette,
        REMOVE_BACKGROUND => Icon::FxRemoveBackground,
        "Zoom Blur" => Icon::FxZoomBlur,
        _ => Icon::FxBlur,
    }
}
