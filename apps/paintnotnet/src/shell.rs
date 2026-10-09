//! The image list: one live thumbnail per open image, like paint.net's tabs.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, Sense, TextureOptions, vec2};
use pnn_core::{ColorBgra, TILE_SIZE, TileCoord};

use crate::workspace::{DocumentTab, Workspace};

/// Longest side of an image-list thumbnail, in points.
const THUMB: f32 = 48.0;
const THUMB_THROTTLE: Duration = Duration::from_millis(250);

pub enum ImageListAction {
    Activate(usize),
    Close(u64),
    Move { from: usize, to: usize },
}

pub fn image_list(ui: &mut egui::Ui, ws: &Workspace) -> Vec<ImageListAction> {
    let mut actions = Vec::new();
    egui::ScrollArea::horizontal()
        .id_salt("image_list")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, tab) in ws.tabs().iter().enumerate() {
                    let selected = ws.active_index() == Some(i);
                    let id = egui::Id::new(("image_list_item", tab.key));
                    let item = ui
                        .dnd_drag_source(id, i, |ui| thumbnail_item(ui, tab, selected))
                        .response;
                    let item = item.on_hover_text(tab.title());
                    if item.clicked() {
                        actions.push(ImageListAction::Activate(i));
                    }
                    if item.middle_clicked() {
                        actions.push(ImageListAction::Close(tab.key));
                    }
                    if let Some(from) = item.dnd_release_payload::<usize>() {
                        actions.push(ImageListAction::Move { from: *from, to: i });
                    }
                    // Close button, shown while hovering.
                    if item.hovered() || ui.rect_contains_pointer(item.rect) {
                        let r = egui::Rect::from_min_size(
                            item.rect.right_top() + vec2(-16.0, 2.0),
                            vec2(14.0, 14.0),
                        );
                        if ui
                            .put(r, egui::Button::new("×").small().frame(false))
                            .clicked()
                        {
                            actions.push(ImageListAction::Close(tab.key));
                        }
                    }
                }
            });
        });
    actions
}

fn thumbnail_item(ui: &mut egui::Ui, tab: &DocumentTab, selected: bool) {
    let frame = egui::Frame::new()
        .inner_margin(4.0)
        .corner_radius(3.0)
        .fill(if selected {
            ui.visuals().selection.bg_fill
        } else {
            Color32::TRANSPARENT
        });
    frame.show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(THUMB, THUMB), Sense::hover());
        if let Some(tex) = &tab.thumbnail {
            // The texture is in physical pixels; show it at 1:1 device pixels.
            let shown = egui::Rect::from_center_size(
                rect.center(),
                tex.size_vec2() / ui.ctx().pixels_per_point(),
            );
            ui.painter().image(
                tex.id(),
                shown,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            ui.painter().rect_stroke(
                shown,
                0.0,
                (1.0, Color32::from_gray(90)),
                egui::StrokeKind::Outside,
            );
        }
        if tab.is_dirty() {
            ui.painter().text(
                rect.left_top(),
                egui::Align2::LEFT_TOP,
                "*",
                egui::FontId::proportional(16.0),
                ui.visuals().strong_text_color(),
            );
        }
    });
}

/// Refresh a tab's thumbnail if its document changed (throttled while it keeps changing).
pub fn update_thumbnail(ctx: &egui::Context, tab: &mut DocumentTab, now: Instant) {
    if tab.thumbnail.is_some() && tab.thumbnail_revision == tab.doc.revision() {
        return;
    }
    if let Some(t) = tab.thumbnail_updated
        && now.duration_since(t) < THUMB_THROTTLE
    {
        ctx.request_repaint_after(THUMB_THROTTLE);
        return;
    }
    tab.compositor.update(&tab.doc);
    let (w, h) = (tab.doc.width() as f32, tab.doc.height() as f32);
    let ppp = ctx.pixels_per_point();
    let scale = (THUMB * ppp / w.max(h)).min(1.0);
    let (tw, th) = (
        ((w * scale).round() as usize).max(1),
        ((h * scale).round() as usize).max(1),
    );
    let mut pixels = Vec::with_capacity(tw * th);
    for y in 0..th {
        for x in 0..tw {
            let sx = (((x as f32 + 0.5) / scale) as u32).min(tab.doc.width() - 1);
            let sy = (((y as f32 + 0.5) / scale) as u32).min(tab.doc.height() - 1);
            let c = TileCoord {
                tx: sx / TILE_SIZE,
                ty: sy / TILE_SIZE,
            };
            let p = tab.compositor.tile(c).map_or(ColorBgra::TRANSPARENT, |t| {
                t.get(sx % TILE_SIZE, sy % TILE_SIZE)
            });
            // Over a small version of paint.net's checkerboard.
            let v = if ((x ^ y) & 4) != 0 { 255 } else { 191 };
            let a = p.a as u32;
            let mix = |c: u8| ((c as u32 * a + v * (255 - a)) / 255) as u8;
            pixels.push(Color32::from_rgb(mix(p.r), mix(p.g), mix(p.b)));
        }
    }
    let image = ColorImage::new([tw, th], pixels);
    match &mut tab.thumbnail {
        Some(handle) => handle.set(image, TextureOptions::LINEAR),
        None => {
            tab.thumbnail =
                Some(ctx.load_texture(format!("thumb-{}", tab.key), image, TextureOptions::LINEAR))
        }
    }
    tab.thumbnail_revision = tab.doc.revision();
    tab.thumbnail_updated = Some(now);
}
