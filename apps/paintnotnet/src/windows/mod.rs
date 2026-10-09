//! Contents of the floating utility windows.

pub mod colors;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureOptions, pos2, vec2};
use pnn_core::{ColorBgra, Document, LayerId};
use pnn_tools::ToolKind;

use crate::commands::Command;
use crate::icons::{Icon, Icons};
use crate::theme::Palette;
use crate::workspace::DocumentTab;

/// Tools window: paint.net's two-column icon grid.
pub fn tools(
    ui: &mut egui::Ui,
    current: ToolKind,
    icons: &Icons,
    palette: &Palette,
) -> Option<ToolKind> {
    let mut picked = None;
    egui::Grid::new("tools_grid")
        .min_col_width(22.0)
        .spacing(vec2(2.0, 2.0))
        .show(ui, |ui| {
            for (i, &tool) in ToolKind::ALL.iter().enumerate() {
                let selected = tool == current;
                let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                if selected {
                    ui.painter().rect(
                        rect,
                        1.0,
                        palette.selection,
                        egui::Stroke::new(1.0, palette.selection_stroke),
                        egui::StrokeKind::Inside,
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 1.0, palette.hover);
                }
                ui.painter().image(
                    icons.texture(Icon::for_tool(tool)).id(),
                    Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                if response.on_hover_text(tool.tooltip()).clicked() {
                    picked = Some(tool);
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    picked
}

pub enum LayersAction {
    Select(usize),
    ToggleVisible(usize),
    Properties(usize),
    Command(Command),
}

const THUMB: f32 = 40.0;
const THUMB_THROTTLE: Duration = Duration::from_millis(250);

/// Per-tab layer thumbnails, refreshed when the document changes (throttled).
#[derive(Default)]
pub struct LayerThumbs {
    textures: Vec<(LayerId, egui::TextureHandle)>,
    revision: u64,
    updated: Option<Instant>,
}

impl LayerThumbs {
    fn refresh(&mut self, ctx: &egui::Context, doc: &Document, now: Instant) {
        let same_layers = self.textures.len() == doc.layers().len()
            && self
                .textures
                .iter()
                .zip(doc.layers())
                .all(|((id, _), l)| *id == l.id());
        if same_layers && self.revision == doc.revision() {
            return;
        }
        if same_layers
            && let Some(t) = self.updated
            && now.duration_since(t) < THUMB_THROTTLE
        {
            ctx.request_repaint_after(THUMB_THROTTLE);
            return;
        }
        let ppp = ctx.pixels_per_point();
        let (w, h) = (doc.width() as f32, doc.height() as f32);
        let scale = (THUMB * ppp / w.max(h)).min(1.0);
        let (tw, th) = (
            ((w * scale).round() as usize).max(1),
            ((h * scale).round() as usize).max(1),
        );
        self.textures = doc
            .layers()
            .iter()
            .map(|layer| {
                let mut pixels = Vec::with_capacity(tw * th);
                for y in 0..th {
                    for x in 0..tw {
                        let sx = (((x as f32 + 0.5) / scale) as u32).min(doc.width() - 1);
                        let sy = (((y as f32 + 0.5) / scale) as u32).min(doc.height() - 1);
                        let p: ColorBgra = layer.surface.get(sx, sy);
                        let v = if ((x ^ y) & 4) != 0 { 255 } else { 191 };
                        let a = p.a as u32;
                        let mix = |c: u8| ((c as u32 * a + v * (255 - a)) / 255) as u8;
                        pixels.push(Color32::from_rgb(mix(p.r), mix(p.g), mix(p.b)));
                    }
                }
                let image = ColorImage::new([tw, th], pixels);
                (
                    layer.id(),
                    ctx.load_texture(
                        format!("layer-thumb-{:?}", layer.id()),
                        image,
                        TextureOptions::LINEAR,
                    ),
                )
            })
            .collect();
        self.revision = doc.revision();
        self.updated = Some(now);
    }
}

/// Layers window: thumbnails, names, visibility checkboxes; top layer first, as paint.net
/// lists them. Bottom bar: add, delete, duplicate, merge down, move up/down, properties.
pub fn layers(
    ui: &mut egui::Ui,
    tab: Option<&mut DocumentTab>,
    icons: &Icons,
    palette: &Palette,
    enabled: &dyn Fn(Command) -> bool,
    now: Instant,
) -> Vec<LayersAction> {
    let mut actions = Vec::new();
    let bar_height = 24.0;
    let list_height = (ui.available_height() - bar_height).max(40.0);
    egui::Frame::new().fill(palette.list).show(ui, |ui| {
        ui.set_min_height(list_height);
        ui.set_max_height(list_height);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let Some(DocumentTab {
                    doc,
                    layer_thumbs: thumbs,
                    ..
                }) = tab
                else {
                    return;
                };
                thumbs.refresh(ui.ctx(), doc, now);
                for index in (0..doc.layers().len()).rev() {
                    let layer = doc.layer(index);
                    let selected = index == doc.active_index();
                    let row_width = ui.available_width();
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(row_width, THUMB + 8.0), Sense::click());
                    if selected {
                        ui.painter().rect(
                            rect.shrink(1.0),
                            0.0,
                            palette.selection,
                            egui::Stroke::new(1.0, palette.selection_stroke),
                            egui::StrokeKind::Inside,
                        );
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(rect.shrink(1.0), 0.0, palette.hover);
                    }
                    if let Some((_, tex)) = thumbs.textures.iter().find(|(id, _)| *id == layer.id())
                    {
                        let size = tex.size_vec2() / ui.ctx().pixels_per_point();
                        let thumb_area =
                            Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(THUMB, THUMB));
                        let shown = Rect::from_center_size(thumb_area.center(), size);
                        ui.painter().image(
                            tex.id(),
                            shown,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        ui.painter().rect_stroke(
                            shown,
                            0.0,
                            egui::Stroke::new(1.0, palette.window_border),
                            egui::StrokeKind::Outside,
                        );
                    }
                    let name_clip = Rect::from_min_max(
                        pos2(rect.min.x + THUMB + 12.0, rect.min.y),
                        pos2(rect.max.x - 24.0, rect.max.y),
                    );
                    ui.painter_at(name_clip).text(
                        pos2(name_clip.min.x, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &layer.name,
                        egui::FontId::proportional(12.0),
                        palette.text,
                    );
                    let check_rect = Rect::from_center_size(
                        pos2(rect.max.x - 12.0, rect.center().y),
                        vec2(16.0, 16.0),
                    );
                    let mut visible = layer.visible;
                    if ui
                        .put(check_rect, egui::Checkbox::without_text(&mut visible))
                        .clicked()
                    {
                        actions.push(LayersAction::ToggleVisible(index));
                    } else if response.double_clicked() {
                        actions.push(LayersAction::Properties(index));
                    } else if response.clicked() {
                        actions.push(LayersAction::Select(index));
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (icon, command, tip) in [
            (Icon::AddLayer, Command::AddNewLayer, "Add New Layer"),
            (Icon::DeleteLayer, Command::DeleteLayer, "Delete Layer"),
            (
                Icon::DuplicateLayer,
                Command::DuplicateLayer,
                "Duplicate Layer",
            ),
            (
                Icon::MergeLayerDown,
                Command::MergeLayerDown,
                "Merge Layer Down",
            ),
            (Icon::MoveLayerUp, Command::MoveLayerUp, "Move Layer Up"),
            (
                Icon::MoveLayerDown,
                Command::MoveLayerDown,
                "Move Layer Down",
            ),
            (
                Icon::LayerProperties,
                Command::LayerProperties,
                "Layer Properties",
            ),
        ] {
            let tip = match command.shortcut_text() {
                Some(s) => format!("{tip} ({s})"),
                None => tip.to_owned(),
            };
            let button = ui
                .add_enabled(
                    enabled(command),
                    egui::Button::image(icons.image(icon)).frame(false),
                )
                .on_hover_text(tip);
            if button.clicked() {
                // Ctrl+click on the arrows moves to the top/bottom (paint.net).
                let ctrl = ui.input(|i| i.modifiers.ctrl);
                let c = match (command, ctrl) {
                    (Command::MoveLayerUp, true) => Command::MoveLayerToTop,
                    (Command::MoveLayerDown, true) => Command::MoveLayerToBottom,
                    (c, _) => c,
                };
                actions.push(LayersAction::Command(c));
            }
        }
    });
    actions
}

/// History window. Phase 3 fills the list; until then it shows the document's first step.
pub fn history(
    ui: &mut egui::Ui,
    tab: Option<&DocumentTab>,
    icons: &Icons,
    palette: &Palette,
    enabled: &dyn Fn(Command) -> bool,
) -> Option<Command> {
    let mut picked = None;
    let bar_height = 24.0;
    let list_height = (ui.available_height() - bar_height).max(40.0);
    egui::Frame::new().fill(palette.list).show(ui, |ui| {
        ui.set_min_height(list_height);
        ui.set_max_height(list_height);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let Some(tab) = tab else { return };
                let (label, icon) = if tab.path.is_some() {
                    ("Open Image", Icon::Open)
                } else {
                    ("New Image", Icon::New)
                };
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                ui.painter().rect(
                    rect,
                    0.0,
                    palette.selection,
                    egui::Stroke::new(1.0, palette.selection_stroke),
                    egui::StrokeKind::Inside,
                );
                ui.painter().image(
                    icons.texture(icon).id(),
                    Rect::from_min_size(rect.min + vec2(3.0, 2.0), vec2(16.0, 16.0)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                ui.painter().text(
                    pos2(rect.min.x + 24.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(12.0),
                    palette.text,
                );
            });
    });
    ui.horizontal(|ui| {
        for (icon, command) in [(Icon::Undo, Command::Undo), (Icon::Redo, Command::Redo)] {
            let tip = format!(
                "{:?} ({})",
                command,
                command.shortcut_text().unwrap_or_default()
            );
            if ui
                .add_enabled(
                    enabled(command),
                    egui::Button::image(icons.image(icon)).frame(false),
                )
                .on_hover_text(tip)
                .clicked()
            {
                picked = Some(command);
            }
        }
    });
    picked
}
