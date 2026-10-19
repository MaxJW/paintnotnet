//! Modal dialogs: New Image, unsaved changes, JPEG quality, errors.

use eframe::egui::{self, Align, Layout};

use crate::icons::{Icon, Icons};
use crate::theme;

/// paint.net's maximum image dimension.
pub const MAX_DIMENSION: u32 = 65535;

pub enum Modal {
    NewImage(NewImage),
    About,
    /// File > Open Recent > Clear this list: asks first, as paint.net 3.36.
    ClearRecent,
    /// The pasted image is larger than the canvas (paint.net asks the same).
    PasteLarger {
        new_layer: bool,
    },
    /// Saving a layered image in a flat format (paint.net's WarnAboutFlattening).
    FlattenForSave {
        key: u64,
        then: SaveTarget,
    },
    UnsavedChanges {
        key: u64,
        title: String,
    },
    UnsavedOnExit {
        count: usize,
    },
    /// Save options: the file type (a dropdown of every format) and that format's settings
    /// (paint.net's Save Configuration dialog, for every type in one).
    SaveConfig {
        key: u64,
        options: pnn_io::SaveOptions,
        then: SaveTarget,
    },
    Error {
        title: String,
        message: String,
    },
    /// Images autosaved by a session that didn't exit cleanly.
    Recover {
        items: Vec<crate::autosave::Recovered>,
    },
}

/// Where a save goes once its settings are confirmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveTarget {
    pub path: std::path::PathBuf,
    pub format: pnn_io::FileFormat,
    pub close_after: bool,
    /// The flatten prompt has been answered for this save.
    pub flattened: bool,
    /// Save As: the save options dialog shows (to pick the file type) even for a format
    /// without settings.
    pub choose_type: bool,
    /// The save options dialog has been answered for this save.
    pub configured: bool,
    /// The path chosen before the save options dialog (`None` until it opens).
    pub picked: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModalResult {
    CreateImage {
        width: u32,
        height: u32,
    },
    Save {
        key: u64,
    },
    DontSave {
        key: u64,
    },
    SaveAllAndExit,
    ExitWithoutSaving,
    Recover,
    DiscardRecovered,
    ClearRecent,
    SaveConfigured {
        key: u64,
        options: pnn_io::SaveOptions,
        then: SaveTarget,
    },
    PasteExpand {
        new_layer: bool,
        expand: bool,
    },
    FlattenAndSave {
        key: u64,
        then: SaveTarget,
    },
    Cancelled,
    Closed,
}

pub struct NewImage {
    pub width: u32,
    pub height: u32,
    pub keep_aspect: bool,
}

impl NewImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            keep_aspect: true,
        }
    }
}

/// How a dialog button looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The main action: filled with the accent; Enter picks it.
    Primary,
    Normal,
}

/// New Image presets: name, width, height.
pub const SIZE_PRESETS: [(&str, u32, u32); 6] = [
    ("Default", 800, 600),
    ("HD", 1280, 720),
    ("Full HD", 1920, 1080),
    ("4K UHD", 3840, 2160),
    ("Square", 1080, 1080),
    ("A4 · 300 dpi", 2480, 3508),
];

impl Modal {
    /// Draw the dialog. Returns a result once the user picks something.
    pub fn show(&mut self, ctx: &egui::Context, icons: &Icons) -> Option<ModalResult> {
        let palette = theme::palette(ctx);
        let id = egui::Id::new("modal");
        let mut result = None;
        let frame = egui::Frame::new()
            .fill(palette.window)
            .stroke(egui::Stroke::new(1.0, palette.window_border))
            .corner_radius(theme::radius::PANEL)
            .shadow(egui::Shadow {
                offset: [0, 18],
                blur: 60,
                spread: 0,
                color: palette.shadow.gamma_multiply(1.4),
            })
            .inner_margin(egui::Margin {
                left: 22,
                right: 22,
                top: 20,
                bottom: 18,
            });
        let backdrop = if palette.dark {
            egui::Color32::from_black_alpha(115)
        } else {
            egui::Color32::from_rgba_unmultiplied(20, 30, 50, 70)
        };
        let response = egui::Modal::new(id)
            .frame(frame)
            .backdrop_color(backdrop)
            .show(ctx, |ui| {
                ui.set_width(if matches!(self, Modal::NewImage(_)) { 460.0 } else { 380.0 });
                ui.spacing_mut().item_spacing.y = 10.0;
                result = match self {
                    Modal::NewImage(state) => new_image(ui, state, icons, &palette),
                    Modal::UnsavedChanges { key, title } => {
                        header(
                            ui,
                            icons,
                            Some(Icon::Save),
                            &format!("Save changes to \u{201c}{title}\u{201d}?"),
                            Some("Your changes will be lost if you close it without saving."),
                            &palette,
                        );
                        footer(
                            ui,
                            &[
                                ("Save", ModalResult::Save { key: *key }, Kind::Primary),
                                ("Cancel", ModalResult::Cancelled, Kind::Normal),
                                ("Don't Save", ModalResult::DontSave { key: *key }, Kind::Normal),
                            ],
                            &palette,
                        )
                    }
                    Modal::UnsavedOnExit { count } => {
                        let what = if *count == 1 {
                            "1 image has".to_owned()
                        } else {
                            format!("{count} images have")
                        };
                        header(
                            ui,
                            icons,
                            Some(Icon::Save),
                            "Save before exiting?",
                            Some(&format!("{what} unsaved changes.")),
                            &palette,
                        );
                        footer(
                            ui,
                            &[
                                ("Save All", ModalResult::SaveAllAndExit, Kind::Primary),
                                ("Cancel", ModalResult::Cancelled, Kind::Normal),
                                ("Don't Save", ModalResult::ExitWithoutSaving, Kind::Normal),
                            ],
                            &palette,
                        )
                    }
                    Modal::SaveConfig { key, options, then } => {
                        let name = then
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        header(
                            ui,
                            icons,
                            Some(Icon::Save),
                            "Save options",
                            Some(&name),
                            &palette,
                        );
                        egui::Grid::new("save_config")
                            .num_columns(2)
                            .spacing(egui::vec2(14.0, 10.0))
                            .show(ui, |ui| {
                                field_label(ui, "File type", &palette);
                                let mut format = then.format;
                                egui::ComboBox::from_id_salt("save_file_type")
                                    .selected_text(type_label(format))
                                    .show_ui(ui, |ui| {
                                        for f in pnn_io::FileFormat::ALL {
                                            ui.selectable_value(&mut format, f, type_label(f));
                                        }
                                    });
                                ui.end_row();
                                if format != then.format {
                                    then.format = format;
                                    then.path.set_extension(format.extensions()[0]);
                                }
                                format_options(ui, then.format, options, &palette);
                            });
                        // Another type means another file name: say if that replaces a file.
                        if then.path.exists() && Some(&then.path) != picked_path(then) {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} already exists and will be replaced.",
                                    then.path
                                        .file_name()
                                        .map(|n| n.to_string_lossy())
                                        .unwrap_or_default()
                                ))
                                .color(palette.weak_text),
                            );
                        }
                        let ok = ModalResult::SaveConfigured {
                            key: *key,
                            options: *options,
                            then: then.clone(),
                        };
                        footer(
                            ui,
                            &[
                                ("OK", ok, Kind::Primary),
                                ("Cancel", ModalResult::Cancelled, Kind::Normal),
                            ],
                            &palette,
                        )
                    }
                    Modal::FlattenForSave { key, then } => {
                        header(
                            ui,
                            icons,
                            Some(Icon::Flatten),
                            "Flatten to save?",
                            Some("This file format holds a single layer, so the image will be flattened and then saved. You can undo the flatten afterwards."),
                            &palette,
                        );
                        let ok = ModalResult::FlattenAndSave {
                            key: *key,
                            then: then.clone(),
                        };
                        footer(
                            ui,
                            &[
                                ("Flatten", ok, Kind::Primary),
                                ("Cancel", ModalResult::Cancelled, Kind::Normal),
                            ],
                            &palette,
                        )
                    }
                    Modal::PasteLarger { new_layer } => {
                        header(
                            ui,
                            icons,
                            Some(Icon::Paste),
                            "The pasted image is larger than the canvas",
                            Some("Expand the canvas to fit it, or keep the canvas size and crop what overhangs."),
                            &palette,
                        );
                        let n = *new_layer;
                        footer(
                            ui,
                            &[
                                (
                                    "Expand canvas",
                                    ModalResult::PasteExpand {
                                        new_layer: n,
                                        expand: true,
                                    },
                                    Kind::Primary,
                                ),
                                (
                                    "Keep canvas size",
                                    ModalResult::PasteExpand {
                                        new_layer: n,
                                        expand: false,
                                    },
                                    Kind::Normal,
                                ),
                                ("Cancel", ModalResult::Cancelled, Kind::Normal),
                            ],
                            &palette,
                        )
                    }
                    Modal::Recover { items } => {
                        header(
                            ui,
                            icons,
                            Some(Icon::HistoryWindow),
                            "Recover images",
                            Some("paintnotnet didn't close properly last time. These images had unsaved changes:"),
                            &palette,
                        );
                        egui::Frame::new()
                            .fill(palette.list)
                            .corner_radius(theme::radius::CONTROL)
                            .inner_margin(10)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 4.0;
                                for r in items.iter() {
                                    ui.label(&r.title);
                                    if let Some(p) = &r.original {
                                        ui.label(
                                            egui::RichText::new(p.display().to_string())
                                                .small()
                                                .color(palette.weak_text),
                                        );
                                    }
                                }
                            });
                        footer(
                            ui,
                            &[
                                ("Recover", ModalResult::Recover, Kind::Primary),
                                ("Discard", ModalResult::DiscardRecovered, Kind::Normal),
                            ],
                            &palette,
                        )
                    }
                    Modal::ClearRecent => {
                        header(
                            ui,
                            icons,
                            Some(Icon::Open),
                            "Open Recent",
                            Some("Are you sure you want to clear the Open Recent list?"),
                            &palette,
                        );
                        footer(
                            ui,
                            &[
                                ("Yes", ModalResult::ClearRecent, Kind::Primary),
                                ("No", ModalResult::Cancelled, Kind::Normal),
                            ],
                            &palette,
                        )
                    }
                    Modal::About => {
                        header(
                            ui,
                            icons,
                            None,
                            "paintnotnet",
                            Some(&format!("Version {}", env!("CARGO_PKG_VERSION"))),
                            &palette,
                        );
                        ui.label("A modern paint.net for Linux, written in Rust.");
                        ui.label(
                            egui::RichText::new(
                                "Includes code ported from Paint.NET 3.36 (MIT). Icons: Lucide (ISC). \
                                 Typeface: Geist (SIL OFL). See NOTICE.",
                            )
                            .color(palette.weak_text),
                        );
                        footer(ui, &[("OK", ModalResult::Closed, Kind::Primary)], &palette)
                    }
                    Modal::Error { title, message } => {
                        header(
                            ui,
                            icons,
                            Some(Icon::Warning),
                            title.as_str(),
                            Some(message.as_str()),
                            &palette,
                        );
                        footer(ui, &[("OK", ModalResult::Closed, Kind::Primary)], &palette)
                    }
                };
            });
        if result.is_none() && response.should_close() {
            result = Some(ModalResult::Cancelled);
        }
        result
    }
}

/// A dialog's title (with an optional icon) and a line or two of explanation.
fn header(
    ui: &mut egui::Ui,
    icons: &Icons,
    icon: Option<Icon>,
    title: &str,
    description: Option<&str>,
    palette: &theme::Palette,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if let Some(icon) = icon {
            let (r, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(r, theme::radius::CONTROL, palette.accent_alpha(0.16));
            ui.painter().image(
                icons.texture(icon).id(),
                egui::Rect::from_center_size(r.center(), egui::vec2(18.0, 18.0)),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                palette.accent,
            );
        }
        ui.label(
            egui::RichText::new(title)
                .font(egui::FontId::new(17.0, theme::semibold()))
                .color(palette.text),
        );
    });
    if let Some(d) = description {
        ui.label(egui::RichText::new(d).color(palette.weak_text));
    }
    ui.add_space(4.0);
}

/// "WebP (.webp)": a file type in the save options' dropdown.
fn type_label(f: pnn_io::FileFormat) -> String {
    format!("{} (.{})", f.name(), f.extensions()[0])
}

/// The path the save started with (before the file type changed), kept in `picked`.
fn picked_path(then: &SaveTarget) -> Option<&std::path::PathBuf> {
    then.picked.as_ref()
}

/// The settings rows for saving as `format`; formats without settings get a note on what
/// saving keeps.
fn format_options(
    ui: &mut egui::Ui,
    format: pnn_io::FileFormat,
    options: &mut pnn_io::SaveOptions,
    palette: &theme::Palette,
) {
    use pnn_io::FileFormat;
    match format {
        FileFormat::WebP => {
            // pdn-webp's controls (bundled with paint.net 5).
            field_label(ui, "Preset", palette);
            egui::ComboBox::from_id_salt("webp_preset")
                .selected_text(options.webp_preset.name())
                .show_ui(ui, |ui| {
                    for p in pnn_io::WebPPreset::ALL {
                        ui.selectable_value(&mut options.webp_preset, p, p.name());
                    }
                });
            ui.end_row();
            field_label(ui, "Quality", palette);
            ui.add_enabled(
                !options.webp_lossless,
                egui::Slider::new(&mut options.webp_quality, 0..=100),
            );
            ui.end_row();
            field_label(ui, "Effort", palette);
            ui.add(egui::Slider::new(&mut options.webp_effort, 0..=9))
                .on_hover_text("Higher is slower and smaller");
            ui.end_row();
            ui.label("");
            crate::controls::checkbox(ui, &mut options.webp_lossless, "Lossless");
            ui.end_row();
        }
        FileFormat::Jpeg => {
            field_label(ui, "Quality", palette);
            ui.add(egui::Slider::new(&mut options.jpeg_quality, 0..=100));
            ui.end_row();
        }
        f => {
            let mut note = if f.layered() {
                "Keeps the layers.".to_owned()
            } else {
                "Saved as one layer.".to_owned()
            };
            if !f.has_alpha() {
                note.push_str(" Transparent areas become white.");
            }
            ui.label("");
            ui.label(egui::RichText::new(note).color(palette.weak_text));
            ui.end_row();
        }
    }
}

fn field_label(ui: &mut egui::Ui, text: &str, palette: &theme::Palette) {
    ui.label(egui::RichText::new(text).color(palette.weak_text));
}

/// Buttons on the right, the first (primary) rightmost. Enter picks the primary one.
/// The OK / Cancel row of the effect and document dialogs, styled as the message boxes'
/// `footer`: right-aligned, Cancel then the accent OK. `Some(true)`: OK.
pub fn ok_cancel(ui: &mut egui::Ui, ok_enabled: bool) -> Option<bool> {
    let palette = theme::palette(ui.ctx());
    let mut picked = None;
    // A horizontal row keeps the right-aligned buttons from claiming the window's whole
    // height (an auto-sized window would grow to fill the screen).
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let size = egui::vec2(84.0, 32.0);
            let ok = egui::Button::new(
                egui::RichText::new("OK")
                    .color(palette.on_accent)
                    .font(egui::FontId::new(theme::text::BODY, theme::medium())),
            )
            .fill(palette.accent)
            .min_size(size);
            if ui.add_enabled(ok_enabled, ok).clicked() {
                picked = Some(true);
            }
            if ui.add(egui::Button::new("Cancel").min_size(size)).clicked() {
                picked = Some(false);
            }
        });
    });
    picked
}

fn footer(
    ui: &mut egui::Ui,
    choices: &[(&str, ModalResult, Kind)],
    palette: &theme::Palette,
) -> Option<ModalResult> {
    let mut picked = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (label, result, kind) in choices {
                let button = match kind {
                    Kind::Primary => egui::Button::new(
                        egui::RichText::new(*label)
                            .color(palette.on_accent)
                            .font(egui::FontId::new(theme::text::BODY, theme::medium())),
                    )
                    .fill(palette.accent),
                    Kind::Normal => egui::Button::new(*label),
                };
                if ui.add(button.min_size(egui::vec2(84.0, 32.0))).clicked() {
                    picked = Some(result.clone());
                }
            }
        });
    });
    if picked.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        picked = choices
            .iter()
            .find(|c| c.2 == Kind::Primary)
            .map(|(_, r, _)| r.clone());
    }
    picked
}

fn new_image(
    ui: &mut egui::Ui,
    s: &mut NewImage,
    icons: &Icons,
    palette: &theme::Palette,
) -> Option<ModalResult> {
    header(ui, icons, Some(Icon::New), "New image", None, palette);
    // Presets: three to a row, name over size.
    egui::Grid::new("new_image_presets")
        .num_columns(3)
        .spacing(egui::vec2(8.0, 8.0))
        .show(ui, |ui| {
            for (i, &(name, w, h)) in SIZE_PRESETS.iter().enumerate() {
                if preset_card(ui, name, w, h, (s.width, s.height) == (w, h), palette) {
                    (s.width, s.height) = (w, h);
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // Fields on the left, a preview of the shape on the right.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let (old_w, old_h) = (s.width, s.height);
            let mut swapped = false;
            egui::Grid::new("new_image_size")
                .num_columns(3)
                .spacing(egui::vec2(10.0, 8.0))
                .show(ui, |ui| {
                    field_label(ui, "Width", palette);
                    ui.add_sized(
                        [110.0, 28.0],
                        egui::DragValue::new(&mut s.width)
                            .range(1..=MAX_DIMENSION)
                            .suffix(" px"),
                    );
                    let (icon, tip) = if s.keep_aspect {
                        (Icon::AspectLocked, "Aspect ratio kept: click to unlink")
                    } else {
                        (
                            Icon::AspectFree,
                            "Width and height independent: click to link",
                        )
                    };
                    let link = ui
                        .add(egui::Button::image(icons.image(icon)).selected(s.keep_aspect))
                        .on_hover_text(tip);
                    let keep = s.keep_aspect;
                    link.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            keep,
                            "Keep aspect ratio",
                        )
                    });
                    if link.clicked() {
                        s.keep_aspect = !s.keep_aspect;
                    }
                    ui.end_row();
                    field_label(ui, "Height", palette);
                    ui.add_sized(
                        [110.0, 28.0],
                        egui::DragValue::new(&mut s.height)
                            .range(1..=MAX_DIMENSION)
                            .suffix(" px"),
                    );
                    let swap = ui
                        .add(egui::Button::image(icons.image(Icon::SwapOrientation)))
                        .on_hover_text("Swap width and height");
                    swap.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "Swap width and height",
                        )
                    });
                    if swap.clicked() {
                        (s.width, s.height) = (s.height, s.width);
                        swapped = true;
                    }
                    ui.end_row();
                });
            // Only edits to one field keep the ratio (a swap already does).
            if s.keep_aspect && !swapped {
                if s.width != old_w {
                    s.height = ((s.width as f64 * old_h as f64 / old_w as f64).round() as u32)
                        .clamp(1, MAX_DIMENSION);
                } else if s.height != old_h {
                    s.width = ((s.height as f64 * old_w as f64 / old_h as f64).round() as u32)
                        .clamp(1, MAX_DIMENSION);
                }
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            shape_preview(ui, s.width, s.height, palette);
        });
    });
    let bytes = s.width as u64 * s.height as u64 * 4;
    ui.label(
        egui::RichText::new(format!(
            "{} × {} px · {} in memory",
            s.width,
            s.height,
            human_bytes(bytes)
        ))
        .color(palette.weak_text),
    );
    footer(
        ui,
        &[
            (
                "Create",
                ModalResult::CreateImage {
                    width: s.width,
                    height: s.height,
                },
                Kind::Primary,
            ),
            ("Cancel", ModalResult::Cancelled, Kind::Normal),
        ],
        palette,
    )
}

/// A preset button: its name over its size. Returns whether it was clicked.
fn preset_card(
    ui: &mut egui::Ui,
    name: &str,
    w: u32,
    h: u32,
    selected: bool,
    palette: &theme::Palette,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(132.0, 48.0), egui::Sense::click());
    let label = format!("{name} {w} × {h}");
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &label)
    });
    let (fill, stroke) = if selected {
        (palette.selection, palette.selection_stroke)
    } else if response.hovered() {
        (palette.hover.gamma_multiply(1.8), palette.window_border)
    } else {
        (palette.hover, palette.window_border)
    };
    ui.painter().rect(
        rect,
        theme::radius::CONTROL,
        fill,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.left_top() + egui::vec2(12.0, 9.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::new(theme::text::BODY, theme::medium()),
        palette.text,
    );
    ui.painter().text(
        rect.left_bottom() + egui::vec2(12.0, -9.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{w} × {h}"),
        egui::FontId::monospace(theme::text::SMALL),
        palette.weak_text,
    );
    response.clicked()
}

/// The new image's shape, scaled into a 120 × 90 box.
fn shape_preview(ui: &mut egui::Ui, w: u32, h: u32, palette: &theme::Palette) {
    let (area, _) = ui.allocate_exact_size(egui::vec2(120.0, 90.0), egui::Sense::hover());
    let scale = (area.width() / w as f32).min(area.height() / h as f32);
    let size = egui::vec2((w as f32 * scale).max(2.0), (h as f32 * scale).max(2.0));
    let r = egui::Rect::from_center_size(area.center(), size);
    ui.painter().rect(
        r,
        2.0,
        egui::Color32::WHITE,
        egui::Stroke::new(1.0, palette.window_border),
        egui::StrokeKind::Outside,
    );
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_formatting() {
        assert_eq!(human_bytes(512), "512 bytes");
        assert_eq!(human_bytes(800 * 600 * 4), "1.8 MB");
    }
}
