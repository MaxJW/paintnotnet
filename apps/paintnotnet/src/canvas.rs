//! The editing window: draws the active document on the GPU and handles view input
//! (wheel scroll, Ctrl+wheel zoom, Space/middle-drag pan, scrollbars).

use eframe::egui::{self, PointerButton, Rect, Sense, Vec2, pos2, vec2};
use eframe::egui_wgpu;
use pnn_core::TileCoord;
use pnn_render::{CanvasCallback, CanvasRenderer, View, ViewUniforms, display_tile, view};
use rayon::prelude::*;

use crate::workspace::DocumentTab;

/// Scrollbar thickness in points.
const SCROLLBAR: f32 = 14.0;
/// Space+arrow pan step in screen px (×10 with Ctrl). Not documented by paint.net; tune by feel.
pub const ARROW_PAN_STEP: f64 = 16.0;

#[derive(Default)]
pub struct CanvasState {
    pub pixel_grid: bool,
    /// Image coordinate under the pointer, for the status bar.
    pub cursor: Option<[f64; 2]>,
    /// Last laid-out viewport size in physical px (keyboard scroll/zoom commands use it).
    pub viewport: [f64; 2],
    /// Where the canvas was last laid out, in points (tests, overlays).
    pub rect: Option<Rect>,
    zoom_wheel: f32,
}

/// Per-tab GPU bookkeeping: which composite tile revisions are on the GPU.
#[derive(Default)]
pub struct GpuSync {
    revisions: Vec<u64>,
}

pub fn surround_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_gray(48)
    } else {
        egui::Color32::from_gray(171)
    }
}

pub fn show(
    ui: &mut egui::Ui,
    tab: &mut DocumentTab,
    state: &mut CanvasState,
    gpu: Option<&egui_wgpu::RenderState>,
) {
    let ppp = ui.ctx().pixels_per_point() as f64;
    let full = snap_to_pixels(ui.available_rect_before_wrap(), ppp as f32);
    ui.allocate_rect(full, Sense::hover());
    let img = tab.image_size();
    let px = |r: Rect| [r.width() as f64 * ppp, r.height() as f64 * ppp];

    // Lay out: scrollbars appear only when the content overflows on that axis.
    let view = tab.view.get_or_insert_with(|| View::fitted(img, px(full)));
    let mut canvas_rect = full;
    for _ in 0..2 {
        view.update(img, px(canvas_rect));
        let vp = px(canvas_rect);
        let (h, v) = (
            img[0] * view.zoom > vp[0] + 0.5,
            img[1] * view.zoom > vp[1] + 0.5,
        );
        canvas_rect = Rect::from_min_max(
            full.min,
            pos2(
                full.max.x - if v { SCROLLBAR } else { 0.0 },
                full.max.y - if h { SCROLLBAR } else { 0.0 },
            ),
        );
    }
    let vp = px(canvas_rect);
    view.update(img, vp);
    state.viewport = vp;
    state.rect = Some(canvas_rect);

    let response = ui.interact(canvas_rect, ui.id().with("canvas"), Sense::click_and_drag());
    let space_held =
        ui.input(|i| i.key_down(egui::Key::Space)) && !ui.ctx().egui_wants_keyboard_input();

    // Pan: middle-drag anywhere, or left-drag while Space is held.
    let panning = response.dragged_by(PointerButton::Middle)
        || (space_held && response.dragged_by(PointerButton::Primary));
    if panning {
        let d = response.drag_delta() * ppp as f32;
        view.scroll_by([-d.x as f64, -d.y as f64], img, vp);
    }
    if panning {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if space_held && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // Wheel: scroll (Shift = horizontal, handled by egui); Ctrl = step through zoom presets
    // about the pointer.
    if response.hovered() {
        let (scroll, wheel): (Vec2, Vec<(egui::MouseWheelUnit, Vec2, egui::Modifiers)>) =
            ui.input(|i| {
                let wheel = i
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel {
                            unit,
                            delta,
                            modifiers,
                            ..
                        } => Some((*unit, *delta, *modifiers)),
                        _ => None,
                    })
                    .collect();
                (i.smooth_scroll_delta, wheel)
            });
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .unwrap_or(canvas_rect.center());
        let anchor = [
            ((pointer.x - canvas_rect.min.x) as f64) * ppp,
            ((pointer.y - canvas_rect.min.y) as f64) * ppp,
        ];
        for (unit, delta, modifiers) in wheel {
            if !modifiers.ctrl {
                continue;
            }
            state.zoom_wheel += match unit {
                egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                egui::MouseWheelUnit::Point => delta.y / 50.0,
            };
            while state.zoom_wheel >= 1.0 {
                state.zoom_wheel -= 1.0;
                view.zoom_about(view::next_zoom_in(view.zoom), anchor, img, vp);
            }
            while state.zoom_wheel <= -1.0 {
                state.zoom_wheel += 1.0;
                view.zoom_about(view::next_zoom_out(view.zoom), anchor, img, vp);
            }
        }
        if scroll != Vec2::ZERO && !ui.input(|i| i.modifiers.ctrl) {
            view.scroll_by(
                [-(scroll.x as f64) * ppp, -(scroll.y as f64) * ppp],
                img,
                vp,
            );
        }
    }

    state.cursor = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| canvas_rect.contains(*p))
        .map(|p| {
            view.screen_to_image(
                [
                    ((p.x - canvas_rect.min.x) as f64) * ppp,
                    ((p.y - canvas_rect.min.y) as f64) * ppp,
                ],
                vp,
            )
        });

    let view = *view;
    scrollbars(ui, full, canvas_rect, tab, ppp);

    ui.painter()
        .rect_filled(canvas_rect, 0.0, surround_color(ui));
    if let Some(gpu) = gpu {
        sync_gpu(gpu, tab);
        let origin = view.image_to_screen([0.0, 0.0], vp);
        let uniforms = ViewUniforms {
            origin: [origin[0] as f32, origin[1] as f32],
            zoom: view.zoom as f32,
            ppp: ppp as f32,
            viewport: [vp[0] as f32, vp[1] as f32],
            image_size: [img[0] as f32, img[1] as f32],
            grid: if state.pixel_grid { 1.0 } else { 0.0 },
            ..Default::default()
        };
        ui.painter()
            .with_clip_rect(canvas_rect)
            .add(egui_wgpu::Callback::new_paint_callback(
                canvas_rect,
                CanvasCallback {
                    key: tab.key,
                    uniforms,
                },
            ));
    }
}

/// Upload composite tiles whose revision differs from what the GPU holds.
fn sync_gpu(gpu: &egui_wgpu::RenderState, tab: &mut DocumentTab) {
    tab.compositor.update(&tab.doc);
    let (w, h) = (tab.doc.width(), tab.doc.height());
    let recreated = {
        let mut renderer = gpu.renderer.write();
        let Some(canvas) = renderer.callback_resources.get_mut::<CanvasRenderer>() else {
            return;
        };
        canvas.ensure_canvas(&gpu.device, tab.key, w, h)
    };
    let (tx, ty) = (tab.compositor.tiles_x(), tab.compositor.tiles_y());
    let sync = &mut tab.gpu;
    if recreated || sync.revisions.len() != (tx * ty) as usize {
        sync.revisions = vec![0; (tx * ty) as usize];
    }
    let stale: Vec<TileCoord> = (0..ty)
        .flat_map(|ty| (0..tx).map(move |tx| TileCoord { tx, ty }))
        .filter(|&c| tab.compositor.tile_revision(c) != sync.revisions[(c.ty * tx + c.tx) as usize])
        .collect();
    if stale.is_empty() {
        return;
    }
    let compositor = &tab.compositor;
    let sync = &mut tab.gpu;
    let tiles: Vec<_> = stale
        .par_iter()
        .map(|&c| display_tile(compositor.tile(c).map(|t| &**t)))
        .collect();
    let renderer = gpu.renderer.read();
    if let Some(canvas) = renderer.callback_resources.get::<CanvasRenderer>() {
        for (c, tile) in stale.iter().zip(&tiles) {
            canvas.upload(&gpu.queue, tab.key, *c, tile);
            sync.revisions[(c.ty * tx + c.tx) as usize] = compositor.tile_revision(*c);
        }
    }
}

fn scrollbars(ui: &mut egui::Ui, full: Rect, canvas: Rect, tab: &mut DocumentTab, ppp: f64) {
    let img = tab.image_size();
    let vp = [canvas.width() as f64 * ppp, canvas.height() as f64 * ppp];
    for axis in 0..2 {
        let Some(view) = tab.view.as_mut() else {
            return;
        };
        let Some((start, len)) = view.scrollbar(axis, img, vp) else {
            continue;
        };
        let track = if axis == 0 {
            Rect::from_min_max(
                pos2(canvas.min.x, canvas.max.y),
                pos2(canvas.max.x, full.max.y),
            )
        } else {
            Rect::from_min_max(
                pos2(canvas.max.x, canvas.min.y),
                pos2(full.max.x, canvas.max.y),
            )
        };
        let track_len = if axis == 0 {
            track.width()
        } else {
            track.height()
        } as f64;
        let thumb_len = (len * track_len).max(20.0);
        let thumb_start = start * (track_len - thumb_len) / (1.0 - len).max(1e-9);
        let thumb = if axis == 0 {
            Rect::from_min_size(
                pos2(track.min.x + thumb_start as f32, track.min.y + 2.0),
                vec2(thumb_len as f32, track.height() - 4.0),
            )
        } else {
            Rect::from_min_size(
                pos2(track.min.x + 2.0, track.min.y + thumb_start as f32),
                vec2(track.width() - 4.0, thumb_len as f32),
            )
        };
        let response = ui.interact(
            track,
            ui.id().with(("scrollbar", axis)),
            Sense::click_and_drag(),
        );
        let content = img[axis] * view.zoom;
        let px_per_track_pt = (content - vp[axis]) / (track_len - thumb_len).max(1.0);
        if response.dragged() {
            let d = if axis == 0 {
                response.drag_delta().x
            } else {
                response.drag_delta().y
            } as f64;
            let mut delta = [0.0; 2];
            delta[axis] = d * px_per_track_pt;
            view.scroll_by(delta, img, vp);
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
            && !thumb.contains(p)
        {
            let before = if axis == 0 {
                p.x < thumb.min.x
            } else {
                p.y < thumb.min.y
            };
            let mut delta = [0.0; 2];
            delta[axis] = if before { -vp[axis] } else { vp[axis] };
            view.scroll_by(delta, img, vp);
        }
        let visuals = ui.visuals();
        ui.painter()
            .rect_filled(track, 0.0, visuals.extreme_bg_color);
        let fill = if response.dragged() || response.hovered() {
            visuals.widgets.hovered.bg_fill
        } else {
            visuals.widgets.inactive.bg_fill
        };
        ui.painter().rect_filled(thumb, 3.0, fill);
    }
    // Corner square where both scrollbars meet.
    if canvas.max.x < full.max.x && canvas.max.y < full.max.y {
        ui.painter().rect_filled(
            Rect::from_min_max(canvas.max, full.max),
            0.0,
            ui.visuals().extreme_bg_color,
        );
    }
}

fn snap_to_pixels(r: Rect, ppp: f32) -> Rect {
    let s = |v: f32| (v * ppp).round() / ppp;
    Rect::from_min_max(pos2(s(r.min.x), s(r.min.y)), pos2(s(r.max.x), s(r.max.y)))
}
