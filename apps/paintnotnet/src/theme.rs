//! paintnotnet's design tokens: "Graphite Glass" (design A, chosen 2026-10-14) in dark, plus a
//! light counterpart with the same structure. Colours, corner radii, spacing, type scale and
//! motion live here, and `setup` maps them onto every egui widget so menus, toolbars, panels,
//! dialogs and effect dialogs share one look. UI text is Geist; numbers are Geist Mono.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Visuals,
};

/// Corner radii in points.
pub mod radius {
    /// Panels (islands) and dialogs.
    pub const PANEL: u8 = 14;
    /// Menus and popups.
    pub const MENU: u8 = 10;
    /// Buttons, text boxes, combo boxes, list rows.
    pub const CONTROL: u8 = 8;
    /// Small chips, swatches, checkboxes.
    pub const SMALL: u8 = 6;
}

/// Spacing scale in points.
pub mod space {
    pub const XS: f32 = 2.0;
    pub const S: f32 = 4.0;
    pub const M: f32 = 8.0;
    pub const L: f32 = 12.0;
    pub const XL: f32 = 16.0;
}

/// Type scale in points.
pub mod text {
    pub const BODY: f32 = 13.0;
    pub const SMALL: f32 = 11.5;
    pub const HEADING: f32 = 14.0;
    pub const MONO: f32 = 12.0;
}

/// Animation durations in seconds.
pub mod motion {
    /// Hover and press feedback, toggles.
    pub const FAST: f32 = 0.12;
    /// Panels and sections opening or collapsing.
    pub const NORMAL: f32 = 0.18;
}

/// Heights of the fixed bands, in points.
pub mod size {
    /// Height of buttons, text boxes and list rows.
    pub const CONTROL: f32 = 28.0;
}

/// Font family for medium-weight text (labels that need a little emphasis).
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

/// Font family for semibold text (panel titles, headings, the active tool's name).
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// Title bar, menu band and status bar.
    pub band: Color32,
    /// Panel and dialog body where no glass is drawn (opaque).
    pub window: Color32,
    /// Panel and dialog body over the blurred canvas (translucent).
    pub glass: Color32,
    pub window_border: Color32,
    /// Sunken fields: text boxes, lists.
    pub list: Color32,
    /// Slider rails, unchecked checkboxes, scroll handles.
    pub control: Color32,
    /// Workspace around the image.
    pub surround: Color32,
    pub text: Color32,
    /// Labels next to controls, secondary information.
    pub weak_text: Color32,
    /// Hints and disabled-looking text that must still be readable.
    pub faint_text: Color32,
    /// Monochrome icons.
    pub icon: Color32,
    pub accent: Color32,
    /// Text and icons drawn on a solid accent fill.
    pub on_accent: Color32,
    /// Selected list item / active tool.
    pub selection: Color32,
    pub selection_stroke: Color32,
    pub hover: Color32,
    /// Destructive actions (closing, deleting).
    pub close: Color32,
    pub separator: Color32,
    /// Drop shadow colour under panels, dialogs and menus.
    pub shadow: Color32,
}

fn rgba(r: u8, g: u8, b: u8, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0).round() as u8)
}

fn with_alpha(c: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    rgba(r, g, b, alpha)
}

impl Palette {
    pub fn dark() -> Self {
        let accent = Color32::from_rgb(0x5b, 0x9d, 0xff);
        Self {
            dark: true,
            band: Color32::from_rgb(0x10, 0x11, 0x14),
            window: Color32::from_rgb(0x1e, 0x20, 0x26),
            glass: rgba(0x1e, 0x20, 0x26, 0.62),
            window_border: rgba(255, 255, 255, 0.08),
            list: Color32::from_rgb(0x17, 0x18, 0x1c),
            control: Color32::from_rgb(0x36, 0x38, 0x3f),
            surround: Color32::from_rgb(0x15, 0x16, 0x1a),
            text: Color32::from_rgb(0xe8, 0xe9, 0xec),
            weak_text: Color32::from_rgb(0xa3, 0xa7, 0xb0),
            faint_text: Color32::from_rgb(0x8d, 0x91, 0x9b),
            icon: Color32::from_rgb(0xc9, 0xcc, 0xd3),
            accent,
            on_accent: Color32::from_rgb(0x0e, 0x17, 0x30),
            selection: with_alpha(accent, 0.24),
            selection_stroke: with_alpha(accent, 0.7),
            hover: rgba(255, 255, 255, 0.07),
            close: Color32::from_rgb(0xe5, 0x48, 0x4d),
            separator: rgba(255, 255, 255, 0.10),
            shadow: rgba(0, 0, 0, 0.35),
        }
    }

    pub fn light() -> Self {
        let accent = Color32::from_rgb(0x2f, 0x6f, 0xe0);
        Self {
            dark: false,
            band: Color32::from_rgb(0xf3, 0xf4, 0xf6),
            window: Color32::from_rgb(0xfb, 0xfb, 0xfc),
            glass: rgba(0xff, 0xff, 0xff, 0.66),
            window_border: rgba(0x14, 0x1e, 0x32, 0.10),
            list: Color32::from_rgb(0xec, 0xee, 0xf1),
            control: Color32::from_rgb(0xd3, 0xd7, 0xde),
            surround: Color32::from_rgb(0xe4, 0xe7, 0xec),
            text: Color32::from_rgb(0x1c, 0x20, 0x26),
            weak_text: Color32::from_rgb(0x4a, 0x51, 0x5c),
            faint_text: Color32::from_rgb(0x5f, 0x66, 0x72),
            icon: Color32::from_rgb(0x2e, 0x35, 0x40),
            accent,
            on_accent: Color32::WHITE,
            selection: with_alpha(accent, 0.18),
            selection_stroke: with_alpha(accent, 0.6),
            hover: rgba(0x14, 0x1e, 0x32, 0.06),
            close: Color32::from_rgb(0xd9, 0x3a, 0x3f),
            separator: rgba(0x14, 0x1e, 0x32, 0.10),
            shadow: rgba(0x14, 0x1e, 0x32, 0.16),
        }
    }

    /// The accent at `alpha` opacity (selected tools, presets, focus rings).
    pub fn accent_alpha(&self, alpha: f32) -> Color32 {
        with_alpha(self.accent, alpha)
    }

    /// Shadow under panels and dialogs.
    pub fn panel_shadow(&self) -> Shadow {
        Shadow {
            offset: [0, 10],
            blur: 34,
            spread: 0,
            color: self.shadow,
        }
    }

    /// egui's visuals for this palette.
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        let control = CornerRadius::same(radius::CONTROL);
        v.panel_fill = self.band;
        v.window_fill = self.window;
        v.window_stroke = Stroke::new(1.0, self.window_border);
        v.window_corner_radius = CornerRadius::same(radius::PANEL);
        v.window_shadow = self.panel_shadow();
        v.window_highlight_topmost = false;
        v.popup_shadow = Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: self.shadow,
        };
        v.menu_corner_radius = CornerRadius::same(radius::MENU);
        v.extreme_bg_color = self.list;
        v.text_edit_bg_color = Some(self.list);
        v.faint_bg_color = self.hover;
        v.code_bg_color = self.list;
        v.override_text_color = Some(self.text);
        v.weak_text_color = Some(self.weak_text);
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.selection;
        v.selection.stroke = Stroke::new(1.0, self.selection_stroke);
        v.text_cursor.stroke = Stroke::new(2.0, self.accent);
        v.slider_trailing_fill = true;
        v.collapsing_header_frame = false;
        v.indent_has_left_vline = false;
        v.striped = false;
        v.disabled_alpha = 0.45;
        v.resize_corner_size = 10.0;

        let w = &mut v.widgets;
        for wv in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            wv.corner_radius = control;
            wv.expansion = 0.0;
            wv.fg_stroke = Stroke::new(1.5, self.icon);
            wv.bg_stroke = Stroke::NONE;
        }
        w.noninteractive.bg_fill = self.window;
        w.noninteractive.weak_bg_fill = self.window;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.separator);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        // Buttons sit lightly on the panel; checkboxes, rails and scroll handles are raised.
        w.inactive.weak_bg_fill = self.hover;
        w.inactive.bg_fill = self.control;
        w.hovered.weak_bg_fill = self.hover.gamma_multiply(1.8);
        w.hovered.bg_fill = self.control.lerp_to_gamma(self.text, 0.15);
        w.hovered.bg_stroke = Stroke::new(1.0, self.window_border);
        w.hovered.fg_stroke = Stroke::new(1.5, self.text);
        w.active.weak_bg_fill = self.accent_alpha(0.32);
        w.active.bg_fill = self.accent;
        w.active.bg_stroke = Stroke::new(1.0, self.selection_stroke);
        w.active.fg_stroke = Stroke::new(1.5, self.text);
        w.open.weak_bg_fill = self.hover.gamma_multiply(1.8);
        w.open.bg_fill = self.hover.gamma_multiply(1.8);
        w.open.fg_stroke = Stroke::new(1.5, self.text);
        v
    }
}

/// Install the fonts, type scale, spacing and both palettes. Call once at startup.
pub fn setup(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(text::BODY));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(text::BODY));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(text::SMALL));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(text::HEADING, semibold()));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(text::MONO));
        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(space::M + 2.0, space::M);
        s.button_padding = egui::vec2(space::L, space::S + 1.0);
        s.interact_size.y = size::CONTROL;
        s.menu_margin = Margin::same(8);
        s.window_margin = Margin::same(space::XL as i8);
        s.slider_rail_height = 4.0;
        s.icon_width = 16.0;
        s.icon_width_inner = 9.0;
        s.icon_spacing = 6.0;
        s.menu_width = 220.0;
        s.scroll = egui::style::ScrollStyle::floating();
        style.animation_time = motion::FAST;
        // Quick to explain an icon; once one tooltip is up, its neighbours show at once.
        style.interaction.tooltip_delay = 0.15;
        style.interaction.tooltip_grace_time = 0.6;
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

macro_rules! font {
    ($file:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/geist/",
            $file
        ))
    };
}

/// Geist (Regular, Medium, SemiBold) and Geist Mono, then the desktop's UI font for scripts
/// Geist doesn't cover, then egui's own fallbacks (symbols, emoji).
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.into(), FontData::from_static(bytes).into());
    };
    add("geist", font!("Geist-Regular.ttf"));
    add("geist-medium", font!("Geist-Medium.ttf"));
    add("geist-semibold", font!("Geist-SemiBold.ttf"));
    add("geist-mono", font!("GeistMono-Regular.ttf"));
    let system = system_ui_font().map(|bytes| {
        fonts
            .font_data
            .insert("system-ui".into(), FontData::from_owned(bytes).into());
        "system-ui".to_owned()
    });
    let fallbacks: Vec<String> = system
        .into_iter()
        .chain(fonts.families[&FontFamily::Proportional].iter().cloned())
        .collect();
    let family = |first: &str| {
        std::iter::once(first.to_owned())
            .chain(fallbacks.iter().cloned())
            .collect::<Vec<_>>()
    };
    let proportional = family("geist");
    let medium_family = family("geist-medium");
    let semibold_family = family("geist-semibold");
    let mut mono = vec!["geist-mono".to_owned()];
    mono.extend(fonts.families[&FontFamily::Monospace].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Monospace, mono);
    fonts.families.insert(medium(), medium_family);
    fonts.families.insert(semibold(), semibold_family);
    fonts
}

/// The desktop's UI font, used only for characters Geist lacks (e.g. CJK layer names).
fn system_ui_font() -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans-serif:style=Regular"])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    std::fs::read(path.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of an opaque colour.
    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn text_is_readable_on_every_surface() {
        for p in [Palette::dark(), Palette::light()] {
            for bg in [p.band, p.window, p.list, p.surround] {
                assert!(contrast(p.text, bg) >= 7.0, "{p:?} text on {bg:?}");
                assert!(
                    contrast(p.weak_text, bg) >= 4.5,
                    "{p:?} weak text on {bg:?}"
                );
                assert!(
                    contrast(p.faint_text, bg) >= 4.5,
                    "{p:?} faint text on {bg:?}"
                );
            }
            assert!(contrast(p.on_accent, p.accent) >= 4.5, "{p:?} on accent");
        }
    }

    #[test]
    fn fonts_have_all_families() {
        let f = fonts();
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            medium(),
            semibold(),
        ] {
            let list = &f.families[&family];
            assert!(list[0].starts_with("geist"), "{family:?}: {list:?}");
            assert!(list.len() > 1, "{family:?} has fallbacks");
        }
    }
}
