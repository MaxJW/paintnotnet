//! paint.net 5's look: Light and Dark colour schemes (sampled from the official doc
//! screenshots), compact 12 px UI text in the system UI font.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Visuals,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// Menu band, toolbars, status bar.
    pub band: Color32,
    /// Floating window body.
    pub window: Color32,
    pub window_border: Color32,
    /// Lists (layers, history) and text boxes.
    pub list: Color32,
    /// Grey area around the image.
    pub surround: Color32,
    pub text: Color32,
    pub weak_text: Color32,
    /// Selected list item / active tool.
    pub selection: Color32,
    pub selection_stroke: Color32,
    pub hover: Color32,
    /// The red close button on floating windows.
    pub close: Color32,
    pub separator: Color32,
}

impl Palette {
    pub fn light() -> Self {
        Self {
            dark: false,
            band: Color32::from_rgb(0xff, 0xff, 0xff),
            window: Color32::from_rgb(0xf0, 0xf0, 0xf0),
            window_border: Color32::from_rgb(0xb4, 0xb4, 0xb4),
            list: Color32::from_rgb(0xf9, 0xf9, 0xf9),
            surround: Color32::from_rgb(0xcf, 0xcf, 0xcf),
            text: Color32::from_rgb(0x1a, 0x1a, 0x1a),
            weak_text: Color32::from_rgb(0x6d, 0x6d, 0x6d),
            selection: Color32::from_rgb(0xcc, 0xe4, 0xf7),
            selection_stroke: Color32::from_rgb(0x7b, 0xb7, 0xe3),
            hover: Color32::from_rgb(0xe5, 0xf1, 0xfb),
            close: Color32::from_rgb(0xc4, 0x2b, 0x1c),
            separator: Color32::from_rgb(0xd9, 0xd9, 0xd9),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            band: Color32::from_rgb(0x1e, 0x1e, 0x1e),
            window: Color32::from_rgb(0x28, 0x28, 0x28),
            window_border: Color32::from_rgb(0x46, 0x46, 0x46),
            list: Color32::from_rgb(0x20, 0x20, 0x20),
            surround: Color32::from_rgb(0x32, 0x32, 0x32),
            text: Color32::from_rgb(0xe6, 0xe6, 0xe6),
            weak_text: Color32::from_rgb(0x9a, 0x9a, 0x9a),
            selection: Color32::from_rgb(0x19, 0x47, 0x6d),
            selection_stroke: Color32::from_rgb(0x3a, 0x8e, 0xe6),
            hover: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            close: Color32::from_rgb(0xc4, 0x2b, 0x1c),
            separator: Color32::from_rgb(0x3c, 0x3c, 0x3c),
        }
    }

    /// Apply to egui's visuals for this theme.
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        v.panel_fill = self.band;
        v.window_fill = self.window;
        v.window_stroke = Stroke::new(1.0, self.window_border);
        v.window_corner_radius = CornerRadius::ZERO;
        v.window_shadow = egui::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(60),
        };
        v.popup_shadow = egui::Shadow {
            offset: [0, 2],
            blur: 6,
            spread: 0,
            color: Color32::from_black_alpha(50),
        };
        v.menu_corner_radius = CornerRadius::ZERO;
        v.extreme_bg_color = self.list;
        v.faint_bg_color = self.window;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.selection;
        v.selection.stroke = Stroke::new(1.0, self.selection_stroke);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(2);
            w.fg_stroke.color = self.text;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.separator);
        v.widgets.noninteractive.fg_stroke.color = self.text;
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.hovered.weak_bg_fill = self.hover;
        v.widgets.hovered.bg_fill = self.hover;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.selection_stroke);
        v.widgets.active.weak_bg_fill = self.selection;
        v.widgets.active.bg_fill = self.selection;
        v.widgets.open.weak_bg_fill = self.selection;
        v
    }
}

/// Install fonts and the compact paint.net spacing. Call once at startup.
pub fn setup(ctx: &egui::Context) {
    if let Some(font) = system_ui_font() {
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("system-ui".into(), FontData::from_owned(font).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system-ui".into());
        ctx.set_fonts(fonts);
    }
    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(12.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(12.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(10.0));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(11.5));
        style.spacing.item_spacing = egui::vec2(6.0, 3.0);
        style.spacing.button_padding = egui::vec2(5.0, 2.0);
        style.spacing.interact_size.y = 20.0;
        style.spacing.menu_margin = egui::Margin::same(3);
        style.spacing.window_margin = egui::Margin::same(4);
        style.interaction.tooltip_delay = 0.5;
    });
    ctx.set_visuals_of(egui::Theme::Dark, Palette::dark().visuals());
    ctx.set_visuals_of(egui::Theme::Light, Palette::light().visuals());
}

/// The palette for the theme egui is currently showing (follows the system setting).
pub fn palette(ctx: &egui::Context) -> Palette {
    if ctx.global_style().visuals.dark_mode {
        Palette::dark()
    } else {
        Palette::light()
    }
}

/// paint.net uses the Windows UI font (Segoe UI); use the desktop's UI font instead.
fn system_ui_font() -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans-serif:style=Regular"])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    std::fs::read(path.trim()).ok()
}
