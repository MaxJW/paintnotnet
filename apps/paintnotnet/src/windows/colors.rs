//! The Colors window: primary/secondary swatches, colour wheel, "More >>" RGB/HSV/hex/alpha
//! controls, and the palette.
//!
//! Wheel geometry follows paint.net 3.36 `ColorWheel.cs` (MIT, see NOTICE): hue 0° (red) at
//! 3 o'clock increasing clockwise, saturation = distance from the centre. Modifier
//! behaviour is from the 5.x shortcut table: Ctrl = hue only, Alt = saturation only,
//! Shift = saturation with hue snapped to 15°, Ctrl+Shift = hue snapped to 15°.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use pnn_core::ColorBgra;
use pnn_tools::{ActiveColor, ToolSettings};

use crate::icons::{Icon, Icons};
use crate::palette::DEFAULT_PALETTE;

/// Hue in degrees [0, 360), saturation and value in [0, 1].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

pub fn rgb_to_hsv(c: ColorBgra) -> Hsv {
    let (r, g, b) = (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    Hsv {
        h,
        s: if max == 0.0 { 0.0 } else { d / max },
        v: max,
    }
}

pub fn hsv_to_rgb(hsv: Hsv, a: u8) -> ColorBgra {
    let c = hsv.v * hsv.s;
    let hp = (hsv.h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = hsv.v - c;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    ColorBgra::from_rgba(to(r), to(g), to(b), a)
}

pub fn to_color32(c: ColorBgra) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Colour as paint.net's hex box shows it: RRGGBB, plus AA when not opaque.
pub fn to_hex(c: ColorBgra) -> String {
    if c.a == 255 {
        format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

pub fn from_hex(s: &str, alpha: u8) -> Option<ColorBgra> {
    let s = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok()?;
    match s.len() {
        6 => Some(ColorBgra::from_rgba(
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
            alpha,
        )),
        8 => Some(ColorBgra::from_rgba(
            (v >> 24) as u8,
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
        )),
        3 => {
            let e = |n: u32| ((n & 0xF) * 17) as u8;
            Some(ColorBgra::from_rgba(e(v >> 8), e(v >> 4), e(v), alpha))
        }
        _ => None,
    }
}

/// Wheel modifier behaviour from the shortcut table.
pub fn wheel_pick(
    current: Hsv,
    angle_deg: f32,
    radius_frac: f32,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> Hsv {
    let snap = |h: f32| ((h / 15.0).round() * 15.0).rem_euclid(360.0);
    let hue = angle_deg.rem_euclid(360.0);
    let sat = radius_frac.clamp(0.0, 1.0);
    match (ctrl, alt, shift) {
        (true, _, true) => Hsv {
            h: snap(hue),
            ..current
        },
        (true, _, false) => Hsv { h: hue, ..current },
        (false, true, _) => Hsv { s: sat, ..current },
        (false, false, true) => Hsv {
            h: snap(hue),
            s: sat,
            ..current
        },
        (false, false, false) => Hsv {
            h: hue,
            s: sat,
            ..current
        },
    }
}

pub struct ColorsState {
    pub more: bool,
    palette: Vec<ColorBgra>,
    /// Hue/saturation survive greys and black: keep the last HSV per slot.
    hsv: [Hsv; 2],
    hsv_of: [ColorBgra; 2],
    hex: String,
    hex_of: Option<ColorBgra>,
    /// "Add color to palette" armed: the next swatch click stores the active colour.
    adding: bool,
}

impl Default for ColorsState {
    fn default() -> Self {
        Self {
            more: false,
            palette: DEFAULT_PALETTE
                .iter()
                .map(|&c| ColorBgra::from_u32(c))
                .collect(),
            hsv: [
                Hsv {
                    h: 0.0,
                    s: 0.0,
                    v: 0.0,
                },
                Hsv {
                    h: 0.0,
                    s: 0.0,
                    v: 1.0,
                },
            ],
            hsv_of: [ColorBgra::BLACK, ColorBgra::WHITE],
            hex: String::new(),
            hex_of: None,
            adding: false,
        }
    }
}

const WHEEL: f32 = 150.0;
const SWATCH: f32 = 11.0;

impl ColorsState {
    fn slot(settings: &ToolSettings, inactive: bool) -> usize {
        match (settings.active_color, inactive) {
            (ActiveColor::Primary, false) | (ActiveColor::Secondary, true) => 0,
            _ => 1,
        }
    }

    fn color(settings: &ToolSettings, slot: usize) -> ColorBgra {
        if slot == 0 {
            settings.primary
        } else {
            settings.secondary
        }
    }

    fn set(settings: &mut ToolSettings, slot: usize, c: ColorBgra) {
        if slot == 0 {
            settings.primary = c
        } else {
            settings.secondary = c
        }
    }

    fn hsv(&mut self, settings: &ToolSettings, slot: usize) -> Hsv {
        let c = Self::color(settings, slot);
        if c != self.hsv_of[slot] {
            let fresh = rgb_to_hsv(c);
            let old = self.hsv[slot];
            // Keep hue (and saturation for black) when the new colour doesn't define them.
            self.hsv[slot] = Hsv {
                h: if fresh.s == 0.0 { old.h } else { fresh.h },
                s: if fresh.v == 0.0 { old.s } else { fresh.s },
                v: fresh.v,
            };
            self.hsv_of[slot] = c;
        }
        self.hsv[slot]
    }

    fn set_hsv(&mut self, settings: &mut ToolSettings, slot: usize, hsv: Hsv) {
        let a = Self::color(settings, slot).a;
        let c = hsv_to_rgb(hsv, a);
        Self::set(settings, slot, c);
        self.hsv[slot] = hsv;
        self.hsv_of[slot] = c;
    }

    /// Title-row button: "More" shows the RGB, HSV and alpha sliders under the wheel.
    pub fn actions(&mut self, ui: &mut egui::Ui) {
        let label = if self.more { "Less" } else { "More" };
        if ui
            .add(egui::Button::new(label).min_size(vec2(52.0, 24.0)))
            .on_hover_text("Show or hide the RGB, HSV and alpha sliders")
            .clicked()
        {
            self.more = !self.more;
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, settings: &mut ToolSettings, icons: &Icons) {
        let width = ui.available_width();
        ui.horizontal(|ui| {
            self.swatches(ui, settings, icons);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.menu_image_button(icons.image(Icon::Palettes), |ui| {
                    if ui.button("Reset to default palette").clicked() {
                        self.palette = DEFAULT_PALETTE
                            .iter()
                            .map(|&c| ColorBgra::from_u32(c))
                            .collect();
                        ui.close();
                    }
                    ui.add_enabled(false, egui::Button::new("Save palette as..."));
                    ui.add_enabled(false, egui::Button::new("Open palettes folder"));
                })
                .response
                .on_hover_text("Palettes");
                let add = ui.add(
                    egui::Button::image(icons.image(Icon::AddColor))
                        .selected(self.adding)
                        .frame_when_inactive(false),
                );
                if add
                    .on_hover_text("Add color to palette: click a swatch to store the active color")
                    .clicked()
                {
                    self.adding = !self.adding;
                }
            });
        });
        let wheel = (width - 8.0).clamp(96.0, WHEEL);
        ui.vertical_centered(|ui| self.wheel(ui, settings, wheel));
        self.palette_grid(ui, settings, width);
        if self.more {
            ui.add_space(4.0);
            ui.separator();
            self.more_panel(ui, settings);
        }
    }

    fn swatches(&mut self, ui: &mut egui::Ui, settings: &mut ToolSettings, icons: &Icons) {
        let (rect, _) = ui.allocate_exact_size(vec2(58.0, 52.0), Sense::hover());
        let primary = Rect::from_min_size(rect.min + vec2(4.0, 2.0), vec2(30.0, 30.0));
        let secondary = Rect::from_min_size(rect.min + vec2(20.0, 18.0), vec2(30.0, 30.0));
        let draw = |ui: &egui::Ui, r: Rect, c: ColorBgra, active: bool| {
            checker(ui, r.shrink(1.0));
            ui.painter().rect_filled(r.shrink(1.0), 0.0, to_color32(c));
            ui.painter().rect_stroke(
                r,
                0.0,
                Stroke::new(1.0, ui.visuals().text_color()),
                egui::StrokeKind::Inside,
            );
            if active {
                ui.painter().rect_stroke(
                    r.expand(1.5),
                    0.0,
                    Stroke::new(1.0, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Outside,
                );
            }
        };
        // Active slot is drawn on top.
        let primary_active = settings.active_color == ActiveColor::Primary;
        if primary_active {
            draw(ui, secondary, settings.secondary, false);
            draw(ui, primary, settings.primary, true);
        } else {
            draw(ui, primary, settings.primary, false);
            draw(ui, secondary, settings.secondary, true);
        }
        // egui gives overlapping clicks to the widget added last: add the top swatch last.
        let (p, s) = if primary_active {
            let s = ui.interact(secondary, ui.id().with("secondary"), Sense::click());
            (
                ui.interact(primary, ui.id().with("primary"), Sense::click()),
                s,
            )
        } else {
            let p = ui.interact(primary, ui.id().with("primary"), Sense::click());
            (
                p,
                ui.interact(secondary, ui.id().with("secondary"), Sense::click()),
            )
        };
        let (p, s) = (
            p.on_hover_text("Primary color"),
            s.on_hover_text("Secondary color"),
        );
        if p.clicked() {
            settings.active_color = ActiveColor::Primary;
        }
        if s.clicked() {
            settings.active_color = ActiveColor::Secondary;
        }
        let swap = Rect::from_min_size(pos2(rect.max.x - 14.0, rect.min.y), vec2(14.0, 14.0));
        ui.painter().image(
            icons.texture(Icon::Swap).id(),
            swap,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        if ui
            .interact(swap, ui.id().with("swap"), Sense::click())
            .on_hover_text("Swap colors (X)")
            .clicked()
        {
            settings.swap_colors();
        }
        let reset = Rect::from_min_size(pos2(rect.min.x, rect.max.y - 14.0), vec2(14.0, 14.0));
        ui.painter().image(
            icons.texture(Icon::Reset).id(),
            reset,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        if ui
            .interact(reset, ui.id().with("reset"), Sense::click())
            .on_hover_text("Reset to black and white")
            .clicked()
        {
            settings.primary = ColorBgra::BLACK;
            settings.secondary = ColorBgra::WHITE;
        }
    }

    fn wheel(&mut self, ui: &mut egui::Ui, settings: &mut ToolSettings, size: f32) {
        let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
        let center = rect.center();
        let radius = size / 2.0 - 1.0;
        // Hue/saturation disc at full value, as paint.net draws it.
        let mut mesh = Mesh::default();
        mesh.colored_vertex(center, Color32::WHITE);
        let n = 120;
        for i in 0..=n {
            let h = i as f32 / n as f32 * 360.0;
            let t = h.to_radians();
            let c = hsv_to_rgb(Hsv { h, s: 1.0, v: 1.0 }, 255);
            mesh.colored_vertex(center + vec2(t.cos(), t.sin()) * radius, to_color32(c));
            if i > 0 {
                mesh.add_triangle(0, i, i + 1);
            }
        }
        ui.painter().add(Shape::mesh(mesh));
        ui.painter().circle_stroke(
            center,
            radius,
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        let pointer = response.interact_pointer_pos();
        let button_inactive = ui.input(|i| i.pointer.button_down(egui::PointerButton::Secondary));
        if (response.dragged()
            || response.clicked()
            || response.secondary_clicked()
            || response.drag_started())
            && let Some(p) = pointer
        {
            let slot = Self::slot(settings, button_inactive || response.secondary_clicked());
            let d = p - center;
            let angle = d.y.atan2(d.x).to_degrees();
            let frac = d.length() / radius;
            let m = ui.input(|i| i.modifiers);
            let current = self.hsv(settings, slot);
            let picked = wheel_pick(current, angle, frac, m.ctrl, m.alt, m.shift);
            self.set_hsv(settings, slot, picked);
        }
        // Selector on the active colour.
        let slot = Self::slot(settings, false);
        let hsv = self.hsv(settings, slot);
        let t = hsv.h.to_radians();
        let p = center + vec2(t.cos(), t.sin()) * hsv.s * (radius - 1.0);
        ui.painter()
            .circle_stroke(p, 4.0, Stroke::new(1.5, Color32::BLACK));
        ui.painter()
            .circle_stroke(p, 2.5, Stroke::new(1.0, Color32::WHITE));
    }

    fn palette_grid(&mut self, ui: &mut egui::Ui, settings: &mut ToolSettings, width: f32) {
        let rows = if self.more { 6 } else { 2 };
        let swatch = ((width - 1.0) / 16.0).floor().clamp(SWATCH, 2.0 * SWATCH);
        let (rect, _) = ui.allocate_exact_size(
            vec2(16.0 * swatch + 1.0, rows as f32 * swatch + 1.0),
            Sense::hover(),
        );
        for i in 0..(16 * rows).min(self.palette.len()) {
            let c = self.palette[i];
            let (col, row) = ((i % 16) as f32, (i / 16) as f32);
            let r = Rect::from_min_size(
                rect.min + vec2(col * swatch, row * swatch),
                vec2(swatch, swatch),
            );
            ui.painter().rect_filled(r.shrink(1.0), 2.0, to_color32(c));
            let response = ui
                .interact(r, ui.id().with(("swatch", i)), Sense::click())
                .on_hover_text(to_hex(c));
            if response.hovered() {
                ui.painter().rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, ui.visuals().text_color()),
                    egui::StrokeKind::Inside,
                );
            }
            if response.clicked() {
                if self.adding {
                    self.palette[i] = settings.active_color_value();
                    self.adding = false;
                } else {
                    settings.set_active_color_value(c);
                }
            } else if response.secondary_clicked() {
                settings.set_inactive_color_value(c);
            }
        }
    }

    fn more_panel(&mut self, ui: &mut egui::Ui, settings: &mut ToolSettings) {
        let slot = Self::slot(settings, false);
        let mut c = Self::color(settings, slot);
        ui.label(egui::RichText::new("RGB").strong());
        let mut changed_rgb = false;
        for (label, ch) in [("R:", 0usize), ("G:", 1), ("B:", 2)] {
            ui.horizontal(|ui| {
                ui.label(label);
                let mut v = [c.r, c.g, c.b][ch];
                let lo = {
                    let mut k = c;
                    set_ch(&mut k, ch, 0);
                    k
                };
                let hi = {
                    let mut k = c;
                    set_ch(&mut k, ch, 255);
                    k
                };
                if gradient_slider(
                    ui,
                    &mut v,
                    &[
                        to_color32(lo.with_alpha(255)),
                        to_color32(hi.with_alpha(255)),
                    ],
                ) | ui
                    .add(egui::DragValue::new(&mut v).range(0..=255))
                    .changed()
                {
                    set_ch(&mut c, ch, v);
                    changed_rgb = true;
                }
            });
        }
        if changed_rgb {
            Self::set(settings, slot, c);
        }
        ui.horizontal(|ui| {
            ui.label("Hex:");
            if self.hex_of != Some(c) {
                self.hex = to_hex(c);
                self.hex_of = Some(c);
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.hex)
                    .desired_width(80.0)
                    .font(egui::TextStyle::Monospace),
            );
            if response.lost_focus()
                && let Some(parsed) = from_hex(&self.hex, c.a)
            {
                Self::set(settings, slot, parsed);
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new("HSV").strong());
        let mut hsv = self.hsv(settings, slot);
        let mut changed_hsv = false;
        ui.horizontal(|ui| {
            ui.label("H:");
            let mut hv = (hsv.h / 360.0 * 255.0).round() as u8;
            let rainbow: Vec<Color32> = (0..=6)
                .map(|i| {
                    to_color32(hsv_to_rgb(
                        Hsv {
                            h: i as f32 * 60.0,
                            s: 1.0,
                            v: 1.0,
                        },
                        255,
                    ))
                })
                .collect();
            if gradient_slider(ui, &mut hv, &rainbow) {
                hsv.h = hv as f32 / 255.0 * 360.0;
                changed_hsv = true;
            }
            let mut h = hsv.h.round() as u16;
            if ui
                .add(egui::DragValue::new(&mut h).range(0..=360))
                .changed()
            {
                hsv.h = h as f32;
                changed_hsv = true;
            }
        });
        for (label, which) in [("S:", 0), ("V:", 1)] {
            ui.horizontal(|ui| {
                ui.label(label);
                let val = if which == 0 { hsv.s } else { hsv.v };
                let mut b = (val * 255.0).round() as u8;
                let ends = if which == 0 {
                    [
                        hsv_to_rgb(Hsv { s: 0.0, ..hsv }, 255),
                        hsv_to_rgb(Hsv { s: 1.0, ..hsv }, 255),
                    ]
                } else {
                    [
                        hsv_to_rgb(Hsv { v: 0.0, ..hsv }, 255),
                        hsv_to_rgb(Hsv { v: 1.0, ..hsv }, 255),
                    ]
                };
                let mut pct = (val * 100.0).round() as u8;
                let slid = gradient_slider(ui, &mut b, &[to_color32(ends[0]), to_color32(ends[1])]);
                let typed = ui
                    .add(egui::DragValue::new(&mut pct).range(0..=100))
                    .changed();
                if slid || typed {
                    let v = if typed {
                        pct as f32 / 100.0
                    } else {
                        b as f32 / 255.0
                    };
                    if which == 0 {
                        hsv.s = v
                    } else {
                        hsv.v = v
                    }
                    changed_hsv = true;
                }
            });
        }
        if changed_hsv {
            self.set_hsv(settings, slot, hsv);
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Opacity - Alpha").strong());
        ui.horizontal(|ui| {
            let mut c = Self::color(settings, slot);
            let mut a = c.a;
            let ends = [to_color32(c.with_alpha(0)), to_color32(c.with_alpha(255))];
            if gradient_slider(ui, &mut a, &ends)
                | ui.add(egui::DragValue::new(&mut a).range(0..=255))
                    .changed()
            {
                c.a = a;
                Self::set(settings, slot, c);
            }
        });
    }
}

fn set_ch(c: &mut ColorBgra, ch: usize, v: u8) {
    match ch {
        0 => c.r = v,
        1 => c.g = v,
        _ => c.b = v,
    }
}

/// paint.net-style colour slider: a gradient bar with a triangle marker. Returns true when
/// the user changed the value.
fn gradient_slider(ui: &mut egui::Ui, value: &mut u8, stops: &[Color32]) -> bool {
    let width = (ui.available_width() - 64.0).max(96.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 14.0), Sense::click_and_drag());
    let bar = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - 4.0));
    checker(ui, bar);
    let mut mesh = Mesh::default();
    let n = stops.len().max(2) - 1;
    for (i, &c) in stops.iter().enumerate() {
        let x = bar.min.x + bar.width() * i as f32 / n as f32;
        mesh.colored_vertex(pos2(x, bar.min.y), c);
        mesh.colored_vertex(pos2(x, bar.max.y), c);
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
    ui.painter().rect_stroke(
        bar,
        0.0,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let x = bar.min.x + bar.width() * (*value as f32 / 255.0);
    let tri = vec![
        pos2(x, rect.max.y - 5.0),
        pos2(x - 4.0, rect.max.y),
        pos2(x + 4.0, rect.max.y),
    ];
    ui.painter().add(Shape::convex_polygon(
        tri,
        ui.visuals().text_color(),
        Stroke::NONE,
    ));
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let v = (((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0) * 255.0).round() as u8;
        if v != *value {
            *value = v;
            return true;
        }
    }
    false
}

fn checker(ui: &egui::Ui, r: Rect) {
    let painter = ui.painter_at(r);
    painter.rect_filled(r, 0.0, Color32::WHITE);
    let s = 4.0;
    let mut y = r.min.y;
    let mut row = 0;
    while y < r.max.y {
        let mut x = r.min.x + if row % 2 == 0 { 0.0 } else { s };
        while x < r.max.x {
            painter.rect_filled(
                Rect::from_min_size(pos2(x, y), vec2(s, s)),
                0.0,
                Color32::from_gray(191),
            );
            x += 2.0 * s;
        }
        y += s;
        row += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trip() {
        for c in [
            ColorBgra::from_rgba(255, 0, 0, 255),
            ColorBgra::from_rgba(12, 200, 99, 255),
            ColorBgra::from_rgba(128, 128, 128, 255),
        ] {
            assert_eq!(hsv_to_rgb(rgb_to_hsv(c), 255), c);
        }
        let h = rgb_to_hsv(ColorBgra::from_rgba(0, 0, 255, 255));
        assert_eq!((h.h, h.s, h.v), (240.0, 1.0, 1.0));
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(
            from_hex("FF8000", 255),
            Some(ColorBgra::from_rgba(255, 128, 0, 255))
        );
        assert_eq!(
            from_hex("#f80", 255),
            Some(ColorBgra::from_rgba(255, 136, 0, 255))
        );
        assert_eq!(
            from_hex("11223344", 255),
            Some(ColorBgra::from_rgba(0x11, 0x22, 0x33, 0x44))
        );
        assert_eq!(from_hex("zz", 255), None);
        assert_eq!(to_hex(ColorBgra::from_rgba(1, 2, 3, 255)), "010203");
        assert_eq!(to_hex(ColorBgra::from_rgba(1, 2, 3, 4)), "01020304");
    }

    #[test]
    fn wheel_modifiers_follow_paint_net() {
        let cur = Hsv {
            h: 100.0,
            s: 0.5,
            v: 0.8,
        };
        let plain = wheel_pick(cur, 37.0, 0.9, false, false, false);
        assert_eq!((plain.h, plain.s, plain.v), (37.0, 0.9, 0.8));
        let ctrl = wheel_pick(cur, 37.0, 0.9, true, false, false);
        assert_eq!((ctrl.h, ctrl.s), (37.0, 0.5), "Ctrl: hue only");
        let alt = wheel_pick(cur, 37.0, 0.9, false, true, false);
        assert_eq!((alt.h, alt.s), (100.0, 0.9), "Alt: saturation only");
        let shift = wheel_pick(cur, 37.0, 0.9, false, false, true);
        assert_eq!(
            (shift.h, shift.s),
            (30.0, 0.9),
            "Shift: saturation, hue snapped to 15°"
        );
        let ctrl_shift = wheel_pick(cur, 37.0, 0.9, true, false, true);
        assert_eq!(
            (ctrl_shift.h, ctrl_shift.s),
            (30.0, 0.5),
            "Ctrl+Shift: hue snapped"
        );
        assert_eq!(wheel_pick(cur, -90.0, 2.0, false, false, false).h, 270.0);
    }

    #[test]
    fn default_palette_starts_like_paint_net() {
        let s = ColorsState::default();
        assert_eq!(s.palette.len(), 96);
        assert_eq!(s.palette[0], ColorBgra::BLACK);
        assert_eq!(s.palette[2], ColorBgra::from_rgba(255, 0, 0, 255));
        assert_eq!(s.palette[16], ColorBgra::WHITE);
    }
}
