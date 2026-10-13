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
use pnn_effects::framework::{EffectDef, Env, PropKind, Renderer, Value};

use crate::app::PaintNotNetApp;
use crate::icons::Icon;

/// Which menu item an effect came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectRef {
    Adjustment(Adjustment),
}

impl EffectRef {
    pub fn def(self) -> Option<EffectDef> {
        match self {
            Self::Adjustment(a) => pnn_effects::adjustments::def(a),
        }
    }
    /// History icon: the menu icon, or a near one for 5.x-only adjustments.
    fn icon(self) -> Icon {
        match self {
            Self::Adjustment(a) => crate::menus::adjustment_icon(a).unwrap_or(match a {
                Adjustment::InvertAlpha => Icon::InvertColors,
                _ => Icon::BrightnessContrast,
            }),
        }
    }
}

type TileResult = (TileCoord, Option<Arc<Tile<ColorBgra>>>);

struct Job {
    values: Vec<Value>,
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
    before: Surface,
    selection: Option<Arc<Mask>>,
    env: Env,
    job: Option<Job>,
}

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
    let mut row = vec![ColorBgra::TRANSPARENT; (x1.saturating_sub(x0)) as usize];
    for y in y0..y1 {
        renderer.render_row(src, x0, y, &mut row);
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

    /// Start rendering the current values in the background.
    fn spawn(&mut self, ctx: &egui::Context) {
        let renderer: Arc<dyn Renderer> =
            (self.def.build)(&self.values, &self.before, &self.env).into();
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let tiles = self.tiles();
        self.job = Some(Job {
            values: self.values.clone(),
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
        rayon::spawn(move || {
            use rayon::prelude::*;
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
        let renderer = (self.def.build)(&self.values, &self.before, &self.env);
        let (src, selection, bounds) = (&self.before, self.selection.as_deref(), self.env.bounds);
        self.tiles()
            .par_iter()
            .map(|&c| (c, render_tile(renderer.as_ref(), src, selection, bounds, c)))
            .collect()
    }

    /// Set the settings and restart the preview (tests, scripted use).
    pub fn set_values(&mut self, ctx: &egui::Context, values: Vec<Value>) {
        self.values = values;
        self.spawn(ctx);
    }

    /// Smoke test: set every integer property and re-render if it changed.
    pub fn set_all_ints(&mut self, ctx: &egui::Context, v: i32) {
        let new: Vec<Value> = self
            .values
            .iter()
            .map(|x| match x {
                Value::Int(_) => Value::Int(v),
                other => *other,
            })
            .collect();
        if new != self.values {
            self.values = new;
            self.spawn(ctx);
        }
    }

    fn preview_done(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| j.values == self.values && j.received == j.total)
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
        let values = def.defaults();
        let interactive = !def.props.is_empty();
        let Some(session) = self.effect_session(target, def, values) else {
            return;
        };
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
        let bounds = tab.doc.selection().pixel_bounds(w, h).unwrap_or(full);
        let layer = tab.doc.active_index();
        Some(EffectSession {
            key: tab.key,
            layer,
            target,
            def,
            values,
            before: tab.doc.layer(layer).surface.clone(),
            selection: tab.selection_mask(antialias),
            env: Env {
                primary,
                secondary,
                bounds,
            },
            job: None,
        })
    }

    /// Effects > Repeat (Ctrl+F): the last effect with its last settings, no dialog.
    pub(crate) fn repeat_effect(&mut self) -> bool {
        let Some((target, values)) = self.last_applied.clone() else {
            return false;
        };
        let Some(def) = target.def() else {
            return false;
        };
        if let Some(s) = self.effect_session(target, def, values) {
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
        // Drain finished preview tiles.
        if let Some(job) = s.job.as_mut() {
            let tiles: Vec<TileResult> = job.rx.try_iter().collect();
            job.received += tiles.len();
            if let Some(tab) = self.workspace.tab_mut(s.key) {
                put_tiles(tab, s.layer, tiles);
            }
        }
        let mut outcome = None;
        let mut changed = false;
        let def = &s.def;
        let values = &mut s.values;
        egui::Window::new(def.name)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(300.0);
                for (i, p) in def.props.iter().enumerate() {
                    let before = values[i];
                    property_ui(ui, p.name, &p.kind, &mut values[i], p.default);
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
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        outcome = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        outcome = Some(false);
                    }
                });
            });
        let (enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if outcome.is_none() && esc {
            outcome = Some(false);
        } else if outcome.is_none() && enter && !ctx.egui_wants_keyboard_input() {
            outcome = Some(true);
        }
        if changed && let Some(s) = self.effect.as_mut() {
            s.spawn(ctx);
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
            s.job = None;
            let tiles = s.render_all();
            put_tiles(tab, s.layer, tiles);
        }
        tab.commit(s.def.name, s.target.icon());
        self.last_applied = Some((s.target, s.values.clone()));
    }

    /// Name for Effects > Repeat.
    pub(crate) fn last_effect_name(&self) -> Option<&'static str> {
        let (t, _) = self.last_applied.as_ref()?;
        match t {
            EffectRef::Adjustment(a) => Some(a.name()),
        }
    }
}

/// One generated control.
fn property_ui(ui: &mut egui::Ui, name: &str, kind: &PropKind, v: &mut Value, default: Value) {
    match kind {
        PropKind::Int { min, max } => {
            ui.label(name);
            let mut x = v.int();
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut x, *min..=*max).show_value(false));
                ui.add(egui::DragValue::new(&mut x).range(*min..=*max));
                if ui.small_button("↺").on_hover_text("Reset").clicked() {
                    x = default.int();
                }
            });
            *v = Value::Int(x);
        }
        PropKind::Float { min, max, decimals } => {
            ui.label(name);
            let mut x = v.float();
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut x, *min..=*max).show_value(false));
                ui.add(
                    egui::DragValue::new(&mut x)
                        .range(*min..=*max)
                        .max_decimals(*decimals)
                        .speed(10f64.powi(-(*decimals as i32))),
                );
                if ui.small_button("↺").on_hover_text("Reset").clicked() {
                    x = default.float();
                }
            });
            *v = Value::Float(x);
        }
        PropKind::Bool => {
            let mut b = v.bool();
            ui.checkbox(&mut b, name);
            *v = Value::Bool(b);
        }
        PropKind::Choice(options) => {
            ui.label(name);
            let mut c = v.choice().min(options.len().saturating_sub(1));
            egui::ComboBox::from_id_salt(name)
                .selected_text(options.get(c).copied().unwrap_or(""))
                .show_ui(ui, |ui| {
                    for (i, o) in options.iter().enumerate() {
                        ui.selectable_value(&mut c, i, *o);
                    }
                });
            *v = Value::Choice(c);
        }
    }
}
