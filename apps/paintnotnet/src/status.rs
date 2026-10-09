//! Status bar, laid out like paint.net 5's (`StatusBar.html`):
//! tool hint on the left; selection size, cursor position, units, zoom box, zoom to window,
//! zoom out, zoom slider and zoom in on the right.

use eframe::egui::{self, vec2};
use pnn_render::view::{MAX_ZOOM, MIN_ZOOM};
use pnn_tools::ToolKind;

use crate::commands::{Command, Units};
use crate::icons::{Icon, Icons};

pub struct StatusInfo {
    pub tool: ToolKind,
    pub busy: bool,
    /// Selection bounds size, or the image size when nothing is selected.
    pub selection_size: Option<(u32, u32)>,
    pub cursor: Option<[f64; 2]>,
    pub zoom: Option<f64>,
    pub units: Units,
    pub dpi: f64,
}

pub enum StatusAction {
    SetZoom(f64),
    Command(Command),
}

#[derive(Default)]
pub struct StatusState {
    zoom_text: String,
    zoom_text_for: Option<u64>,
}

pub fn format_zoom(zoom: f64) -> String {
    let pct = zoom * 100.0;
    if pct < 10.0 && (pct - pct.round()).abs() > 0.05 {
        format!("{pct:.1}")
    } else {
        format!("{}", pct.round() as i64)
    }
}

pub fn format_length(px: f64, units: Units, dpi: f64) -> String {
    match units {
        Units::Pixels => format!("{}", px.floor() as i64),
        Units::Inches => format!("{:.2}", px / dpi),
        Units::Centimeters => format!("{:.2}", px / dpi * 2.54),
    }
}

fn units_label(units: Units) -> &'static str {
    match units {
        Units::Pixels => "px",
        Units::Inches => "in",
        Units::Centimeters => "cm",
    }
}

/// Slider position (0..1) ↔ zoom, logarithmic over paint.net's 1%–3200% range.
fn zoom_to_slider(zoom: f64) -> f64 {
    (zoom.ln() - MIN_ZOOM.ln()) / (MAX_ZOOM.ln() - MIN_ZOOM.ln())
}

fn slider_to_zoom(t: f64) -> f64 {
    (MIN_ZOOM.ln() + t.clamp(0.0, 1.0) * (MAX_ZOOM.ln() - MIN_ZOOM.ln())).exp()
}

pub fn status_bar(
    ui: &mut egui::Ui,
    info: &StatusInfo,
    state: &mut StatusState,
    icons: &Icons,
) -> Option<StatusAction> {
    let mut action = None;
    // Right-hand controls keep their space; the hint on the left is cut short.
    egui::Sides::new().shrink_left().show(
        ui,
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(icons.image(Icon::for_tool(info.tool)));
            if info.busy {
                ui.spinner();
                ui.label("Working…");
            } else {
                let hint = format!("{}: {}", info.tool.name(), info.tool.help());
                ui.add(egui::Label::new(hint).truncate());
            }
        },
        |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let has_doc = info.zoom.is_some();
            if ui
                .add_enabled(
                    has_doc,
                    egui::Button::image(icons.image(Icon::ZoomIn)).frame(false),
                )
                .on_hover_text("Zoom In")
                .clicked()
            {
                action = Some(StatusAction::Command(Command::ZoomIn));
            }
            let mut t = info.zoom.map_or(0.5, zoom_to_slider);
            let slider = ui.add_enabled(
                has_doc,
                egui::Slider::new(&mut t, 0.0..=1.0)
                    .show_value(false)
                    .trailing_fill(false),
            );
            if slider.changed() {
                action = Some(StatusAction::SetZoom(slider_to_zoom(t)));
            }
            if ui
                .add_enabled(
                    has_doc,
                    egui::Button::image(icons.image(Icon::ZoomOut)).frame(false),
                )
                .on_hover_text("Zoom Out")
                .clicked()
            {
                action = Some(StatusAction::Command(Command::ZoomOut));
            }
            if ui
                .add_enabled(
                    has_doc,
                    egui::Button::image(icons.image(Icon::ZoomToWindow)).frame(false),
                )
                .on_hover_text("Zoom to Window (Ctrl+B)")
                .clicked()
            {
                action = Some(StatusAction::Command(Command::ZoomToWindow));
            }
            // Zoom percentage box: type a value and press Enter.
            if let Some(zoom) = info.zoom {
                let key = (zoom * 1e6) as u64;
                if state.zoom_text_for != Some(key) {
                    state.zoom_text = format!("{}%", format_zoom(zoom));
                    state.zoom_text_for = Some(key);
                }
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.zoom_text)
                        .desired_width(46.0)
                        .horizontal_align(egui::Align::RIGHT),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Ok(pct) = state
                        .zoom_text
                        .trim()
                        .trim_end_matches('%')
                        .trim()
                        .parse::<f64>()
                    {
                        action = Some(StatusAction::SetZoom(
                            (pct / 100.0).clamp(MIN_ZOOM, MAX_ZOOM),
                        ));
                    }
                    state.zoom_text_for = None;
                }
            }
            ui.separator();
            egui::ComboBox::from_id_salt("units")
                .selected_text(units_label(info.units))
                .width(36.0)
                .show_ui(ui, |ui| {
                    for (u, name) in [
                        (Units::Pixels, "Pixels"),
                        (Units::Inches, "Inches"),
                        (Units::Centimeters, "Centimeters"),
                    ] {
                        if ui.selectable_label(info.units == u, name).clicked() {
                            action = Some(StatusAction::Command(Command::SetUnits(u)));
                        }
                    }
                });
            ui.separator();
            if let Some([x, y]) = info.cursor {
                let text = format!(
                    "{}, {}",
                    format_length(x, info.units, info.dpi),
                    format_length(y, info.units, info.dpi)
                );
                ui.add_sized(vec2(84.0, 16.0), egui::Label::new(text));
                ui.add(icons.image(Icon::CursorXY));
            } else {
                ui.add_space(104.0);
            }
            ui.separator();
            if let Some((w, h)) = info.selection_size {
                let text = format!(
                    "{} × {}",
                    format_length(w as f64, info.units, info.dpi),
                    format_length(h as f64, info.units, info.dpi)
                );
                ui.add_sized(vec2(84.0, 16.0), egui::Label::new(text));
                ui.add(icons.image(Icon::Selection));
            }
        },
    );
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_text() {
        assert_eq!(format_zoom(1.0), "100");
        assert_eq!(format_zoom(0.66), "66");
        assert_eq!(format_zoom(0.164), "16");
        assert_eq!(format_zoom(0.083), "8.3");
    }

    #[test]
    fn slider_round_trip_and_ends() {
        assert!((slider_to_zoom(zoom_to_slider(1.0)) - 1.0).abs() < 1e-9);
        assert!((slider_to_zoom(0.0) - MIN_ZOOM).abs() < 1e-12);
        assert!((slider_to_zoom(1.0) - MAX_ZOOM).abs() < 1e-9);
    }

    #[test]
    fn units() {
        assert_eq!(format_length(96.0, Units::Inches, 96.0), "1.00");
        assert_eq!(format_length(96.0, Units::Centimeters, 96.0), "2.54");
        assert_eq!(format_length(12.7, Units::Pixels, 96.0), "12");
    }
}
