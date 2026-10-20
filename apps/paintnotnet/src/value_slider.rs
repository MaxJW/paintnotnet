//! Dialog sliders. paint.net 5's effect dialogs give each setting its name, a slider, a number
//! box and a reset button; here the name, box and reset share the top row and the slider runs
//! the dialog's whole width below it (it had a third of it, ~2 units per pixel on a 0–200
//! range). Made for exact settings: grabbing the knob doesn't move the value, Shift drags ten
//! times finer, the mouse wheel and arrow keys step one unit (Shift: ten), Page Up / Down a
//! tenth of the range, Home / End to the ends, double-click resets, and the box takes typed
//! values. The track fills from zero when the range spans it (Hue −180…180), and a tick marks
//! the default.

use eframe::egui::{self, Align, Color32, Key, Layout, Rect, Sense, Stroke, pos2, vec2};

use crate::theme::{self, Palette};

/// Content width of the effect and document dialogs (the dialog adds its margins).
pub const DIALOG_WIDTH: f32 = 340.0;
/// The number box.
const VALUE_WIDTH: f32 = 78.0;
const ROW_HEIGHT: f32 = 24.0;
const KNOB: f32 = 7.0;
const DIAL: f32 = 44.0;
/// Shift-drag moves the value this much slower.
const FINE: f64 = 0.1;
/// Dragging a compact field this far sweeps its whole range.
const SCRUB_TRAVEL: f32 = 240.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Scale {
    Linear,
    /// More travel for small values: logarithmic when the range starts above zero, cubic
    /// from zero.
    LowEnd,
}

/// A labelled number setting: `ValueSlider::new("Radius", 0.0, 200.0).default(2.0)`.
#[derive(Clone, Copy, Debug)]
pub struct ValueSlider<'a> {
    label: &'a str,
    min: f64,
    max: f64,
    default: Option<f64>,
    decimals: usize,
    suffix: &'a str,
    scale: Scale,
    dial: bool,
    hint: Option<&'a str>,
    /// How the compact field shows the value (else the decimals and suffix).
    formatter: Option<fn(f64) -> String>,
    /// The mouse wheel's next value up or down (else one unit).
    wheel: Option<fn(f64, bool) -> f64>,
}

impl<'a> ValueSlider<'a> {
    pub fn new(label: &'a str, min: f64, max: f64) -> Self {
        Self {
            label,
            min,
            max,
            default: None,
            decimals: 0,
            suffix: "",
            scale: Scale::Linear,
            dial: false,
            hint: None,
            formatter: None,
            wheel: None,
        }
    }

    /// What ↺ and double-click go back to (no reset without one).
    pub fn default(mut self, value: f64) -> Self {
        self.default = Some(value);
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = suffix;
        self
    }

    /// Give small values more of the track (radii up to 250 px, zoom 1/16×–16×).
    pub fn low_end(mut self) -> Self {
        self.scale = Scale::LowEnd;
        self
    }

    /// An angle in degrees: a dial (0° right, counter-clockwise, as paint.net's angle
    /// chooser) beside the slider.
    pub fn dial(mut self) -> Self {
        self.dial = true;
        self
    }

    /// Tooltip on the name.
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Show the value this way in the compact field.
    pub fn formatter(mut self, f: fn(f64) -> String) -> Self {
        self.formatter = Some(f);
        self
    }

    /// The wheel steps through these values instead of one unit at a time.
    pub fn wheel_steps(mut self, next: fn(f64, bool) -> f64) -> Self {
        self.wheel = Some(next);
        self
    }

    /// The toolbar's compact form, "Size 12 px" in one small field: drag sideways on it to
    /// change it (Shift: ten times finer), scroll over it to step it, click it for a pop-over
    /// with the full slider and number box, then `extra` (e.g. size presets). A line along
    /// its foot shows where the value sits in the range.
    pub fn show_compact(
        &self,
        ui: &mut egui::Ui,
        value: &mut f64,
        extra: impl FnOnce(&mut egui::Ui, &mut f64),
    ) -> bool {
        let before = *value;
        let palette = theme::palette(ui.ctx());
        let font = egui::FontId::proportional(theme::text::BODY);
        let fonts = ui.ctx().clone();
        let layout = |text: String, color: Color32| {
            fonts.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), color))
        };
        let name = layout(self.label.to_owned(), palette.weak_text);
        // Room for the widest value, so the field doesn't change width while dragged.
        let widest = [self.min, self.max, *value]
            .map(|v| layout(self.format(v), palette.text).size().x)
            .into_iter()
            .fold(0.0, f32::max);
        let size = vec2(10.0 + name.size().x + 8.0 + widest + 6.0 + 8.0 + 10.0, 28.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let response = response
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text(match self.hint {
                Some(h) => format!("{h}\nDrag sideways (Shift: finer), scroll, or click"),
                None => "Drag sideways (Shift: finer), scroll, or click".to_owned(),
            });

        // Dragging: relative from where it started, in track terms (so whole units aren't
        // lost to rounding between frames); Shift slows it, from where it was pressed.
        let grab_id = response.id.with("scrub");
        if response.dragged()
            && let Some(p) = ui.ctx().pointer_interact_pos()
        {
            let fine = ui.input(|i| i.modifiers.shift);
            let held = ui.data(|d| d.get_temp::<Grab>(grab_id));
            // A drag is reported once the pointer has moved a little: measure the first one
            // from where the button went down, so that bit isn't lost.
            let from = match held {
                None => ui.input(|i| i.pointer.press_origin()).map_or(p.x, |o| o.x),
                Some(_) => p.x,
            };
            let g = held.filter(|g| g.fine == fine).unwrap_or(Grab {
                x: from,
                t: self.track_at(*value),
                fine,
            });
            let gain = if fine { FINE } else { 1.0 };
            let t = g.t + f64::from(p.x - g.x) / f64::from(SCRUB_TRAVEL) * gain;
            *value = self.tidy(self.value_at(t));
            ui.data_mut(|d| d.insert_temp(grab_id, g));
        } else {
            ui.data_mut(|d| d.remove::<Grab>(grab_id));
        }
        if response.hovered() {
            let notches = wheel_notches(ui);
            if notches != 0.0 {
                let up = notches > 0.0;
                *value = match self.wheel {
                    Some(next) => self.tidy(next(*value, up)),
                    None => self.tidy(*value + notches * self.step()),
                };
            }
        }
        egui::Popup::from_toggle_button_response(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .gap(6.0)
            .show(|ui| {
                ui.set_width(280.0);
                self.show(ui, value);
                extra(ui, value);
            });
        let shown = *value;
        response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), shown, self.label));

        // Paint.
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        // Filled as the toolbar's buttons are.
        let widgets = &ui.visuals().widgets;
        let fill = if response.hovered() || response.dragged() || open {
            widgets.hovered.weak_bg_fill
        } else {
            widgets.inactive.weak_bg_fill
        };
        let painter = ui.painter();
        painter.rect_filled(rect, theme::radius::CONTROL, fill);
        let y = rect.center().y;
        let mut x = rect.min.x + 10.0;
        let name_w = name.size().x;
        painter.galley(pos2(x, y - name.size().y / 2.0), name, palette.weak_text);
        x += name_w + 8.0;
        let text = layout(self.format(*value), palette.text);
        painter.galley(pos2(x, y - text.size().y / 2.0), text, palette.text);
        // ▾
        let cx = rect.max.x - 14.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(cx - 3.5, y - 1.5),
                pos2(cx + 3.5, y - 1.5),
                pos2(cx, y + 2.5),
            ],
            palette.weak_text,
            Stroke::NONE,
        ));
        // Where the value sits: a line along the foot, from the range's origin.
        let foot = Rect::from_min_max(
            pos2(rect.min.x + 6.0, rect.max.y - 2.5),
            pos2(rect.max.x - 6.0, rect.max.y - 1.0),
        );
        let x_of = |t: f64| foot.min.x + foot.width() * t as f32;
        let (t, o) = (self.track_at(*value), self.track_at(self.origin()));
        painter.rect_filled(
            Rect::from_min_max(
                pos2(x_of(t.min(o)), foot.min.y),
                pos2(x_of(t.max(o)), foot.max.y),
            ),
            1.0,
            palette
                .accent
                .gamma_multiply(if ui.is_enabled() { 0.6 } else { 0.25 }),
        );
        *value != before
    }

    /// The value as the compact field shows it.
    fn format(&self, v: f64) -> String {
        match self.formatter {
            Some(f) => f(v),
            None => format!("{v:.*}{}", self.decimals, self.suffix),
        }
    }

    /// Name, box and reset, then the slider (and dial) across the available width. Returns
    /// whether the value changed.
    pub fn show<N: egui::emath::Numeric>(&self, ui: &mut egui::Ui, value: &mut N) -> bool {
        let spec = self.for_type::<N>();
        let mut v = value.to_f64();
        let changed = ui.push_id(self.label, |ui| spec.show_f64(ui, &mut v)).inner;
        if changed {
            *value = N::from_f64(v);
        }
        changed
    }

    /// Slider and box only, `width` wide, for a form whose label column names it.
    pub fn show_inline<N: egui::emath::Numeric>(
        &self,
        ui: &mut egui::Ui,
        value: &mut N,
        width: f32,
    ) -> bool {
        let spec = self.for_type::<N>();
        let mut v = value.to_f64();
        let palette = theme::palette(ui.ctx());
        let before = v;
        ui.push_id(self.label, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::space::M;
                let track = width - VALUE_WIDTH - theme::space::M;
                spec.slider(ui, &mut v, track, &palette);
                spec.number_box(ui, &mut v);
            });
        });
        v = spec.tidy(v);
        let changed = v != before;
        if changed {
            *value = N::from_f64(v);
        }
        changed
    }

    fn for_type<N: egui::emath::Numeric>(&self) -> Self {
        let mut spec = *self;
        if N::INTEGRAL {
            spec.decimals = 0;
        }
        spec
    }

    fn show_f64(&self, ui: &mut egui::Ui, v: &mut f64) -> bool {
        let before = *v;
        // The name sits close above its slider; the dialog's spacing separates settings.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.rows(ui, v);
        });
        *v = self.tidy(*v);
        *v != before
    }

    fn rows(&self, ui: &mut egui::Ui, v: &mut f64) {
        let palette = theme::palette(ui.ctx());
        let width = ui.available_width();
        ui.horizontal(|ui| {
            ui.set_width(width);
            let name = ui.label(self.label);
            if let Some(hint) = self.hint {
                name.on_hover_text(hint);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = theme::space::S;
                if let Some(d) = self.default {
                    self.reset_button(ui, v, d, &palette);
                }
                self.number_box(ui, v);
            });
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::M;
            let mut track = width;
            if self.dial {
                self.dial_ui(ui, v, &palette);
                track -= DIAL + theme::space::M;
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                self.slider(ui, v, track, &palette);
            });
        });
    }

    /// One unit of the shown precision.
    fn step(&self) -> f64 {
        10f64.powi(-(self.decimals as i32))
    }

    /// Clamped and rounded to the shown precision.
    fn tidy(&self, v: f64) -> f64 {
        let p = 10f64.powi(self.decimals as i32);
        ((v * p).round() / p).clamp(self.min, self.max)
    }

    /// Track position 0–1 of `v`.
    fn track_at(&self, v: f64) -> f64 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        let v = v.clamp(self.min, self.max);
        match self.scale {
            Scale::Linear => (v - self.min) / span,
            Scale::LowEnd if self.min > 0.0 => (v / self.min).ln() / (self.max / self.min).ln(),
            Scale::LowEnd => ((v - self.min) / span).cbrt(),
        }
    }

    fn value_at(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let span = self.max - self.min;
        match self.scale {
            Scale::Linear => self.min + span * t,
            Scale::LowEnd if self.min > 0.0 => self.min * (self.max / self.min).powf(t),
            Scale::LowEnd => self.min + span * t.powi(3),
        }
    }

    /// Where the fill starts: zero inside the range, else its low end.
    fn origin(&self) -> f64 {
        if self.min < 0.0 && self.max > 0.0 {
            0.0
        } else {
            self.min
        }
    }

    fn number_box(&self, ui: &mut egui::Ui, v: &mut f64) {
        // Dragging the box covers the range in ~400 points; typing sets it exactly.
        let speed = ((self.max - self.min) / 400.0).max(self.step() / 4.0);
        let field = egui::DragValue::new(v)
            .range(self.min..=self.max)
            .min_decimals(self.decimals)
            .max_decimals(self.decimals)
            .suffix(self.suffix)
            .speed(speed);
        ui.add_sized(vec2(VALUE_WIDTH, ROW_HEIGHT - 2.0), field)
            .on_hover_text("Type a value, or drag");
    }

    fn reset_button(&self, ui: &mut egui::Ui, v: &mut f64, default: f64, palette: &Palette) {
        let at_default = (*v - default).abs() < self.step() / 2.0;
        let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
        let response = response.on_hover_text("Reset (or double-click the slider)");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Reset")
        });
        if response.clicked() {
            *v = default;
        }
        if response.hovered() && !at_default {
            ui.painter()
                .rect_filled(rect, theme::radius::SMALL, palette.hover);
        }
        let color = if at_default || !ui.is_enabled() {
            palette.weak_text.gamma_multiply(0.45)
        } else {
            palette.weak_text
        };
        paint_reset(ui.painter(), rect.center(), color);
    }

    fn slider(&self, ui: &mut egui::Ui, v: &mut f64, width: f32, palette: &Palette) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(width.max(40.0), ROW_HEIGHT), Sense::click_and_drag());
        let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 2.0 * KNOB, 4.0));
        let x_of = |t: f64| rail.min.x + rail.width() * t as f32;
        let t_of = |x: f32| f64::from((x - rail.min.x) / rail.width());
        let grab_id = response.id.with("grab");

        // Pointer: a press on the knob keeps the value until it moves; a press on the track
        // jumps there. Either way the drag is relative from then on, so Shift can slow it.
        if response.is_pointer_button_down_on()
            && let Some(p) = ui.ctx().pointer_interact_pos()
        {
            let fine = ui.input(|i| i.modifiers.shift);
            let mut grab = ui.data(|d| d.get_temp::<Grab>(grab_id));
            if grab.is_none() {
                let t = self.track_at(*v);
                let on_knob = (p.x - x_of(t)).abs() <= KNOB + 3.0;
                let t = if on_knob {
                    t
                } else {
                    t_of(p.x).clamp(0.0, 1.0)
                };
                grab = Some(Grab { x: p.x, t, fine });
                response.request_focus();
            }
            let mut g = grab.expect("set above");
            if g.fine != fine {
                // Shift pressed or released mid-drag: carry on from here.
                g = Grab {
                    x: p.x,
                    t: self.track_at(*v),
                    fine,
                };
            }
            let gain = if fine { FINE } else { 1.0 };
            let t = g.t + f64::from(p.x - g.x) / f64::from(rail.width()) * gain;
            *v = self.tidy(self.value_at(t));
            ui.data_mut(|d| d.insert_temp(grab_id, g));
        } else {
            ui.data_mut(|d| d.remove::<Grab>(grab_id));
        }
        // A quick third click counts as a triple: reset on that too.
        if (response.double_clicked() || response.triple_clicked())
            && let Some(d) = self.default
        {
            *v = d;
        }
        if response.hovered() {
            let notches = wheel_notches(ui);
            if notches != 0.0 {
                let shift = ui.input(|i| i.modifiers.shift);
                let n = if shift { 10.0 } else { 1.0 };
                *v = self.tidy(*v + notches * n * self.step());
            }
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
            *v = self.keys(ui, *v);
        }
        let shown = *v;
        response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), shown, self.label));

        // Paint.
        let enabled = ui.is_enabled();
        let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
        let painter = ui.painter();
        painter.rect_filled(rail, 2.0, fade(palette.control));
        let (t, o) = (self.track_at(*v), self.track_at(self.origin()));
        let (a, b) = (x_of(t.min(o)), x_of(t.max(o)));
        painter.rect_filled(
            Rect::from_min_max(pos2(a, rail.min.y), pos2(b, rail.max.y)),
            2.0,
            fade(palette.accent),
        );
        if let Some(d) = self.default
            && (self.track_at(d) - o).abs() > 1e-6
        {
            let x = x_of(self.track_at(d));
            painter.line_segment(
                [pos2(x, rail.min.y - 4.0), pos2(x, rail.max.y + 4.0)],
                Stroke::new(1.5, fade(palette.weak_text.gamma_multiply(0.6))),
            );
        }
        let active = enabled && (response.hovered() || response.dragged() || response.has_focus());
        let halo =
            ui.ctx()
                .animate_bool_with_time(response.id.with("halo"), active, theme::motion::FAST);
        let c = pos2(x_of(t), rail.center().y);
        if halo > 0.0 {
            painter.circle_filled(c, KNOB + 3.0 * halo, palette.accent_alpha(0.35 * halo));
        }
        painter.circle(
            c,
            KNOB,
            fade(Color32::from_rgb(0xf2, 0xf4, 0xf7)),
            Stroke::new(1.0, Color32::from_black_alpha(70)),
        );
    }

    /// Arrow keys, Page Up / Down, Home / End on the focused slider.
    fn keys(&self, ui: &egui::Ui, v: f64) -> f64 {
        let step = self.step();
        let page = ((self.max - self.min) / 10.0).max(step);
        ui.input(|i| {
            let mut v = v;
            for e in &i.events {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = e
                else {
                    continue;
                };
                let n = if modifiers.shift { 10.0 } else { 1.0 };
                v = match key {
                    Key::ArrowRight | Key::ArrowUp => v + n * step,
                    Key::ArrowLeft | Key::ArrowDown => v - n * step,
                    Key::PageUp => v + page,
                    Key::PageDown => v - page,
                    Key::Home => self.min,
                    Key::End => self.max,
                    _ => continue,
                };
                v = self.tidy(v);
            }
            v
        })
    }

    /// The angle dial: drag to point it (Shift: 15° steps), double-click to reset.
    fn dial_ui(&self, ui: &mut egui::Ui, deg: &mut f64, palette: &Palette) {
        let (rect, response) = ui.allocate_exact_size(vec2(DIAL, DIAL), Sense::click_and_drag());
        let c = rect.center();
        if response.is_pointer_button_down_on()
            && let Some(p) = ui.ctx().pointer_interact_pos()
            && (p - c).length() > 3.0
        {
            let d = p - c;
            let mut a = f64::from((-d.y).atan2(d.x).to_degrees());
            if ui.input(|i| i.modifiers.shift) {
                a = (a / 15.0).round() * 15.0;
            }
            *deg = self.tidy(wrap_into(a, self.min, self.max));
        }
        // A quick third click counts as a triple: reset on that too.
        if (response.double_clicked() || response.triple_clicked())
            && let Some(d) = self.default
        {
            *deg = d;
        }
        let shown = *deg;
        response.widget_info(|| {
            egui::WidgetInfo::slider(ui.is_enabled(), shown, format!("{} dial", self.label))
        });
        let enabled = ui.is_enabled();
        let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
        let r = DIAL / 2.0 - 1.0;
        let painter = ui.painter();
        painter.circle(c, r, fade(palette.control), Stroke::NONE);
        // Quarter marks.
        for q in 0..4 {
            let a = q as f32 * std::f32::consts::FRAC_PI_2;
            let dir = vec2(a.cos(), -a.sin());
            painter.line_segment(
                [c + dir * (r - 4.0), c + dir * (r - 1.5)],
                Stroke::new(1.0, fade(palette.weak_text.gamma_multiply(0.6))),
            );
        }
        let a = (*deg as f32).to_radians();
        let dir = vec2(a.cos(), -a.sin());
        let tip = c + dir * (r - 7.0);
        painter.line_segment([c, tip], Stroke::new(2.0, fade(palette.accent)));
        painter.circle_filled(c, 2.5, fade(palette.accent));
        let active = enabled && (response.hovered() || response.dragged());
        let halo =
            ui.ctx()
                .animate_bool_with_time(response.id.with("halo"), active, theme::motion::FAST);
        if halo > 0.0 {
            painter.circle_filled(tip, 5.0 + 2.0 * halo, palette.accent_alpha(0.35 * halo));
        }
        painter.circle(
            tip,
            5.0,
            fade(Color32::from_rgb(0xf2, 0xf4, 0xf7)),
            Stroke::new(1.0, Color32::from_black_alpha(70)),
        );
    }
}

/// A drag in progress: where it started (or Shift last changed) and the track position then.
#[derive(Clone, Copy, Debug)]
struct Grab {
    x: f32,
    t: f64,
    fine: bool,
}

/// Mouse wheel notches this frame (+ = up), from raw wheel events so one notch is one step.
fn wheel_notches(ui: &egui::Ui) -> f64 {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { delta, .. } => {
                    let d = if delta.y != 0.0 { delta.y } else { delta.x };
                    Some(f64::from(d.signum()))
                }
                _ => None,
            })
            .sum()
    })
}

/// `deg` moved by whole turns into `min..=max` (e.g. 0..360 or −180..180).
fn wrap_into(mut deg: f64, min: f64, max: f64) -> f64 {
    while deg < min {
        deg += 360.0;
    }
    while deg > max {
        deg -= 360.0;
    }
    deg.clamp(min, max)
}

/// ↺: an open circle with an arrowhead at its top-left end (Lucide's rotate-ccw).
fn paint_reset(painter: &egui::Painter, c: egui::Pos2, color: Color32) {
    let r = 5.5;
    let stroke = Stroke::new(1.5, color);
    // From the top (−90° in screen terms) round clockwise to just short of it.
    let points: Vec<egui::Pos2> = (0..=20)
        .map(|i| {
            let a = (-90.0 + 290.0 * i as f32 / 20.0).to_radians();
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    let start = points[0];
    painter.add(egui::Shape::line(points, stroke));
    // Arrowhead at the start, pointing back the way the arc runs (counter-clockwise).
    painter.line_segment([start, start + vec2(3.5, -2.5)], stroke);
    painter.line_segment([start, start + vec2(3.5, 2.5)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_positions_round_trip() {
        for s in [
            ValueSlider::new("a", 0.0, 200.0),
            ValueSlider::new("b", -180.0, 180.0),
            ValueSlider::new("c", 0.0, 250.0).low_end(),
            ValueSlider::new("d", 1.0 / 16.0, 16.0).low_end(),
        ] {
            for i in 0..=20 {
                let t = i as f64 / 20.0;
                assert!((s.track_at(s.value_at(t)) - t).abs() < 1e-9, "{s:?} at {t}");
            }
            assert_eq!(s.value_at(0.0), s.min);
            assert!((s.value_at(1.0) - s.max).abs() < 1e-9);
        }
    }

    #[test]
    fn low_end_gives_small_values_room() {
        let radius = ValueSlider::new("r", 0.0, 250.0).low_end();
        assert!(radius.value_at(0.5) < 40.0, "half the track under 40 px");
        let zoom = ValueSlider::new("z", 1.0 / 16.0, 16.0).low_end();
        assert!((zoom.value_at(0.5) - 1.0).abs() < 1e-9, "1× in the middle");
    }

    #[test]
    fn values_snap_to_the_shown_precision() {
        let s = ValueSlider::new("a", -180.0, 180.0).decimals(2);
        assert_eq!(s.tidy(45.678_9), 45.68);
        assert_eq!(s.tidy(400.0), 180.0);
        assert_eq!(ValueSlider::new("i", 0.0, 100.0).tidy(41.6), 42.0);
    }

    #[test]
    fn fill_starts_at_zero_inside_the_range() {
        assert_eq!(ValueSlider::new("h", -180.0, 180.0).origin(), 0.0);
        assert_eq!(ValueSlider::new("r", 1.0, 100.0).origin(), 1.0);
    }

    #[test]
    fn angles_wrap_into_the_range() {
        assert_eq!(wrap_into(-90.0, 0.0, 360.0), 270.0);
        assert_eq!(wrap_into(270.0, -180.0, 180.0), -90.0);
    }
}
