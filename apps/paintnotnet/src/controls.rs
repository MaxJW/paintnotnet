//! Design A's tool controls: compact sliders with typed values, the brush size control
//! (live preview dot, logarithmic slider, presets) and the opacity control.

use eframe::egui::{self, Color32, Rect, Sense, Stroke, pos2, vec2};
use pnn_tools::{MAX_BRUSH_WIDTH, MIN_BRUSH_WIDTH, ToolSettings};

use crate::theme::{self, Palette};

/// paint.net's brush width presets: the mouse wheel over the size control steps through them.
const WIDTH_PRESETS: &[f32] = &[
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 20.0, 25.0,
    30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0, 65.0, 70.0, 75.0, 80.0, 85.0, 90.0, 95.0, 100.0,
    125.0, 150.0, 175.0, 200.0, 225.0, 250.0, 275.0, 300.0, 325.0, 350.0, 375.0, 400.0, 425.0,
    450.0, 475.0, 500.0, 1000.0, 2000.0,
];

/// The next paint.net preset above or below `current`.
fn step_preset(current: f32, up: bool) -> f32 {
    if up {
        WIDTH_PRESETS
            .iter()
            .copied()
            .find(|&w| w > current)
            .unwrap_or(MAX_BRUSH_WIDTH)
    } else {
        WIDTH_PRESETS
            .iter()
            .rev()
            .copied()
            .find(|&w| w < current)
            .unwrap_or(MIN_BRUSH_WIDTH)
    }
}

/// Quick brush sizes, px.
pub const SIZE_PRESETS: [f32; 6] = [2.0, 5.0, 10.0, 25.0, 50.0, 100.0];

/// Brush width ↔ slider position (0–1), logarithmic so small sizes get room.
pub fn size_to_t(width: f32) -> f32 {
    (width.max(MIN_BRUSH_WIDTH) / MIN_BRUSH_WIDTH).ln() / (MAX_BRUSH_WIDTH / MIN_BRUSH_WIDTH).ln()
}

pub fn t_to_size(t: f32) -> f32 {
    round_width(MIN_BRUSH_WIDTH * (MAX_BRUSH_WIDTH / MIN_BRUSH_WIDTH).powf(t.clamp(0.0, 1.0)))
}

/// Whole pixels from 10 px up, tenths below (fractional widths are antialiased).
pub fn round_width(w: f32) -> f32 {
    if w >= 10.0 {
        w.round()
    } else {
        (w * 10.0).round() / 10.0
    }
}

/// Lay out a group of controls as one unit in a wrapping toolbar: when it won't fit on the
/// current row (its width is remembered from the last frame) it starts a new one, so a
/// label never ends up on a different line from its control.
pub fn group<R>(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let id = ui.id().with(("control_group", id));
    let width: Option<f32> = ui.data(|d| d.get_temp(id));
    let at_row_start = ui.cursor().min.x <= ui.max_rect().min.x + 1.0;
    if let Some(w) = width
        && w > ui.available_size_before_wrap().x + 0.5
        && !at_row_start
    {
        ui.end_row();
    }
    let response = ui.horizontal(add);
    let w = response.response.rect.width();
    if width.is_none_or(|old| (old - w).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(id, w));
        ui.ctx().request_repaint();
    }
    response.inner
}

/// A rail slider over 0–1 (`t`), `width` points wide. Returns the response; `t` is updated
/// while it's dragged or clicked.
pub fn slider(
    ui: &mut egui::Ui,
    t: &mut f32,
    width: f32,
    label: &str,
    palette: &Palette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click_and_drag());
    let rail = Rect::from_center_size(rect.center(), vec2(width - 14.0, 4.0));
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        *t = ((p.x - rail.min.x) / rail.width()).clamp(0.0, 1.0);
    }
    let value = *t;
    response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(value), label));
    let painter = ui.painter();
    painter.rect_filled(rail, 2.0, palette.control);
    let x = rail.min.x + rail.width() * value;
    painter.rect_filled(
        Rect::from_min_max(rail.min, pos2(x, rail.max.y)),
        2.0,
        palette.accent,
    );
    let active = response.hovered() || response.dragged();
    let halo =
        ui.ctx()
            .animate_bool_with_time(response.id.with("halo"), active, theme::motion::FAST);
    let c = pos2(x, rail.center().y);
    if halo > 0.0 {
        painter.circle_filled(c, 7.0 + 3.0 * halo, palette.accent_alpha(0.35 * halo));
    }
    painter.circle(
        c,
        7.0,
        Color32::from_rgb(0xf2, 0xf4, 0xf7),
        Stroke::new(1.0, Color32::from_black_alpha(60)),
    );
    response
}

/// Mouse wheel steps over a hovered control (+ = up).
fn wheel(ui: &egui::Ui, response: &egui::Response) -> f32 {
    if !response.hovered() {
        return 0.0;
    }
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { delta, .. } => Some(delta.y.signum()),
                _ => None,
            })
            .sum()
    })
}

/// A typed value in the mono face.
fn value_field<'a>(
    value: &'a mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::DragValue<'a> {
    egui::DragValue::new(value)
        .range(range)
        .speed(0.25)
        .max_decimals(1)
}

/// "Label [====o----] 75%": a 0–100 setting.
pub fn percent(ui: &mut egui::Ui, label: &str, value: &mut u8, palette: &Palette) {
    group(ui, label, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(egui::RichText::new(label).color(palette.weak_text));
        let mut t = f32::from(*value) / 100.0;
        let response = slider(ui, &mut t, 86.0, label, palette);
        let mut v = (t * 100.0).round();
        v += wheel(ui, &response);
        let mut typed = v;
        let field = ui.add(
            value_field(&mut typed, 0.0..=100.0)
                .suffix("%")
                .max_decimals(0)
                .speed(0.5),
        );
        if field.changed() {
            v = typed;
        }
        *value = v.clamp(0.0, 100.0) as u8;
    });
}

/// Opacity of the painting: the active colour's alpha, as paint.net treats it.
pub fn opacity(ui: &mut egui::Ui, settings: &mut ToolSettings, palette: &Palette) {
    let mut c = settings.active_color_value();
    let mut pct = (f32::from(c.a) / 255.0 * 100.0).round() as u8;
    let before = pct;
    percent(ui, "Opacity", &mut pct, palette);
    if pct != before {
        c.a = (f32::from(pct) / 100.0 * 255.0).round() as u8;
        settings.set_active_color_value(c);
    }
}

/// The brush size control: preview dot, "Size", logarithmic slider, typed px, presets.
pub fn brush_size(ui: &mut egui::Ui, settings: &mut ToolSettings, palette: &Palette) {
    group(ui, "brush size", |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        brush_preview(ui, settings, palette);
        ui.label(egui::RichText::new("Size").color(palette.weak_text));
        let mut t = size_to_t(settings.brush_width);
        let response = slider(ui, &mut t, 140.0, "Brush size", palette);
        if response.dragged() || response.clicked() {
            settings.brush_width = t_to_size(t);
        }
        let steps = wheel(ui, &response);
        if steps != 0.0 {
            settings.brush_width = step_preset(settings.brush_width, steps > 0.0);
        }
        ui.add(
            value_field(&mut settings.brush_width, MIN_BRUSH_WIDTH..=MAX_BRUSH_WIDTH)
                .custom_formatter(|v, _| {
                    if v >= 10.0 || v.fract() == 0.0 {
                        format!("{v:.0}")
                    } else {
                        format!("{v:.1}")
                    }
                })
                .suffix(" px"),
        )
        .on_hover_text("Brush size ([ and ] step it; Alt + right-drag on the canvas)");
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.spacing_mut().button_padding = vec2(6.0, 2.0);
        for w in SIZE_PRESETS {
            let selected = (settings.brush_width - w).abs() < 0.05;
            let text = egui::RichText::new(format!("{w}"))
                .font(egui::FontId::monospace(theme::text::SMALL))
                .color(if selected {
                    palette.text
                } else {
                    palette.weak_text
                });
            if ui
                .add(
                    egui::Button::new(text)
                        .selected(selected)
                        .min_size(vec2(28.0, 22.0)),
                )
                .on_hover_text(format!("{w} px"))
                .clicked()
            {
                settings.brush_width = w;
            }
        }
    });
}

/// A 28 pt square showing the brush: its size (log-scaled to fit), hardness (soft edge) and
/// the active colour.
fn brush_preview(ui: &mut egui::Ui, settings: &ToolSettings, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::radius::CONTROL, palette.list);
    let r = 1.5 + 10.5 * size_to_t(settings.brush_width).sqrt();
    let c = settings.active_color_value();
    let color = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a.max(40));
    soft_dot(
        ui.painter(),
        rect.center(),
        r,
        f32::from(settings.hardness) / 100.0,
        color,
    );
}

/// A dot of radius `r` whose solid core is `hardness` of it, fading to the edge.
pub fn soft_dot(painter: &egui::Painter, c: egui::Pos2, r: f32, hardness: f32, color: Color32) {
    let core = r * hardness;
    let rings = 10;
    for i in (0..rings).rev() {
        let f = (i as f32 + 1.0) / rings as f32;
        let radius = core + (r - core) * f;
        let alpha = 1.0 - f * 0.9;
        painter.circle_filled(c, radius, color.gamma_multiply(alpha / rings as f32 * 2.5));
    }
    if core > 0.3 {
        painter.circle_filled(c, core, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_presets_step() {
        assert_eq!(step_preset(2.0, true), 3.0);
        assert_eq!(step_preset(15.0, true), 20.0);
        assert_eq!(step_preset(17.0, false), 15.0);
        assert_eq!(step_preset(1.0, false), 1.0);
        assert_eq!(step_preset(2000.0, true), 2000.0);
    }

    #[test]
    fn size_scale_round_trips_and_spans_the_range() {
        assert_eq!(t_to_size(0.0), MIN_BRUSH_WIDTH);
        assert_eq!(t_to_size(1.0), MAX_BRUSH_WIDTH);
        for w in [1.0, 2.5, 10.0, 60.0, 500.0, 2000.0] {
            assert!((t_to_size(size_to_t(w)) - w).abs() <= 0.051, "{w}");
        }
        // Half way along is about 45 px: small sizes get most of the travel.
        assert!((40.0..50.0).contains(&t_to_size(0.5)));
    }
}
