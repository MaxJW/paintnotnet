//! Frosted glass behind the islands and dialogs (design A).
//!
//! The canvas draws a blurred copy of itself each frame (`pnn_render::glass`); here panels and
//! dialogs paint that blur, tinted, in place of their solid fill. Without a document on the
//! GPU there is nothing to blur, and they keep the solid fill. Menus, tooltips and the modal
//! message boxes stay solid (egui draws their frames internally).

use std::hash::{Hash, Hasher};

use eframe::egui::{self, Color32, CornerRadius, Frame, InnerResponse, Rect, Shape, Stroke};
use eframe::egui_wgpu;
use pnn_render::{GlassCallback, GlassUniforms};

use crate::theme;

/// Extra saturation of the blurred canvas, as CSS `saturate(160%)` in the mockup.
const SATURATION: f32 = 1.6;

fn backdrop_id() -> egui::Id {
    egui::Id::new("glass_backdrop")
}

/// Where the canvas drew its blurred backdrop this frame (`None`: no glass this frame).
pub fn set_backdrop(ctx: &egui::Context, area: Option<Rect>) {
    ctx.data_mut(|d| d.insert_temp(backdrop_id(), area));
}

pub fn backdrop(ctx: &egui::Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Option<Rect>>(backdrop_id()))
        .flatten()
}

/// The paint callback for a glass rectangle at `rect` (points) over the backdrop `area`.
fn glass_shape(
    ctx: &egui::Context,
    id: impl Hash,
    rect: Rect,
    radius: CornerRadius,
    area: Rect,
    tint: Color32,
    opacity: f32,
) -> Shape {
    let ppp = ctx.pixels_per_point();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let [r, g, b, a] = tint.to_srgba_unmultiplied();
    let px = |v: f32| v * ppp;
    let uniforms = GlassUniforms {
        rect: [
            px(rect.min.x),
            px(rect.min.y),
            px(rect.max.x),
            px(rect.max.y),
        ],
        area: [
            px(area.min.x),
            px(area.min.y),
            px(area.width()),
            px(area.height()),
        ],
        tint: [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ],
        radius: px(radius.average()),
        saturation: SATURATION,
        opacity,
        ..Default::default()
    };
    Shape::Callback(egui_wgpu::Callback::new_paint_callback(
        rect,
        GlassCallback {
            id: hasher.finish(),
            uniforms,
        },
    ))
}

/// Show `frame` with glass in place of its fill when there's a backdrop. `id` must be stable
/// and unique for this panel.
pub fn show<R>(
    ui: &mut egui::Ui,
    frame: Frame,
    id: impl Hash,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> InnerResponse<R> {
    let Some(area) = backdrop(ui.ctx()) else {
        return frame.show(ui, add_contents);
    };
    let (stroke, radius) = (frame.stroke, frame.corner_radius);
    let mut slot = None;
    let response = frame
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .show(ui, |ui| {
            // Reserved first inside the frame: after its shadow, before the contents.
            slot = Some(ui.painter().add(Shape::Noop));
            add_contents(ui)
        });
    if let Some(slot) = slot {
        let rect = response.response.rect;
        let palette = theme::palette(ui.ctx());
        let glass = glass_shape(
            ui.ctx(),
            id,
            rect,
            radius,
            area,
            palette.glass,
            ui.opacity(),
        );
        let border = Shape::rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        ui.painter().set(slot, Shape::Vec(vec![glass, border]));
    }
    response
}

/// Show a dialog window with glass in place of its fill when there's a backdrop.
pub fn window<R>(
    ctx: &egui::Context,
    window: egui::Window<'_>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<InnerResponse<Option<R>>> {
    window_with_margin(ctx, window, None, add_contents)
}

/// `window`, with `margin` inside its frame instead of egui's.
pub fn window_with_margin<R>(
    ctx: &egui::Context,
    window: egui::Window<'_>,
    margin: Option<egui::Margin>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<InnerResponse<Option<R>>> {
    let mut frame = Frame::window(&ctx.global_style());
    if let Some(m) = margin {
        frame = frame.inner_margin(m);
    }
    let Some(area) = backdrop(ctx) else {
        return window.frame(frame).show(ctx, add_contents);
    };
    let (stroke, radius, shadow) = (frame.stroke, frame.corner_radius, frame.shadow);
    let response = window
        .frame(frame.fill(Color32::TRANSPARENT).stroke(Stroke::NONE))
        .show(ctx, add_contents)?;
    // The window frame's background is the first shape on the window's own layer, painted
    // before the title bar and contents: put the glass there.
    let rect = response.response.rect;
    let id = response.response.layer_id.id;
    let palette = theme::palette(ctx);
    let shapes = vec![
        Shape::Rect(shadow.as_shape(rect, radius)),
        glass_shape(ctx, id, rect, radius, area, palette.dialog_glass, 1.0),
        Shape::rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside),
    ];
    ctx.layer_painter(response.response.layer_id)
        .set(egui::layers::ShapeIdx(0), Shape::Vec(shapes));
    Some(response)
}
