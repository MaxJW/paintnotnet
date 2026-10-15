//! The docked shell (design A, 2026-10-14): panels are islands floating over a canvas that
//! fills the window.
//!
//! - Tool rail: top-left, one column of tools in groups (two columns when the window is
//!   too short for one).
//! - Tool options bar: along the top, between the rail and the dock.
//! - Right dock: Colors, Layers and History stacked. Its width is dragged from its left
//!   edge, History's height from the gap above it, and each section collapses to its title.
//!
//! The canvas fits and centres within the area the islands leave free ([`Layout::free`]) and
//! can still scroll underneath them. F5–F8 show and hide the panels as in paint.net;
//! Ctrl+Shift+F5–F8 restore a panel's original size. The layout is saved in the settings.

use eframe::egui::{self, Rect, Sense, Vec2, pos2, vec2};

use crate::commands::Panel;
use crate::config::Dock;
use crate::theme::{self, Palette};

/// Gap between the islands and between them and the free canvas area.
pub const GAP: f32 = 16.0;
/// Gap between the window's edges and the islands (user, 2026-10-15: less than between them).
pub const EDGE: f32 = 8.0;
/// Gap between the sections stacked in the dock.
pub const STACK_GAP: f32 = 12.0;
/// Height of a dock section's title row.
pub const HEADER: f32 = 40.0;
const MIN_DOCK_WIDTH: f32 = 240.0;
const MAX_DOCK_WIDTH: f32 = 520.0;
/// Smallest height of a section's body below its title.
const MIN_BODY: f32 = 72.0;
const DEFAULT_COLORS_HEIGHT: f32 = 330.0;
const DEFAULT_OPTIONS_HEIGHT: f32 = 48.0;
const DEFAULT_RAIL: Vec2 = vec2(52.0, 700.0);

/// Order of the sections in the dock, top to bottom.
pub const DOCK_ORDER: [Panel; 3] = [Panel::Colors, Panel::Layers, Panel::History];

/// Which part of a dock section `Panels::show_dock` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Buttons at the right of the title row (laid out right to left).
    Actions,
    /// The section's contents.
    Body,
}

/// Where everything goes this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub rail: Option<Rect>,
    pub options: Rect,
    pub dock: Option<Rect>,
    /// Visible dock sections, top to bottom.
    pub sections: Vec<(Panel, Rect)>,
    /// The canvas area the islands leave free: fitting and centring happen here.
    pub free: Rect,
}

/// The measured sizes and settings the layout depends on.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutInput {
    pub visible: [bool; 4],
    pub collapsed: [bool; 4],
    pub rail: Vec2,
    pub options_height: f32,
    pub dock_width: f32,
    pub colors_height: f32,
    pub history_height: f32,
}

fn idx(p: Panel) -> usize {
    match p {
        Panel::Tools => 0,
        Panel::History => 1,
        Panel::Layers => 2,
        Panel::Colors => 3,
    }
}

/// Lay the islands out in `central` (the area between the title bar and the status bar).
pub fn compute(central: Rect, input: &LayoutInput) -> Layout {
    let inner = central.shrink(EDGE);
    let shown = |p: Panel| input.visible[idx(p)];
    let rail = shown(Panel::Tools).then(|| {
        Rect::from_min_size(
            inner.min,
            vec2(input.rail.x, input.rail.y.min(inner.height())),
        )
    });
    let dock_panels: Vec<Panel> = DOCK_ORDER.into_iter().filter(|&p| shown(p)).collect();
    let dock = (!dock_panels.is_empty()).then(|| {
        let w = input
            .dock_width
            .clamp(MIN_DOCK_WIDTH, MAX_DOCK_WIDTH)
            .min(inner.width() * 0.5);
        Rect::from_min_max(pos2(inner.max.x - w, inner.min.y), inner.max)
    });
    let left = rail.map_or(inner.min.x, |r| r.max.x + GAP);
    let right = dock.map_or(inner.max.x, |d| d.min.x - GAP);
    let options = Rect::from_min_max(
        pos2(left, inner.min.y),
        pos2(right.max(left), inner.min.y + input.options_height),
    );
    let free = Rect::from_min_max(
        pos2(left, options.max.y + GAP),
        pos2(right.max(left), inner.max.y.max(options.max.y + GAP)),
    );

    let mut sections = Vec::new();
    if let Some(dock) = dock {
        let expanded = |p: Panel| !input.collapsed[idx(p)];
        // Layers takes the room that's left; without it, History does.
        let flexible = [Panel::Layers, Panel::History]
            .into_iter()
            .find(|&p| shown(p) && expanded(p));
        let mut heights: Vec<(Panel, f32)> = dock_panels
            .iter()
            .map(|&p| {
                let h = if !expanded(p) {
                    HEADER
                } else {
                    match p {
                        Panel::Colors => input.colors_height,
                        Panel::History => input.history_height,
                        _ => HEADER + MIN_BODY,
                    }
                };
                (p, h.max(HEADER))
            })
            .collect();
        let gaps = STACK_GAP * (heights.len() as f32 - 1.0);
        if let Some(flex) = flexible {
            let others: f32 = heights
                .iter()
                .filter(|(p, _)| *p != flex)
                .map(|(_, h)| h)
                .sum();
            let mut room = dock.height() - gaps - others;
            // Too little room for the flexible section: take it from History first.
            if room < HEADER + MIN_BODY
                && flex == Panel::Layers
                && let Some(h) = heights.iter_mut().find(|(p, _)| *p == Panel::History)
                && h.1 > HEADER
            {
                let give = (HEADER + MIN_BODY - room).min(h.1 - (HEADER + MIN_BODY));
                if give > 0.0 {
                    h.1 -= give;
                    room += give;
                }
            }
            if let Some(h) = heights.iter_mut().find(|(p, _)| *p == flex) {
                h.1 = room.max(HEADER + MIN_BODY);
            }
        }
        let mut y = dock.min.y;
        for (p, h) in heights {
            sections.push((
                p,
                Rect::from_min_size(pos2(dock.min.x, y), vec2(dock.width(), h)),
            ));
            y += h + STACK_GAP;
        }
    }
    Layout {
        rail,
        options,
        dock,
        sections,
        free,
    }
}

pub struct Panels {
    visible: [bool; 4],
    collapsed: [bool; 4],
    dock_width: f32,
    history_height: f32,
    /// Sizes measured from last frame's contents.
    rail: Vec2,
    options_height: f32,
    colors_height: f32,
    /// This frame's layout as drawn (animated); its `free` area is the final one.
    layout: Layout,
    /// Where each panel was last drawn (a hidden panel fades out there).
    last: [Rect; 4],
    /// A panel was shown, hidden or collapsed: animate the layout until this time.
    kick: bool,
    animate_until: f64,
    animating: bool,
    /// The layout settings changed (save them).
    changed: bool,
}

impl Default for Panels {
    fn default() -> Self {
        let dock = Dock::default();
        Self {
            visible: [true; 4],
            collapsed: [false; 4],
            dock_width: dock.width,
            history_height: dock.history_height,
            rail: DEFAULT_RAIL,
            options_height: DEFAULT_OPTIONS_HEIGHT,
            colors_height: DEFAULT_COLORS_HEIGHT,
            layout: Layout {
                rail: None,
                options: Rect::NOTHING,
                dock: None,
                sections: Vec::new(),
                free: Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0)),
            },
            last: [Rect::NOTHING; 4],
            kick: false,
            animate_until: 0.0,
            animating: false,
            changed: false,
        }
    }
}

impl Panels {
    pub fn visible(&self, panel: Panel) -> bool {
        self.visible[idx(panel)]
    }

    pub fn toggle(&mut self, panel: Panel) {
        self.visible[idx(panel)] = !self.visible[idx(panel)];
        self.changed = true;
        self.kick = true;
    }

    pub fn set_visible(&mut self, panel: Panel, visible: bool) {
        self.visible[idx(panel)] = visible;
        self.changed = true;
        self.kick = true;
    }

    pub fn collapsed(&self, panel: Panel) -> bool {
        self.collapsed[idx(panel)]
    }

    pub fn set_collapsed(&mut self, panel: Panel, collapsed: bool) {
        self.collapsed[idx(panel)] = collapsed;
        self.changed = true;
        self.kick = true;
    }

    /// Ctrl+Shift+F5…F8: shown, expanded and at its original size.
    pub fn reset(&mut self, panel: Panel) {
        let d = Dock::default();
        self.visible[idx(panel)] = true;
        self.collapsed[idx(panel)] = false;
        match panel {
            Panel::Tools => {}
            Panel::History => self.history_height = d.history_height,
            Panel::Layers | Panel::Colors => self.dock_width = d.width,
        }
        self.changed = true;
        self.kick = true;
    }

    /// Where `panel` was drawn last frame (`Rect::NOTHING` when hidden).
    pub fn rect(&self, panel: Panel) -> Rect {
        if !self.visible(panel) {
            return Rect::NOTHING;
        }
        match panel {
            Panel::Tools => self.layout.rail.unwrap_or(Rect::NOTHING),
            p => self
                .layout
                .sections
                .iter()
                .find(|(q, _)| *q == p)
                .map_or(Rect::NOTHING, |(_, r)| *r),
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn dock_width(&self) -> f32 {
        self.dock_width
    }

    /// Restore the layout saved in the settings.
    pub fn load(&mut self, dock: &Dock) {
        self.dock_width = dock.width.clamp(MIN_DOCK_WIDTH, MAX_DOCK_WIDTH);
        self.history_height = dock.history_height.max(HEADER + MIN_BODY);
        for p in Panel::ALL {
            self.visible[idx(p)] = !dock.hidden.iter().any(|n| n == p.name());
            self.collapsed[idx(p)] = dock.collapsed.iter().any(|n| n == p.name());
        }
        self.changed = false;
    }

    /// The layout to save in the settings.
    pub fn to_dock(&self) -> Dock {
        let names = |f: &dyn Fn(Panel) -> bool| {
            Panel::ALL
                .into_iter()
                .filter(|&p| f(p))
                .map(|p| p.name().to_owned())
                .collect()
        };
        Dock {
            width: self.dock_width,
            history_height: self.history_height,
            hidden: names(&|p| !self.visible(p)),
            collapsed: names(&|p| self.collapsed(p)),
        }
    }

    /// Whether the layout settings changed since the last call.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Lay out the islands in `central` for this frame; returns the free canvas area.
    ///
    /// After a panel is shown, hidden or collapsed the islands glide to their new places
    /// (`motion::NORMAL`); window resizes and splitter drags follow at once, and the free
    /// area (where the canvas fits) never animates.
    pub fn arrange(&mut self, ctx: &egui::Context, central: Rect) -> Rect {
        let input = LayoutInput {
            visible: self.visible,
            collapsed: self.collapsed,
            rail: self.rail,
            options_height: self.options_height,
            dock_width: self.dock_width,
            colors_height: self.colors_height,
            history_height: self.history_height,
        };
        let target = compute(central, &input);
        let now = ctx.input(|i| i.time);
        if std::mem::take(&mut self.kick) {
            self.animate_until = now + f64::from(theme::motion::NORMAL) + 0.05;
        }
        self.animating = now < self.animate_until;
        if self.animating {
            ctx.request_repaint();
        }
        let time = if self.animating {
            theme::motion::NORMAL
        } else {
            0.0
        };
        let anim = |key: (&str, &str), v: f32| {
            ctx.animate_value_with_time(egui::Id::new(("panel_anim", key)), v, time)
        };
        let mut layout = target;
        let o = layout.options;
        layout.options = Rect::from_min_max(
            pos2(anim(("options", "x0"), o.min.x), o.min.y),
            pos2(anim(("options", "x1"), o.max.x), o.max.y),
        );
        for (p, r) in &mut layout.sections {
            let y = anim((p.name(), "y"), r.min.y);
            let h = anim((p.name(), "h"), r.height());
            *r = Rect::from_min_size(pos2(r.min.x, y), vec2(r.width(), h));
        }
        self.layout = layout;
        self.layout.free
    }

    /// How visible `panel` is (0–1): fades in when shown, out when hidden.
    fn fade(&self, ctx: &egui::Context, panel: Panel) -> f32 {
        ctx.animate_bool_with_time(
            egui::Id::new(("panel_fade", panel.name())),
            self.visible(panel),
            theme::motion::NORMAL,
        )
    }

    /// The tool rail. `contents` gets the height available for the tools.
    pub fn show_rail(
        &mut self,
        ctx: &egui::Context,
        palette: &Palette,
        contents: impl FnOnce(&mut egui::Ui, f32),
    ) {
        let fade = self.fade(ctx, Panel::Tools);
        let Some(rail) = self
            .layout
            .rail
            .or_else(|| (fade > 0.0).then_some(self.last[0]))
        else {
            return;
        };
        let visible = self.visible(Panel::Tools);
        let max_height = self.layout.free.max.y - rail.min.y;
        let margin = egui::Margin::symmetric(10, 10);
        let response = egui::Area::new(egui::Id::new("tool_rail"))
            .fixed_pos(rail.min - vec2((1.0 - fade) * 8.0, 0.0))
            .constrain(false)
            .interactable(visible)
            .show(ctx, |ui| {
                ui.multiply_opacity(fade);
                crate::glass::show(ui, island(palette).inner_margin(margin), "rail", |ui| {
                    contents(ui, max_height - margin.sum().y);
                })
            });
        if !visible {
            return;
        }
        self.last[0] = response.response.rect;
        let size = response.response.rect.size();
        if (size - self.rail).length() > 0.5 {
            self.rail = size;
            ctx.request_repaint();
        }
    }

    /// The tool options bar.
    pub fn show_options(
        &mut self,
        ctx: &egui::Context,
        palette: &Palette,
        contents: impl FnOnce(&mut egui::Ui),
    ) {
        let rect = self.layout.options;
        if rect.width() < 40.0 {
            return;
        }
        let margin = egui::Margin::symmetric(14, 10);
        let response = egui::Area::new(egui::Id::new("tool_options"))
            .fixed_pos(rect.min)
            .constrain(false)
            .show(ctx, |ui| {
                crate::glass::show(ui, island(palette).inner_margin(margin), "options", |ui| {
                    let w = rect.width() - margin.sum().x;
                    ui.set_width(w);
                    ui.set_max_width(w);
                    ui.set_min_height(theme::size::CONTROL + 6.0);
                    contents(ui);
                })
            });
        let h = response.response.rect.height();
        if (h - self.options_height).abs() > 0.5 {
            self.options_height = h;
            ctx.request_repaint();
        }
    }

    /// The right dock's sections. `contents` draws a section's title-row buttons or body.
    pub fn show_dock(
        &mut self,
        ctx: &egui::Context,
        palette: &Palette,
        mut contents: impl FnMut(Panel, Part, &mut egui::Ui),
    ) {
        let sections = self.layout.sections.clone();
        for panel in DOCK_ORDER {
            let fade = self.fade(ctx, panel);
            let visible = self.visible(panel);
            let rect = match sections.iter().find(|(p, _)| *p == panel) {
                Some((_, r)) => *r,
                None if fade > 0.0 => self.last[idx(panel)],
                None => continue,
            };
            let collapsed = self.collapsed(panel);
            let animating = self.animating;
            let mut toggle = false;
            let response = egui::Area::new(egui::Id::new(("dock", panel.name())))
                .fixed_pos(rect.min + vec2((1.0 - fade) * 8.0, 0.0))
                .constrain(false)
                .interactable(visible)
                .show(ctx, |ui| {
                    ui.multiply_opacity(fade);
                    if animating {
                        // Contents appear as the section opens (the shadow may clip meanwhile).
                        let clip = Rect::from_min_max(
                            rect.min - vec2(GAP * 3.0, GAP * 3.0),
                            pos2(rect.max.x + GAP * 3.0, rect.max.y),
                        );
                        ui.set_clip_rect(clip);
                    }
                    crate::glass::show(ui, island(palette), ("dock", panel.name()), |ui| {
                        ui.set_width(rect.width());
                        // Colors sizes itself to its contents; the others fill their slot.
                        if panel != Panel::Colors || collapsed {
                            ui.set_height(rect.height());
                        }
                        ui.spacing_mut().item_spacing.y = 0.0;
                        toggle = header(ui, panel, collapsed, palette, &mut contents);
                        if !collapsed {
                            let margin = if panel == Panel::Colors {
                                egui::Margin {
                                    left: 12,
                                    right: 12,
                                    top: 0,
                                    bottom: 12,
                                }
                            } else {
                                egui::Margin {
                                    left: 10,
                                    right: 10,
                                    top: 0,
                                    bottom: 12,
                                }
                            };
                            egui::Frame::new().inner_margin(margin).show(ui, |ui| {
                                // Colors is tall already: its rows stay as compact as they
                                // were, so Layers and History keep their room.
                                if panel == Panel::Colors {
                                    ui.spacing_mut().item_spacing.y = theme::space::S + 2.0;
                                    ui.spacing_mut().interact_size.y = 26.0;
                                } else {
                                    ui.spacing_mut().item_spacing.y = theme::space::M;
                                }
                                ui.set_width(ui.available_width());
                                contents(panel, Part::Body, ui)
                            });
                        }
                    })
                });
            if toggle {
                self.set_collapsed(panel, !collapsed);
            }
            if !visible {
                continue;
            }
            self.last[idx(panel)] = rect;
            if panel == Panel::Colors && !collapsed && !animating {
                let h = response.response.rect.height();
                if (h - self.colors_height).abs() > 0.5 {
                    self.colors_height = h;
                    ctx.request_repaint();
                }
            }
        }
        if let Some(dock) = self.layout.dock {
            self.splitters(ctx, dock, &sections);
        }
    }

    /// Drag handles: the dock's left edge (width) and the gap above History (its height).
    fn splitters(&mut self, ctx: &egui::Context, dock: Rect, sections: &[(Panel, Rect)]) {
        egui::Area::new(egui::Id::new("dock_splitters"))
            .fixed_pos(dock.min - vec2(GAP, 0.0))
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let edge = Rect::from_min_max(
                    pos2(dock.min.x - 5.0, dock.min.y),
                    pos2(dock.min.x + 2.0, dock.max.y),
                );
                let r = ui.interact(edge, ui.id().with("width"), Sense::drag());
                if r.hovered() || r.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if r.dragged() {
                    self.dock_width =
                        (self.dock_width - r.drag_delta().x).clamp(MIN_DOCK_WIDTH, MAX_DOCK_WIDTH);
                }
                if r.drag_stopped() {
                    self.changed = true;
                }
                let history = sections.iter().position(|(p, _)| *p == Panel::History);
                if let Some(i) = history
                    && i > 0
                    && !self.collapsed(Panel::History)
                    && !self.collapsed(sections[i - 1].0)
                {
                    let above = sections[i - 1].1;
                    let gap = Rect::from_min_max(
                        pos2(dock.min.x, above.max.y),
                        pos2(dock.max.x, sections[i].1.min.y),
                    );
                    let r = ui.interact(gap, ui.id().with("history"), Sense::drag());
                    if r.hovered() || r.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    if r.dragged() {
                        let max = dock.height() - (HEADER + MIN_BODY) * 2.0;
                        self.history_height = (self.history_height - r.drag_delta().y)
                            .clamp(HEADER + MIN_BODY, max.max(HEADER + MIN_BODY));
                    }
                    if r.drag_stopped() {
                        self.changed = true;
                    }
                }
            });
    }
}

/// An island's frame: rounded, bordered, with a soft shadow.
pub fn island(palette: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.window)
        .stroke(egui::Stroke::new(1.0, palette.window_border))
        .corner_radius(theme::radius::PANEL)
        .shadow(palette.panel_shadow())
}

/// A section's title row: chevron and name (click to collapse), buttons on the right.
/// Returns whether the title was clicked.
fn header(
    ui: &mut egui::Ui,
    panel: Panel,
    collapsed: bool,
    palette: &Palette,
    contents: &mut impl FnMut(Panel, Part, &mut egui::Ui),
) -> bool {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEADER), Sense::hover());
    let font = egui::FontId::new(theme::text::HEADING, theme::semibold());
    let galley = ui
        .painter()
        .layout_no_wrap(panel.name().to_owned(), font, palette.text);
    let title = Rect::from_min_size(
        rect.min + vec2(8.0, 4.0),
        vec2(galley.size().x + 26.0, HEADER - 8.0),
    );
    let response = ui
        .interact(title, ui.id().with(("title", panel.name())), Sense::click())
        .on_hover_text(if collapsed { "Expand" } else { "Collapse" });
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, panel.name())
    });
    if response.hovered() {
        ui.painter()
            .rect_filled(title, theme::radius::CONTROL, palette.hover);
    }
    // Chevron: down when expanded, right when collapsed.
    let c = pos2(title.min.x + 11.0, title.center().y);
    let s = 3.5;
    let pts = if collapsed {
        vec![c + vec2(-1.5, -s), c + vec2(2.0, 0.0), c + vec2(-1.5, s)]
    } else {
        vec![c + vec2(-s, -1.5), c + vec2(0.0, 2.0), c + vec2(s, -1.5)]
    };
    ui.painter().add(egui::Shape::line(
        pts,
        egui::Stroke::new(1.5, palette.weak_text),
    ));
    ui.painter().galley(
        pos2(title.min.x + 20.0, title.center().y - galley.size().y / 2.0),
        galley,
        palette.text,
    );
    let actions = Rect::from_min_max(
        pos2(title.max.x + 4.0, rect.min.y),
        rect.max - vec2(6.0, 0.0),
    );
    if actions.width() > 0.0 && !collapsed {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(actions)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 0.0;
        child.spacing_mut().button_padding = vec2(4.0, 4.0);
        contents(panel, Part::Actions, &mut child);
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> LayoutInput {
        LayoutInput {
            visible: [true; 4],
            collapsed: [false; 4],
            rail: vec2(52.0, 600.0),
            options_height: 48.0,
            dock_width: 300.0,
            colors_height: 300.0,
            history_height: 192.0,
        }
    }

    const CENTRAL: Rect = Rect {
        min: pos2(0.0, 40.0),
        max: pos2(1440.0, 872.0),
    };

    #[test]
    fn design_a_layout() {
        let l = compute(CENTRAL, &input());
        let rail = l.rail.unwrap();
        // EDGE from the window edges, GAP from each other: rail 52 wide, dock 300, options
        // bar 48 high.
        let top = CENTRAL.min.y + EDGE;
        assert_eq!(rail.min, pos2(EDGE, top));
        let dock = l.dock.unwrap();
        let (right, bottom) = (CENTRAL.max.x - EDGE, CENTRAL.max.y - EDGE);
        assert_eq!(
            dock,
            Rect::from_min_max(pos2(right - 300.0, top), pos2(right, bottom))
        );
        let (left, middle_right) = (EDGE + 52.0 + GAP, right - 300.0 - GAP);
        assert_eq!(
            l.options,
            Rect::from_min_max(pos2(left, top), pos2(middle_right, top + 48.0))
        );
        assert_eq!(
            l.free,
            Rect::from_min_max(pos2(left, top + 48.0 + GAP), pos2(middle_right, bottom))
        );
        let names: Vec<_> = l.sections.iter().map(|(p, _)| *p).collect();
        assert_eq!(names, DOCK_ORDER);
        let (colors, layers, history) = (l.sections[0].1, l.sections[1].1, l.sections[2].1);
        assert_eq!(colors.height(), 300.0);
        assert_eq!(history.height(), 192.0);
        assert_eq!(layers.min.y, colors.max.y + STACK_GAP);
        assert_eq!(history.max.y, dock.max.y, "Layers fills the room between");
    }

    #[test]
    fn hidden_panels_free_the_canvas() {
        let mut i = input();
        i.visible = [false; 4];
        let l = compute(CENTRAL, &i);
        assert!(l.rail.is_none() && l.dock.is_none() && l.sections.is_empty());
        assert_eq!(l.free.min.x, EDGE);
        assert_eq!(l.free.max.x, CENTRAL.max.x - EDGE);
    }

    #[test]
    fn collapsed_sections_are_title_high_and_history_flexes_without_layers() {
        let mut i = input();
        i.collapsed[idx(Panel::Colors)] = true;
        i.visible[idx(Panel::Layers)] = false;
        let l = compute(CENTRAL, &i);
        assert_eq!(l.sections.len(), 2);
        assert_eq!(l.sections[0].1.height(), HEADER);
        assert_eq!(l.sections[1].0, Panel::History);
        assert_eq!(l.sections[1].1.max.y, l.dock.unwrap().max.y);
    }

    #[test]
    fn short_windows_squeeze_history_before_layers() {
        let mut i = input();
        i.history_height = 600.0;
        let central = Rect::from_min_max(pos2(0.0, 0.0), pos2(1200.0, 700.0));
        let l = compute(central, &i);
        let layers = l.sections[1].1;
        assert!(layers.height() >= HEADER + MIN_BODY - 0.01, "{layers:?}");
        assert!(l.sections[2].1.max.y <= l.dock.unwrap().max.y + 0.01);
    }

    #[test]
    fn layout_round_trips_through_settings() {
        let mut p = Panels::default();
        p.toggle(Panel::Tools);
        p.set_collapsed(Panel::Colors, true);
        let saved = p.to_dock();
        let mut q = Panels::default();
        q.load(&saved);
        assert!(!q.visible(Panel::Tools) && q.collapsed(Panel::Colors));
        assert!(q.visible(Panel::Layers) && !q.collapsed(Panel::Layers));
        assert!(!q.take_changed());
    }
}
