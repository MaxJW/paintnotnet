//! The editing window: draws the active document on the GPU and handles view input
//! (wheel zoom or scroll, Space/middle-drag pan, scrollbars).

use eframe::egui::{self, PointerButton, Rect, Sense, Vec2, pos2, vec2};
use eframe::egui_wgpu;
use pnn_core::TileCoord;
use pnn_render::{CanvasCallback, CanvasRenderer, View, ViewUniforms, view};

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

/// How the pointer looks over the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolCursor {
    /// A system cursor (grab, move, text).
    System(egui::CursorIcon),
    /// Drawn by us (egui can't show cursor images): a crosshair on the hot spot and the
    /// tool's icon on a small chip beside it.
    Drawn(egui::TextureId),
}

/// The active tool's brush, for the Alt + right-drag resize gesture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushInfo {
    /// Width in image px.
    pub width: f32,
    /// 0–1, for tools with a hardness.
    pub hardness: Option<f32>,
}

/// Alt + right-drag on the canvas resizes the brush: how far the pointer has moved since the
/// button went down (screen points; right = bigger, up = harder).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushResize {
    pub delta: Vec2,
    /// First frame of the gesture.
    pub started: bool,
}

#[derive(Default)]
pub struct CanvasState {
    /// Tool input gathered this frame (taken by the app).
    pub events: Vec<ToolEvent>,
    /// Mouse button that started the current tool drag.
    tool_button: Option<ToolButton>,
    /// Cursor for the active tool over the canvas (set by the app).
    pub tool_cursor: Option<ToolCursor>,
    /// Feather preview: (mask identity, coverage) to tint outside of, set by the app.
    pub overlay: Option<(u64, std::sync::Arc<pnn_core::Mask>)>,
    /// Selections are antialiased (set by the app; picks which cached mask the GPU ants use).
    pub antialias: bool,
    /// While a selection is moved, rotated or scaled: its mask from before (identity, mask)
    /// and where to read it for an image point (the inverse transform), for the GPU ants
    /// (set by the app).
    pub ants_transform: Option<(u64, std::sync::Arc<pnn_core::Mask>, [f64; 6])>,
    /// Clone Stamp source marker (image coordinates).
    pub clone_source: Option<[f64; 2]>,
    /// Text caret (top, bottom) and move nub (image coordinates).
    pub text_handles: Option<crate::text_tool::TextHandles>,
    /// Line/Curve and Shapes handles (image coordinates).
    pub vector_handles: Option<crate::vector_tools::VectorHandles>,
    /// Gradient start/end nubs (image coordinates).
    pub gradient_nubs: Option<[[f64; 2]; 2]>,
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
    /// What the wheel does (Settings).
    pub wheel: crate::config::MouseWheel,
    /// The active tool's brush (set by the app; `None` for tools without a width).
    pub brush: Option<BrushInfo>,
    /// The Alt + right-drag resize in progress (read by the app).
    pub resize: Option<BrushResize>,
    /// Where the resize gesture started (screen).
    resize_anchor: Option<egui::Pos2>,
    zoom_wheel: f32,
}

/// Per-tab GPU bookkeeping: which document tile revisions the GPU has composited.
#[derive(Default)]
pub struct GpuSync {
    revisions: Vec<u64>,
    /// The live gradient, drawn by the GPU over its layer's pixels while the nubs move (the
    /// document gets it once they stop; see `fill_tools`).
    pub preview: Option<GradientPreview>,
    /// What the GPU last drew of `preview`: (layer, gradient, selection identity).
    shown_preview: Option<(
        pnn_core::LayerId,
        pnn_tools::gradient::GradientShader,
        usize,
    )>,
    /// Which mask the overlay textures hold (0 = none uploaded).
    overlay_revision: u64,
    /// Identity of each uploaded overlay tile (0 = empty), to upload only what changed.
    overlay_tiles: Vec<usize>,
    /// The same for the whole-image ants mask.
    ants_revision: u64,
    ants_tiles: Vec<usize>,
    /// Floating pixels being dragged (set by the app each frame).
    pub floating: Option<FloatingPreview>,
    /// What the GPU last drew of `floating`.
    shown_floating: Option<FloatingKey>,
}

/// Floating pixels being dragged, for the GPU compositor to draw over `layer`.
#[derive(Clone)]
pub struct FloatingPreview {
    pub layer: pnn_core::LayerId,
    pub serial: u64,
    pub pixels: pnn_core::Surface,
    pub rect: pnn_core::Rect,
    pub offset: [i32; 2],
    /// Rotated or scaled (else moved by `offset`).
    pub inverse: Option<pnn_render::FloatingTransform>,
}

/// A live gradient for the GPU compositor to draw on `layer`.
#[derive(Clone)]
pub struct GradientPreview {
    pub layer: pnn_core::LayerId,
    pub shader: pnn_tools::gradient::GradientShader,
    pub selection: Option<std::sync::Arc<pnn_core::Mask>>,
}

pub fn surround_color(ui: &egui::Ui) -> egui::Color32 {
    crate::theme::palette(ui.ctx()).surround
}

pub fn show(
    ui: &mut egui::Ui,
    tab: &mut DocumentTab,
    state: &mut CanvasState,
    free: Rect,
    gpu: Option<&egui_wgpu::RenderState>,
) {
    let ppp = ui.ctx().pixels_per_point() as f64;
    // The image is drawn over the whole area (it runs on under the panels' glass); fitting,
    // centring, rulers and scrollbars use the part the panels leave free.
    let area = snap_to_pixels(ui.available_rect_before_wrap(), ppp as f32);
    ui.allocate_rect(area, Sense::hover());
    let outer = snap_to_pixels(free.intersect(area), ppp as f32);
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

    let response = ui.interact(area, ui.id().with("canvas"), Sense::click_and_drag());
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
    } else if state.resize_anchor.is_some() {
        // The resize HUD stands in for the cursor.
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    } else if let Some(cursor) = state.tool_cursor
        && (response.hovered() || state.tool_button.is_some())
    {
        match cursor {
            ToolCursor::System(icon) => ui.ctx().set_cursor_icon(icon),
            ToolCursor::Drawn(texture) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    draw_cursor(ui.ctx(), p, texture);
                }
            }
        }
    }

    // Wheel. Zoom mode: a mouse wheel steps through the zoom presets about the pointer;
    // Shift+wheel and touchpad (pixel) scrolling scroll. Scroll mode (paint.net): the wheel
    // scrolls (Shift = horizontal, handled by egui) and Ctrl+wheel zooms.
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
        let zoom_mode = state.wheel == crate::config::MouseWheel::Zoom;
        let line_px = ui.ctx().options(|o| o.input_options.line_scroll_speed);
        let mut zoom_steps = 0.0;
        let mut raw_scroll = Vec2::ZERO;
        for (unit, delta, modifiers) in wheel {
            let mouse_wheel = unit != egui::MouseWheelUnit::Point;
            if modifiers.ctrl || (zoom_mode && mouse_wheel && !modifiers.shift) {
                zoom_steps += match unit {
                    egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / 50.0,
                };
            } else if zoom_mode {
                let mut d = if mouse_wheel { delta * line_px } else { delta };
                if modifiers.shift && d.x == 0.0 {
                    d = vec2(d.y, 0.0);
                }
                raw_scroll += d;
            }
        }
        state.zoom_wheel += zoom_steps;
        while state.zoom_wheel >= 1.0 {
            state.zoom_wheel -= 1.0;
            view.zoom_about(view::next_zoom_in(view.zoom), anchor, img, vp);
        }
        while state.zoom_wheel <= -1.0 {
            state.zoom_wheel += 1.0;
            view.zoom_about(view::next_zoom_out(view.zoom), anchor, img, vp);
        }
        // egui's smoothed delta already leaves out Ctrl+wheel (its zoom modifier).
        let scroll = if zoom_mode { raw_scroll } else { scroll };
        if scroll != Vec2::ZERO {
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
                    let on_canvas = over_canvas && area.contains(pos);
                    if pressed
                        && b == ToolButton::Secondary
                        && modifiers.alt
                        && state.brush.is_some()
                        && state.tool_button.is_none()
                        && on_canvas
                    {
                        // Alt + right-drag resizes the brush instead of painting.
                        state.resize_anchor = Some(pos);
                        state.resize = Some(BrushResize {
                            delta: Vec2::ZERO,
                            started: true,
                        });
                    } else if !pressed
                        && b == ToolButton::Secondary
                        && state.resize_anchor.is_some()
                    {
                        state.resize_anchor = None;
                        state.resize = None;
                    } else if pressed
                        && state.tool_button.is_none()
                        && state.resize_anchor.is_none()
                        && on_canvas
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
                egui::Event::PointerMoved(pos) if state.resize_anchor.is_some() => {
                    if let Some(anchor) = state.resize_anchor {
                        state.resize = Some(BrushResize {
                            delta: pos - anchor,
                            started: false,
                        });
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
        .filter(|p| area.contains(*p))
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
    ui.painter().rect_filled(area, 0.0, surround_color(ui));
    // A complex selection (the Magic Wand on a photo: tens of thousands of outlines) gets its
    // ants from the GPU, drawn from the selection's mask: as egui lines they cost tens of
    // milliseconds every frame and the whole app lagged.
    // Decided on the selection's own outline: the polygon ants of a feathered selection are
    // its outline offset, which takes seconds for a complex one (3.1 s for a Magic Wand
    // selection of a photo), so complex selections never compute them. The GPU ants draw the
    // mask's half-coverage line, which is the same contour.
    let points: usize = tab.doc.selection().contours().iter().map(Vec::len).sum();
    let gpu_ants = gpu.is_some()
        && std::env::var_os("PNN_GPU_ANTS").is_none_or(|v| v != "0")
        && points > GPU_ANTS_POINTS;
    let contours =
        (!tab.doc.selection().is_empty() && !gpu_ants).then(|| tab.ant_contours(ui.ctx()));
    let ants_offset = ((ui.input(|i| i.time) * 12.0) % 8.0).floor() as f32;
    if let Some(gpu) = gpu {
        let a = view.screen_to_image([0.0, 0.0], vp);
        let b = view.screen_to_image(vp, vp);
        if sync_gpu(gpu, tab, Some([a[0], a[1], b[0], b[1]])) {
            ui.ctx().request_repaint();
        }
        if let Some((rev, mask)) = &state.overlay {
            sync_overlay(gpu, tab, *rev, mask);
        }
    }
    // The ants mask: the moving selection's from before the move, shifted, or the current one.
    let mut ants_inv = [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0];
    let mut ants_full = false;
    if let Some(gpu) = gpu
        && gpu_ants
    {
        let full = {
            let renderer = gpu.renderer.read();
            renderer
                .callback_resources
                .get::<CanvasRenderer>()
                .is_some_and(|c| c.ants_full(tab.key))
        };
        let source = match &state.ants_transform {
            Some((id, mask, inv)) if full => {
                ants_inv = inv.map(|v| v as f32);
                Some((*id, mask.clone()))
            }
            _ => tab.selection_mask(state.antialias).map(|m| {
                (
                    tab.doc.selection_revision() * 2 + u64::from(state.antialias),
                    m,
                )
            }),
        };
        if let Some((rev, mask)) = source {
            if full {
                sync_ants(gpu, tab, rev, &mask);
                ants_full = true;
            } else if state.overlay.is_none() {
                sync_overlay(gpu, tab, rev, &mask);
            }
        }
    }
    if gpu.is_some() {
        // The view's coordinates start at the free area; the callback covers all of `area`.
        let origin = view.image_to_screen([0.0, 0.0], vp);
        let shift = (canvas_rect.min - area.min) * ppp as f32;
        let uniforms = ViewUniforms {
            origin: [origin[0] as f32 + shift.x, origin[1] as f32 + shift.y],
            zoom: view.zoom as f32,
            ppp: ppp as f32,
            viewport: [area.width() * ppp as f32, area.height() * ppp as f32],
            image_size: [img[0] as f32, img[1] as f32],
            grid: if state.pixel_grid { 1.0 } else { 0.0 },
            overlay: if state.overlay.is_some() { 1.0 } else { 0.0 },
            ants: if gpu_ants { 1.0 + ants_offset } else { 0.0 },
            ants_full: if ants_full { 1.0 } else { 0.0 },
            ants_inv0: [ants_inv[0], ants_inv[1], ants_inv[2], ants_inv[3]],
            ants_inv1: [ants_inv[4], ants_inv[5], 0.0, 0.0],
            ..Default::default()
        };
        ui.painter()
            .with_clip_rect(area)
            .add(egui_wgpu::Callback::new_paint_callback(
                area,
                CanvasCallback {
                    key: tab.key,
                    uniforms,
                    backdrop: Some(surround_color(ui).to_array().map(|v| v as f32 / 255.0)),
                },
            ));
        // The panels over the canvas show this blurred (see `glass`).
        crate::glass::set_backdrop(ui.ctx(), Some(area));
    }
    if gpu_ants {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(80));
    } else if let Some(contours) = contours {
        marching_ants(ui, &contours, &view, canvas_rect, area, vp, ppp);
    }
    if let Some(handles) = state.handles {
        let painter = ui.painter().with_clip_rect(area);
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
    if let Some(nubs) = state.gradient_nubs {
        // 3.36 draws the gradient's nubs as circles (`MoveNubShape.Circle`).
        let painter = ui.painter().with_clip_rect(area);
        let pts = nubs.map(|p| {
            let sp = view.image_to_screen(p, vp);
            egui::pos2(
                canvas_rect.min.x + (sp[0] / ppp) as f32,
                canvas_rect.min.y + (sp[1] / ppp) as f32,
            )
        });
        painter.line_segment(
            pts,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(128)),
        );
        for c in pts {
            painter.circle(
                c,
                4.5,
                egui::Color32::WHITE,
                egui::Stroke::new(1.0, egui::Color32::BLACK),
            );
        }
    }
    // The Clone Stamp's source: a second brush outline, pulsing as 3.36's `OnPulse`.
    if let (Some(d), Some(src)) = (state.brush_outline, state.clone_source) {
        let sp = view.image_to_screen(src, vp);
        let c = egui::pos2(
            canvas_rect.min.x + (sp[0] / ppp) as f32,
            canvas_rect.min.y + (sp[1] / ppp) as f32,
        );
        let r = ((d as f64 / 2.0 * view.zoom / ppp) as f32).max(3.0);
        let t = ui.input(|i| i.time);
        let alpha = (((t * 1000.0 / 300.0).sin() + 1.0) / 2.0 * 224.0 + 31.0) as u8;
        let painter = ui.painter().with_clip_rect(area);
        painter.circle_stroke(
            c,
            r + 1.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(alpha)),
        );
        painter.circle_stroke(
            c,
            r,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(alpha)),
        );
        ui.ctx().request_repaint();
    }
    if let Some(handles) = &state.vector_handles {
        // Pulsing nubs (paint.net 5 pulses them so they stand out) and the move icon.
        let painter = ui.painter().with_clip_rect(area);
        let to_screen = |p: [f64; 2]| {
            let sp = view.image_to_screen(p, vp);
            egui::pos2(
                canvas_rect.min.x + (sp[0] / ppp) as f32,
                canvas_rect.min.y + (sp[1] / ppp) as f32,
            )
        };
        let t = ui.input(|i| i.time);
        let a = (((t * std::f64::consts::TAU / 2.0).sin() + 1.0) / 2.0 * 160.0 + 95.0) as u8;
        if let Some(frame) = handles.frame {
            let pts: Vec<egui::Pos2> = frame.iter().map(|p| to_screen(*p)).collect();
            painter.add(egui::Shape::closed_line(
                pts,
                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(110)),
            ));
        }
        if let Some(p) = handles.pivot {
            // Rotation point: a circle with a cross.
            let c = to_screen(p);
            let s = egui::Stroke::new(1.0, egui::Color32::BLACK);
            painter.circle(c, 5.5, egui::Color32::from_white_alpha(a), s);
            painter.line_segment([c - vec2(5.5, 0.0), c + vec2(5.5, 0.0)], s);
            painter.line_segment([c - vec2(0.0, 5.5), c + vec2(0.0, 5.5)], s);
        }
        for &p in &handles.nubs {
            painter.circle(
                to_screen(p),
                4.5,
                egui::Color32::from_white_alpha(a),
                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(a)),
            );
        }
        let c = to_screen(handles.move_icon);
        let r = Rect::from_center_size(c, vec2(13.0, 13.0));
        painter.rect(
            r,
            1.0,
            egui::Color32::from_white_alpha(a),
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(a)),
            egui::StrokeKind::Inside,
        );
        let s = egui::Stroke::new(1.0, egui::Color32::BLACK);
        painter.line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], s);
        painter.line_segment([c - vec2(0.0, 4.0), c + vec2(0.0, 4.0)], s);
        ui.ctx().request_repaint();
    }
    if let Some((top, bottom, nub)) = state.text_handles {
        let painter = ui.painter().with_clip_rect(area);
        let to_screen = |p: [f64; 2]| {
            let sp = view.image_to_screen(p, vp);
            egui::pos2(
                canvas_rect.min.x + (sp[0] / ppp) as f32,
                canvas_rect.min.y + (sp[1] / ppp) as f32,
            )
        };
        let t = ui.input(|i| i.time);
        // Blinking caret.
        if (t * 2.0) as i64 % 2 == 0 {
            let s = egui::Stroke::new(1.0, egui::Color32::BLACK);
            painter.line_segment([to_screen(top), to_screen(bottom)], s);
        }
        let a = (((t * std::f64::consts::TAU / 2.0).sin() + 1.0) / 2.0 * 160.0 + 95.0) as u8;
        let c = to_screen(nub);
        let r = Rect::from_center_size(c, vec2(13.0, 13.0));
        painter.rect(
            r,
            1.0,
            egui::Color32::from_white_alpha(a),
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(a)),
            egui::StrokeKind::Inside,
        );
        let s = egui::Stroke::new(1.0, egui::Color32::BLACK);
        painter.line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], s);
        painter.line_segment([c - vec2(0.0, 4.0), c + vec2(0.0, 4.0)], s);
        ui.ctx().request_repaint();
    }
    // The brush outline around the pointer (paint.net's `BrushPreviewRenderer`).
    if let (Some(d), Some(cur)) = (state.brush_outline, state.cursor)
        && state.resize_anchor.is_none()
        && ui.rect_contains_pointer(area)
    {
        let sp = view.image_to_screen(cur, vp);
        let c = egui::pos2(
            canvas_rect.min.x + (sp[0] / ppp) as f32,
            canvas_rect.min.y + (sp[1] / ppp) as f32,
        );
        let r = (d as f64 / 2.0 * view.zoom / ppp) as f32;
        if r >= 2.0 {
            let painter = ui.painter().with_clip_rect(area);
            painter.circle_stroke(c, r + 1.0, egui::Stroke::new(1.0, egui::Color32::WHITE));
            painter.circle_stroke(c, r, egui::Stroke::new(1.0, egui::Color32::BLACK));
        }
    }
    if let (Some(anchor), Some(brush)) = (state.resize_anchor, state.brush) {
        resize_hud(ui, anchor, brush, view.zoom / ppp);
    }
    // Last, so they sit over the image (it now runs under them).
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
}

/// Alt + right-drag: the brush at its true size where the drag started, a ring where its hard
/// core ends, and the values beside it.
fn resize_hud(ui: &egui::Ui, anchor: egui::Pos2, brush: BrushInfo, scale: f64) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("brush_resize"),
    ));
    let r = ((brush.width as f64 / 2.0 * scale) as f32).max(1.0);
    let outline = |radius: f32, alpha: u8| {
        painter.circle_stroke(
            anchor,
            radius + 1.0,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(alpha)),
        );
        painter.circle_stroke(
            anchor,
            radius,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(alpha)),
        );
    };
    outline(r, 230);
    if let Some(h) = brush.hardness
        && h < 0.995
    {
        outline((r * h).max(0.5), 140);
    }
    painter.circle_filled(anchor, 2.0, egui::Color32::WHITE);
    let palette = crate::theme::palette(ui.ctx());
    let mut text = format!("{} px", fmt_width(brush.width));
    if let Some(h) = brush.hardness {
        text.push_str(&format!("  ·  Hardness {:.0}%", h * 100.0));
    }
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::monospace(crate::theme::text::BODY),
        palette.text,
    );
    let at = anchor + vec2(r.min(400.0) + 14.0, -galley.size().y / 2.0 - 6.0);
    let chip = Rect::from_min_size(at, galley.size() + vec2(20.0, 12.0));
    painter.rect(
        chip,
        crate::theme::radius::CONTROL,
        palette.window.gamma_multiply(0.92),
        egui::Stroke::new(1.0, palette.window_border),
        egui::StrokeKind::Inside,
    );
    painter.galley(chip.min + vec2(10.0, 6.0), galley, palette.text);
}

fn fmt_width(w: f32) -> String {
    if w >= 10.0 || w.fract() == 0.0 {
        format!("{w:.0}")
    } else {
        format!("{w:.1}")
    }
}

/// The drawn tool cursor: a crosshair with a gap at `p` (white with a dark halo, so it shows
/// on any colour) and the tool's icon on a dark chip below right, above everything else.
fn draw_cursor(ctx: &egui::Context, p: egui::Pos2, texture: egui::TextureId) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("tool_cursor"),
    ));
    let (gap, arm) = (3.0, 6.0);
    let arms = [
        vec2(1.0, 0.0),
        vec2(-1.0, 0.0),
        vec2(0.0, 1.0),
        vec2(0.0, -1.0),
    ];
    for (width, color) in [
        (3.0, egui::Color32::from_black_alpha(170)),
        (1.0, egui::Color32::WHITE),
    ] {
        for d in arms {
            painter.line_segment(
                [p + d * gap, p + d * (gap + arm)],
                egui::Stroke::new(width, color),
            );
        }
    }
    let chip = Rect::from_min_size(p + vec2(9.0, 9.0), vec2(20.0, 20.0));
    painter.rect(
        chip,
        6.0,
        egui::Color32::from_rgba_unmultiplied(20, 22, 26, 225),
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40)),
        egui::StrokeKind::Inside,
    );
    painter.image(
        texture,
        Rect::from_center_size(chip.center(), vec2(14.0, 14.0)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
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

/// Upload the whole-image ants mask when it changed since the last upload (changed tiles only).
fn sync_ants(gpu: &egui_wgpu::RenderState, tab: &mut DocumentTab, rev: u64, mask: &pnn_core::Mask) {
    if tab.gpu.ants_revision == rev {
        return;
    }
    let renderer = gpu.renderer.read();
    let Some(canvas) = renderer.callback_resources.get::<CanvasRenderer>() else {
        return;
    };
    let n = (mask.tiles_x() * mask.tiles_y()) as usize;
    if tab.gpu.ants_tiles.len() != n || tab.gpu.ants_revision == 0 {
        tab.gpu.ants_tiles = vec![usize::MAX; n];
    }
    let tiles_x = mask.tiles_x();
    for c in mask.tile_coords() {
        let tile = mask.tile(c);
        let id = tile.map_or(0, |t| std::sync::Arc::as_ptr(t) as usize);
        let slot = &mut tab.gpu.ants_tiles[(c.ty * tiles_x + c.tx) as usize];
        if *slot != id {
            canvas.upload_ants(&gpu.queue, tab.key, c, tile.map(|t| t.pixels()));
            *slot = id;
        }
    }
    tab.gpu.ants_revision = rev;
}

/// Tiles composited and shown per frame when many changed at once (undo of a whole-image
/// change on a 24 MP photo is ~370 tiles, ~96 MB of uploads: 15–35 ms in one frame). The
/// visible ones go first; the rest follow over the next frames.
const TILES_PER_FRAME: usize = 96;

/// Composite and show what changed. Returns true if tiles are left for the next frames.
fn sync_gpu(gpu: &egui_wgpu::RenderState, tab: &mut DocumentTab, visible: Option<Rect2>) -> bool {
    let (w, h) = (tab.doc.width(), tab.doc.height());
    let mut renderer = gpu.renderer.write();
    let Some(canvas) = renderer.callback_resources.get_mut::<CanvasRenderer>() else {
        return false;
    };
    let recreated = canvas.ensure_canvas(&gpu.device, tab.key, w, h);
    let (tx, ty) = (tab.doc.tiles_x(), tab.doc.tiles_y());
    let sync = &mut tab.gpu;
    if recreated || sync.revisions.len() != (tx * ty) as usize {
        sync.revisions = vec![0; (tx * ty) as usize];
        sync.overlay_revision = 0;
        sync.overlay_tiles.clear();
        sync.ants_revision = 0;
        sync.ants_tiles.clear();
    }
    let doc = &tab.doc;
    // A new or changed live gradient (or its end) redraws everything.
    let preview = sync.preview.as_ref().map(|p| {
        let selection = p
            .selection
            .as_ref()
            .map_or(0, |m| std::sync::Arc::as_ptr(m) as usize);
        (p.layer, p.shader, selection)
    });
    let floating = sync
        .floating
        .as_ref()
        .map(|f| (f.layer, f.serial, f.offset, f.inverse));
    // A new gradient or floating position (or either ending) redraws everything at once.
    let all = preview != sync.shown_preview || floating != sync.shown_floating;
    sync.shown_preview = preview;
    sync.shown_floating = floating;
    let mut stale: Vec<TileCoord> = (0..ty)
        .flat_map(|ty| (0..tx).map(move |tx| TileCoord { tx, ty }))
        .filter(|&c| all || doc.tile_revision(c) != sync.revisions[(c.ty * tx + c.tx) as usize])
        .collect();
    if stale.is_empty() {
        return false;
    }
    // The live gradient redraws everything at once (no half-updated frames while it moves).
    let mut more = false;
    if !all && stale.len() > TILES_PER_FRAME {
        let t = pnn_core::TILE_SIZE as f64;
        if let Some([x0, y0, x1, y1]) = visible {
            stale.sort_by_key(|c| {
                let (cx0, cy0) = (c.tx as f64 * t, c.ty as f64 * t);
                let on_screen = cx0 + t > x0 && cx0 < x1 && cy0 + t > y0 && cy0 < y1;
                !on_screen
            });
        }
        stale.truncate(TILES_PER_FRAME);
        more = true;
    }
    let layers: Vec<pnn_render::LayerInput<'_>> = doc
        .layers()
        .iter()
        .map(|l| pnn_render::LayerInput {
            id: l.id(),
            shown: l.visible && l.opacity > 0,
            mode: l.blend_mode,
            opacity: l.opacity,
            surface: &l.surface,
            gradient: sync
                .preview
                .as_ref()
                .filter(|p| p.layer == l.id())
                .map(|p| pnn_render::LayerGradient {
                    shader: p.shader,
                    selection: p.selection.as_ref(),
                }),
            floating: sync
                .floating
                .as_ref()
                .filter(|f| f.layer == l.id())
                .map(|f| pnn_render::LayerFloating {
                    serial: f.serial,
                    pixels: &f.pixels,
                    rect: f.rect,
                    offset: f.offset,
                    inverse: f.inverse,
                }),
        })
        .collect();
    crate::diagnostics::timed("composite", || {
        canvas.composite(&gpu.device, &gpu.queue, tab.key, &layers, &stale)
    });
    for &c in &stale {
        sync.revisions[(c.ty * tx + c.tx) as usize] = doc.tile_revision(c);
    }
    more
}

/// What identifies a floating preview's drawing: (layer, serial, offset, transform).
type FloatingKey = (
    pnn_core::LayerId,
    u64,
    [i32; 2],
    Option<pnn_render::FloatingTransform>,
);

/// Image-space rectangle [x0, y0, x1, y1].
type Rect2 = [f64; 4];

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
        let total = view.scroll_length(axis, img, vp).unwrap_or(vp[axis]);
        let px_per_track_pt = (total - vp[axis]) / (track_len - thumb_len).max(1.0);
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
        // A thin pill floating over the image (no track), thicker while in use.
        let active = response.dragged() || response.hovered();
        let thickness = if active { 8.0 } else { 5.0 };
        let pill = if axis == 0 {
            Rect::from_center_size(thumb.center(), vec2(thumb.width(), thickness))
        } else {
            Rect::from_center_size(thumb.center(), vec2(thickness, thumb.height()))
        };
        let fill = if active {
            egui::Color32::from_rgba_unmultiplied(170, 175, 185, 230)
        } else {
            egui::Color32::from_rgba_unmultiplied(130, 135, 145, 190)
        };
        ui.painter().rect(
            pill,
            thickness / 2.0,
            fill,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(90)),
            egui::StrokeKind::Outside,
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

/// Above this many outline points the ants are drawn by the GPU from the selection mask
/// (constant cost) instead of as exact polygon lines.
pub const GPU_ANTS_POINTS: usize = 20_000;

/// paint.net's marching ants: a white line with animated black dashes along the selection's
/// polygon edges. Only segments near the visible area are drawn.
fn marching_ants(
    ui: &mut egui::Ui,
    contours: &[pnn_core::selection::Contour],
    view: &View,
    canvas: Rect,
    clip: Rect,
    vp: [f64; 2],
    ppp: f64,
) {
    let painter = ui.painter().with_clip_rect(clip);
    let time = ui.input(|i| i.time);
    let offset = ((time * 12.0) % 8.0) as f32;
    let visible = clip.expand(4.0);
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
