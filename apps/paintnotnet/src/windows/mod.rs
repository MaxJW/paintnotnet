//! Contents of the docked panels: the tool rail, Layers, History and (in `colors`) Colors.

pub mod colors;
pub mod oklch;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureOptions, pos2, vec2};
use pnn_core::{ColorBgra, Document, LayerId};
use pnn_tools::ToolKind;

use crate::commands::Command;
use crate::icons::{Icon, Icons};
use crate::theme::Palette;
use crate::workspace::DocumentTab;

/// The tool rail's groups, top to bottom: select, move, view, paint, pick, draw.
pub const RAIL_GROUPS: &[&[ToolKind]] = &[
    &[
        ToolKind::RectangleSelect,
        ToolKind::EllipseSelect,
        ToolKind::LassoSelect,
        ToolKind::MagicWand,
    ],
    &[ToolKind::MoveSelectedPixels, ToolKind::MoveSelection],
    &[ToolKind::Zoom, ToolKind::Pan],
    &[
        ToolKind::Paintbrush,
        ToolKind::Pencil,
        ToolKind::Eraser,
        ToolKind::PaintBucket,
        ToolKind::Gradient,
        ToolKind::CloneStamp,
        ToolKind::Recolor,
    ],
    &[ToolKind::ColorPicker],
    &[ToolKind::Text, ToolKind::LineCurve, ToolKind::Shapes],
];

/// Tool button edge and the gap between buttons.
const TOOL_BUTTON: f32 = 34.0;
const TOOL_GAP: f32 = 2.0;
/// Space taken by a separator between groups.
const GROUP_GAP: f32 = 11.0;

/// Height of the rail with `columns` columns.
fn rail_height(columns: usize) -> f32 {
    RAIL_GROUPS
        .iter()
        .map(|g| g.len().div_ceil(columns) as f32 * (TOOL_BUTTON + TOOL_GAP) - TOOL_GAP)
        .sum::<f32>()
        + GROUP_GAP * (RAIL_GROUPS.len() - 1) as f32
}

/// Tool rail: one column of tools in groups; two when `max_height` is too short for one.
pub fn tools(
    ui: &mut egui::Ui,
    current: ToolKind,
    icons: &Icons,
    palette: &Palette,
    max_height: f32,
) -> Option<ToolKind> {
    let mut picked = None;
    let columns = if rail_height(1) <= max_height { 1 } else { 2 };
    let width = columns as f32 * (TOOL_BUTTON + TOOL_GAP) - TOOL_GAP;
    ui.spacing_mut().item_spacing = vec2(TOOL_GAP, TOOL_GAP);
    ui.vertical(|ui| {
        for (g, group) in RAIL_GROUPS.iter().enumerate() {
            if g > 0 {
                let (r, _) =
                    ui.allocate_exact_size(vec2(width, GROUP_GAP - TOOL_GAP), Sense::hover());
                let y = r.center().y - TOOL_GAP / 2.0;
                ui.painter().hline(
                    (r.center().x - 12.0)..=(r.center().x + 12.0),
                    y,
                    egui::Stroke::new(1.0, palette.separator),
                );
            }
            for row in group.chunks(columns) {
                ui.horizontal(|ui| {
                    for &tool in row {
                        if tool_button(ui, tool, tool == current, icons, palette) {
                            picked = Some(tool);
                        }
                    }
                });
            }
        }
    });
    picked
}

fn tool_button(
    ui: &mut egui::Ui,
    tool: ToolKind,
    selected: bool,
    icons: &Icons,
    palette: &Palette,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(TOOL_BUTTON, TOOL_BUTTON), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, tool.name())
    });
    let radius = crate::theme::radius::CONTROL + 1;
    if selected {
        ui.painter().rect(
            rect,
            radius,
            palette.selection,
            egui::Stroke::new(1.0, palette.selection_stroke),
            egui::StrokeKind::Inside,
        );
    } else {
        let hover = hover_amount(ui, &response);
        if hover > 0.0 {
            ui.painter()
                .rect_filled(rect, radius, palette.hover.gamma_multiply(hover));
        }
    }
    let tint = if selected || response.hovered() {
        palette.text
    } else {
        palette.icon
    };
    ui.painter().image(
        icons.texture(Icon::for_tool(tool)).id(),
        Rect::from_center_size(rect.center(), vec2(18.0, 18.0)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        tint,
    );
    response.on_hover_text(tool.tooltip()).clicked()
}

pub enum LayersAction {
    Select(usize),
    /// Drag-and-drop reorder (layer indices, 0 = bottom).
    Move {
        from: usize,
        to: usize,
    },
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

/// Layers section body: thumbnails, names and visibility; top layer first, as paint.net
/// lists them. Drag a row onto another to move the layer there.
pub fn layers(
    ui: &mut egui::Ui,
    tab: Option<&mut DocumentTab>,
    palette: &Palette,
    now: Instant,
) -> Vec<LayersAction> {
    let mut actions = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
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
                    ui.allocate_exact_size(vec2(row_width, THUMB + 6.0), Sense::click_and_drag());
                let name = layer.name.clone();
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        true,
                        selected,
                        &name,
                    )
                });
                if response.drag_started() {
                    response.dnd_set_drag_payload(index);
                }
                if let Some(from) = response.dnd_release_payload::<usize>()
                    && *from != index
                {
                    actions.push(LayersAction::Move {
                        from: *from,
                        to: index,
                    });
                }
                if response.dnd_hover_payload::<usize>().is_some() {
                    ui.painter().hline(
                        rect.x_range(),
                        rect.min.y,
                        egui::Stroke::new(2.0, palette.accent),
                    );
                }
                row_background(ui, rect, selected, &response, palette);
                let check_rect = Rect::from_center_size(
                    pos2(rect.min.x + 15.0, rect.center().y),
                    vec2(18.0, 18.0),
                );
                let thumb_area = Rect::from_min_size(
                    pos2(check_rect.max.x + 8.0, rect.min.y + 3.0),
                    vec2(THUMB, THUMB),
                );
                if let Some((_, tex)) = thumbs.textures.iter().find(|(id, _)| *id == layer.id()) {
                    let size = tex.size_vec2() / ui.ctx().pixels_per_point();
                    let shown = Rect::from_center_size(thumb_area.center(), size);
                    egui::Image::new((tex.id(), shown.size()))
                        .corner_radius(4)
                        .paint_at(ui, shown);
                    ui.painter().rect_stroke(
                        shown,
                        4,
                        egui::Stroke::new(1.0, palette.window_border),
                        egui::StrokeKind::Outside,
                    );
                }
                let name_clip = Rect::from_min_max(
                    pos2(thumb_area.max.x + 10.0, rect.min.y),
                    pos2(rect.max.x - 8.0, rect.max.y),
                );
                let color = if layer.visible {
                    palette.text
                } else {
                    palette.faint_text
                };
                ui.painter_at(name_clip).text(
                    pos2(name_clip.min.x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &layer.name,
                    egui::FontId::proportional(crate::theme::text::BODY),
                    color,
                );
                let mut visible = layer.visible;
                if ui
                    .put(check_rect, egui::Checkbox::without_text(&mut visible))
                    .on_hover_text(if layer.visible {
                        "Hide layer"
                    } else {
                        "Show layer"
                    })
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
    actions
}

/// Layers section title buttons (right to left): properties, move down/up, merge down,
/// delete, duplicate, add.
pub fn layers_actions(
    ui: &mut egui::Ui,
    icons: &Icons,
    enabled: &dyn Fn(Command) -> bool,
) -> Option<Command> {
    let mut picked = None;
    for (icon, command, tip) in [
        (
            Icon::LayerProperties,
            Command::LayerProperties,
            "Layer Properties",
        ),
        (
            Icon::MoveLayerDown,
            Command::MoveLayerDown,
            "Move Layer Down",
        ),
        (Icon::MoveLayerUp, Command::MoveLayerUp, "Move Layer Up"),
        (
            Icon::MergeLayerDown,
            Command::MergeLayerDown,
            "Merge Layer Down",
        ),
        (Icon::DeleteLayer, Command::DeleteLayer, "Delete Layer"),
        (
            Icon::DuplicateLayer,
            Command::DuplicateLayer,
            "Duplicate Layer",
        ),
        (Icon::AddLayer, Command::AddNewLayer, "Add New Layer"),
    ] {
        if icon_button(ui, icons, icon, command, tip, enabled) {
            // Ctrl+click on the arrows moves to the top/bottom (paint.net).
            let ctrl = ui.input(|i| i.modifiers.ctrl);
            picked = Some(match (command, ctrl) {
                (Command::MoveLayerUp, true) => Command::MoveLayerToTop,
                (Command::MoveLayerDown, true) => Command::MoveLayerToBottom,
                (c, _) => c,
            });
        }
    }
    picked
}

/// A small frameless icon button for a command, with its shortcut in the tooltip.
fn icon_button(
    ui: &mut egui::Ui,
    icons: &Icons,
    icon: Icon,
    command: Command,
    tip: &str,
    enabled: &dyn Fn(Command) -> bool,
) -> bool {
    let tip = match command.shortcut_text() {
        Some(s) => format!("{tip} ({s})"),
        None => tip.to_owned(),
    };
    ui.add_enabled(
        enabled(command),
        egui::Button::image(icons.image(icon))
            .frame_when_inactive(false)
            .min_size(vec2(24.0, 24.0)),
    )
    .on_hover_text(&tip)
    .on_disabled_hover_text(&tip)
    .clicked()
}

/// How much of the hover highlight to show (eases in and out).
fn hover_amount(ui: &egui::Ui, response: &egui::Response) -> f32 {
    ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        crate::theme::motion::FAST,
    )
}

/// Rounded row highlight for the current item, or the hovered one.
fn row_background(
    ui: &egui::Ui,
    rect: Rect,
    selected: bool,
    response: &egui::Response,
    palette: &Palette,
) {
    let radius = crate::theme::radius::CONTROL + 1;
    if selected {
        ui.painter().rect(
            rect,
            radius,
            palette.selection,
            egui::Stroke::new(1.0, palette.selection_stroke),
            egui::StrokeKind::Inside,
        );
    } else {
        let hover = hover_amount(ui, response);
        if hover > 0.0 {
            ui.painter()
                .rect_filled(rect, radius, palette.hover.gamma_multiply(hover));
        }
    }
}

pub enum HistoryAction {
    Jump(usize),
    Command(Command),
}

/// History section body: every step with its icon; the current step highlighted, undone
/// steps faint (they can be redone); click a step to jump to it.
pub fn history(
    ui: &mut egui::Ui,
    tab: Option<&DocumentTab>,
    icons: &Icons,
    palette: &Palette,
) -> Option<HistoryAction> {
    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let Some(tab) = tab else { return };
            let current = tab.history.current();
            for (i, entry) in tab.history.entries().iter().enumerate() {
                let size = vec2(ui.available_width(), 28.0);
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                // Expose the step to accessibility (screen readers, UI tests).
                let selected = i == current;
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        true,
                        selected,
                        &entry.name,
                    )
                });
                row_background(ui, rect, selected, &response, palette);
                let undone = i > current;
                let tint = if undone {
                    palette.faint_text
                } else {
                    palette.icon
                };
                let icon_rect = Rect::from_center_size(
                    pos2(rect.min.x + 16.0, rect.center().y),
                    vec2(16.0, 16.0),
                );
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter()
                    .image(icons.texture(entry.tag).id(), icon_rect, uv, tint);
                let color = if undone {
                    palette.faint_text
                } else {
                    palette.text
                };
                let at = pos2(rect.min.x + 32.0, rect.center().y);
                let font = egui::FontId::proportional(crate::theme::text::BODY);
                ui.painter()
                    .text(at, egui::Align2::LEFT_CENTER, &entry.name, font, color);
                if response.clicked() && i != current {
                    action = Some(HistoryAction::Jump(i));
                }
            }
        });
    action
}

/// History section title buttons (right to left): redo, undo.
pub fn history_actions(
    ui: &mut egui::Ui,
    icons: &Icons,
    enabled: &dyn Fn(Command) -> bool,
) -> Option<Command> {
    let mut picked = None;
    for (icon, command, name) in [
        (Icon::Redo, Command::Redo, "Redo"),
        (Icon::Undo, Command::Undo, "Undo"),
    ] {
        if icon_button(ui, icons, icon, command, name, enabled) {
            picked = Some(command);
        }
    }
    picked
}
