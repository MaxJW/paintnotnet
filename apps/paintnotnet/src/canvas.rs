//! The editing window: draws the active document on the GPU and handles view input
//! (wheel scroll, Ctrl+wheel zoom, Space/middle-drag pan, scrollbars).

use eframe::egui::{self, PointerButton, Rect, Sense, Vec2, pos2, vec2};
use eframe::egui_wgpu;
use pnn_core::TileCoord;
use pnn_render::{CanvasCallback, CanvasRenderer, View, ViewUniforms, display_tile, view};
use rayon::prelude::*;

use crate::commands::Units;
use crate::workspace::DocumentTab;

/// Ruler thickness in points.
const RULER: f32 = 18.0;

/// Scrollbar thickness in points.
const SCROLLBAR: f32 = 14.0;
/// Space+arrow pan step in screen px (×10 with Ctrl). Not documented by paint.net; tune by feel.
pub const ARROW_PAN_STEP: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolButton {
    Primary,
    Secondary,
}

/// Pointer input for the active tool, in image coordinates (fractional pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolEvent {
    Down {
        pos: [f64; 2],
        button: ToolButton,
        modifiers: egui::Modifiers,
    },
    Move {
        pos: [f64; 2],
    },
    Up {
        pos: [f64; 2],
    },
}

#[derive(Default)]
pub struct CanvasState {
    /// Tool input gathered this frame (taken by the app).
    pub events: Vec<ToolEvent>,
    /// Mouse button that started the current tool drag.
    tool_button: Option<ToolButton>,
    /// Cursor for the active tool over the canvas (set by the app).
    pub tool_cursor: Option<egui::CursorIcon>,
    /// Feather preview: (mask identity, coverage) to tint outside of, set by the app.
    pub overlay: Option<(u64, std::sync::Arc<pnn_core::Mask>)>,
    /// Brush outline diameter (image px) to draw around the pointer.
    pub brush_outline: Option<f32>,
    /// Move tool nubs to draw (image coordinates).
    pub handles: Option<[[f64; 2]; 8]>,
    pub pixel_grid: bool,
    /// View > Rulers, drawn in these units.
    pub rulers: Option<(Units, f64)>,
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
    /// Which mask the overlay textures hold (0 = none uploaded).
    overlay_revision: u64,
    /// Identity of each uploaded overlay tile (0 = empty), to upload only what changed.
    overlay_tiles: Vec<usize>,
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
    let outer = snap_to_pixels(ui.available_rect_before_wrap(), ppp as f32);
    ui.allocate_rect(outer, Sense::hover());
    let full = if state.rulers.is_some() {
        Rect::from_min_max(outer.min + vec2(RULER, RULER), outer.max)
    } else {
        outer
    };
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
    } else if let Some(cursor) = state.tool_cursor
        && (response.hovered() || state.tool_button.is_some())
    {
        ui.ctx().set_cursor_icon(cursor);
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

    // Tool input from raw pointer events (every intermediate point of a fast stroke).
    if !panning && !space_held {
        let to_img = |p: egui::Pos2| {
            view.screen_to_image(
                [
                    ((p.x - canvas_rect.min.x) as f64) * ppp,
                    ((p.y - canvas_rect.min.y) as f64) * ppp,
                ],
                vp,
            )
        };
        let over_canvas = response.contains_pointer();
        let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let b = match button {
                        PointerButton::Primary => ToolButton::Primary,
                        PointerButton::Secondary => ToolButton::Secondary,
                        _ => continue,
                    };
                    if pressed
                        && state.tool_button.is_none()
                        && over_canvas
                        && canvas_rect.contains(pos)
                    {
                        state.tool_button = Some(b);
                        state.events.push(ToolEvent::Down {
                            pos: to_img(pos),
                            button: b,
                            modifiers,
                        });
                    } else if !pressed && state.tool_button == Some(b) {
                        state.tool_button = None;
                        state.events.push(ToolEvent::Up { pos: to_img(pos) });
                    }
                }
                egui::Event::PointerMoved(pos) if state.tool_button.is_some() => {
                    state.events.push(ToolEvent::Move { pos: to_img(pos) });
                }
                _ => {}
            }
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
    if let Some((units, dpi)) = state.rulers {
        rulers(
            ui,
            outer,
            canvas_rect,
            &view,
            vp,
            ppp,
            units,
            dpi,
            state.cursor,
        );
    }

    ui.painter()
        .rect_filled(canvas_rect, 0.0, surround_color(ui));
    if let Some(gpu) = gpu {
        sync_gpu(gpu, tab);
        if let Some((rev, mask)) = &state.overlay {
            sync_overlay(gpu, tab, *rev, mask);
        }
        let origin = view.image_to_screen([0.0, 0.0], vp);
        let uniforms = ViewUniforms {
            origin: [origin[0] as f32, origin[1] as f32],
            zoom: view.zoom as f32,
            ppp: ppp as f32,
            viewport: [vp[0] as f32, vp[1] as f32],
            image_size: [img[0] as f32, img[1] as f32],
            grid: if state.pixel_grid { 1.0 } else { 0.0 },
            overlay: if state.overlay.is_some() { 1.0 } else { 0.0 },
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
    if !tab.doc.selection().is_empty() {
        let contours = tab.ant_contours(ui.ctx());
        marching_ants(ui, &contours, &view, canvas_rect, vp, ppp);
    }
    if let Some(handles) = state.handles {
        let painter = ui.painter().with_clip_rect(canvas_rect);
        for h in handles {
            let sp = view.image_to_screen(h, vp);
            let c = egui::pos2(
                canvas_rect.min.x + (sp[0] / ppp) as f32,
                canvas_rect.min.y + (sp[1] / ppp) as f32,
            );
            let r = Rect::from_center_size(c, vec2(7.0, 7.0));
            let stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
            painter.rect(
                r,
                0.0,
                egui::Color32::WHITE,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
    }
    // The brush outline around the pointer (paint.net's `BrushPreviewRenderer`).
    if let (Some(d), Some(cur)) = (state.brush_outline, state.cursor)
        && ui.rect_contains_pointer(canvas_rect)
    {
        let sp = view.image_to_screen(cur, vp);
        let c = egui::pos2(
            canvas_rect.min.x + (sp[0] / ppp) as f32,
            canvas_rect.min.y + (sp[1] / ppp) as f32,
        );
        let r = (d as f64 / 2.0 * view.zoom / ppp) as f32;
        if r >= 2.0 {
            let painter = ui.painter().with_clip_rect(canvas_rect);
            painter.circle_stroke(c, r + 1.0, egui::Stroke::new(1.0, egui::Color32::WHITE));
            painter.circle_stroke(c, r, egui::Stroke::new(1.0, egui::Color32::BLACK));
        }
    }
}

/// Upload the feather-preview mask when it changed since the last upload.
fn sync_overlay(
    gpu: &egui_wgpu::RenderState,
    tab: &mut DocumentTab,
    rev: u64,
    mask: &pnn_core::Mask,
) {
    if tab.gpu.overlay_revision == rev {
        return;
    }
    let renderer = gpu.renderer.read();
    let Some(canvas) = renderer.callback_resources.get::<CanvasRenderer>() else {
        return;
    };
    let n = (mask.tiles_x() * mask.tiles_y()) as usize;
    if tab.gpu.overlay_tiles.len() != n || tab.gpu.overlay_revision == 0 {
        tab.gpu.overlay_tiles = vec![usize::MAX; n];
    }
    let tiles_x = mask.tiles_x();
    for c in mask.tile_coords() {
        let tile = mask.tile(c);
        let id = tile.map_or(0, |t| std::sync::Arc::as_ptr(t) as usize);
        let slot = &mut tab.gpu.overlay_tiles[(c.ty * tiles_x + c.tx) as usize];
        if *slot != id {
            canvas.upload_overlay(&gpu.queue, tab.key, c, tile.map(|t| t.pixels()));
            *slot = id;
        }
    }
    tab.gpu.overlay_revision = rev;
}

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
        sync.overlay_revision = 0;
        sync.overlay_tiles.clear();
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

/// Ruler tick step in image pixels: the smallest 1/2/5×10^k unit step at least
/// `min_screen_px` apart on screen.
fn ruler_step(zoom: f64, units_per_px: f64, min_screen_px: f64) -> f64 {
    let min_units = min_screen_px / zoom * units_per_px;
    let mut step = 10f64.powf(min_units.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if step * m >= min_units {
            step *= m;
            break;
        }
    }
    step / units_per_px
}

#[allow(clippy::too_many_arguments)]
fn rulers(
    ui: &mut egui::Ui,
    outer: Rect,
    canvas: Rect,
    view: &View,
    vp: [f64; 2],
    ppp: f64,
    units: Units,
    dpi: f64,
    cursor: Option<[f64; 2]>,
) {
    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    let bg = visuals.extreme_bg_color;
    let ink = visuals.text_color();
    let top = Rect::from_min_max(
        pos2(canvas.min.x, outer.min.y),
        pos2(canvas.max.x, outer.min.y + RULER),
    );
    let left = Rect::from_min_max(
        pos2(outer.min.x, canvas.min.y),
        pos2(outer.min.x + RULER, canvas.max.y),
    );
    painter.rect_filled(
        Rect::from_min_max(outer.min, pos2(outer.max.x, outer.min.y + RULER)),
        0.0,
        bg,
    );
    painter.rect_filled(
        Rect::from_min_max(outer.min, pos2(outer.min.x + RULER, outer.max.y)),
        0.0,
        bg,
    );
    let units_per_px = match units {
        Units::Pixels => 1.0,
        Units::Inches => 1.0 / dpi,
        Units::Centimeters => 2.54 / dpi,
    };
    let major = ruler_step(view.zoom, units_per_px, 60.0 * ppp);
    let minor = major / 10.0;
    let font = egui::FontId::proportional(9.0);
    for axis in 0..2 {
        let rect = if axis == 0 { top } else { left };
        let painter = ui.painter_at(rect);
        let start = view.screen_to_image([0.0, 0.0], vp)[axis];
        let end = view.screen_to_image(vp, vp)[axis];
        let first = (start / minor).floor() as i64;
        let last = (end / minor).ceil() as i64;
        if last - first > 4000 {
            continue;
        }
        for i in first..=last {
            let img = i as f64 * minor;
            let s = view.image_to_screen([img, img], vp)[axis] / ppp;
            let is_major = i.rem_euclid(10) == 0;
            let len = if is_major {
                RULER
            } else if i.rem_euclid(5) == 0 {
                RULER * 0.5
            } else {
                RULER * 0.25
            };
            let (a, b) = if axis == 0 {
                let x = canvas.min.x + s as f32;
                (pos2(x, rect.max.y - len), pos2(x, rect.max.y))
            } else {
                let y = canvas.min.y + s as f32;
                (pos2(rect.max.x - len, y), pos2(rect.max.x, y))
            };
            painter.line_segment([a, b], egui::Stroke::new(1.0, ink.gamma_multiply(0.7)));
            if is_major {
                let value = img * units_per_px;
                let text = if units == Units::Pixels {
                    format!("{}", value.round() as i64)
                } else {
                    format!("{value:.2}")
                };
                let at = if axis == 0 {
                    pos2(a.x + 2.0, rect.min.y + 1.0)
                } else {
                    pos2(rect.min.x + 1.0, a.y + 2.0)
                };
                painter.text(at, egui::Align2::LEFT_TOP, text, font.clone(), ink);
            }
        }
        if let Some(c) = cursor {
            let s = view.image_to_screen(c, vp)[axis] / ppp;
            let (a, b) = if axis == 0 {
                let x = canvas.min.x + s as f32;
                (pos2(x, rect.min.y), pos2(x, rect.max.y))
            } else {
                let y = canvas.min.y + s as f32;
                (pos2(rect.min.x, y), pos2(rect.max.x, y))
            };
            painter.line_segment(
                [a, b],
                egui::Stroke::new(1.0, visuals.selection.stroke.color),
            );
        }
    }
}

/// paint.net's marching ants: a white line with animated black dashes along the selection's
/// polygon edges. Only segments near the visible area are drawn.
fn marching_ants(
    ui: &mut egui::Ui,
    contours: &[pnn_core::selection::Contour],
    view: &View,
    canvas: Rect,
    vp: [f64; 2],
    ppp: f64,
) {
    let painter = ui.painter().with_clip_rect(canvas);
    let time = ui.input(|i| i.time);
    let offset = ((time * 12.0) % 8.0) as f32;
    let visible = canvas.expand(4.0);
    let to_screen = |p: [f64; 2]| {
        let s = view.image_to_screen(p, vp);
        egui::pos2(
            canvas.min.x + (s[0] / ppp) as f32,
            canvas.min.y + (s[1] / ppp) as f32,
        )
    };
    let mut segments = 0usize;
    for contour in contours {
        let mut path: Vec<egui::Pos2> = contour.iter().map(|&p| to_screen(p)).collect();
        if path.len() < 2 {
            continue;
        }
        path.push(path[0]);
        // Skip contours entirely off-screen.
        let bb = Rect::from_points(&path);
        if !bb.intersects(visible) {
            continue;
        }
        segments += path.len();
        painter.add(egui::Shape::line(
            path.clone(),
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        ));
        if segments < 200_000 {
            painter.extend(egui::Shape::dashed_line_with_offset(
                &path,
                egui::Stroke::new(1.0, egui::Color32::BLACK),
                &[4.0],
                &[4.0],
                offset,
            ));
        }
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(80));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruler_steps_are_1_2_5() {
        assert_eq!(ruler_step(1.0, 1.0, 60.0), 100.0);
        assert_eq!(ruler_step(2.0, 1.0, 60.0), 50.0);
        assert_eq!(ruler_step(8.0, 1.0, 60.0), 10.0);
        // inches at 96 dpi, 100% zoom: 60 px ≈ 0.625 in → 1 in = 96 px
        assert_eq!(ruler_step(1.0, 1.0 / 96.0, 60.0), 96.0);
    }
}
