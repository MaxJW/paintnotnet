//! Modal dialogs: New Image, unsaved changes, JPEG quality, errors.

use eframe::egui::{self, Align, Layout};

/// paint.net's maximum image dimension.
pub const MAX_DIMENSION: u32 = 65535;

pub enum Modal {
    NewImage(NewImage),
    About,
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
    /// paint.net's Save Configuration dialog for formats with settings (JPEG, WebP).
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

impl Modal {
    /// Draw the dialog. Returns a result once the user picks something.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ModalResult> {
        let id = egui::Id::new("modal");
        let mut result = None;
        let response = egui::Modal::new(id).show(ctx, |ui| {
            ui.set_width(340.0);
            result = match self {
                Modal::NewImage(state) => new_image(ui, state),
                Modal::UnsavedChanges { key, title } => {
                    ui.heading("Unsaved Changes");
                    ui.label(format!("Save changes to \"{title}\" before closing?"));
                    ui.add_space(8.0);
                    buttons(
                        ui,
                        &[
                            ("Save", ModalResult::Save { key: *key }),
                            ("Don't Save", ModalResult::DontSave { key: *key }),
                            ("Cancel", ModalResult::Cancelled),
                        ],
                    )
                }
                Modal::UnsavedOnExit { count } => {
                    ui.heading("Unsaved Changes");
                    let what = if *count == 1 {
                        "1 image has".to_owned()
                    } else {
                        format!("{count} images have")
                    };
                    ui.label(format!("{what} unsaved changes. Save before exiting?"));
                    ui.add_space(8.0);
                    buttons(
                        ui,
                        &[
                            ("Save All", ModalResult::SaveAllAndExit),
                            ("Don't Save", ModalResult::ExitWithoutSaving),
                            ("Cancel", ModalResult::Cancelled),
                        ],
                    )
                }
                Modal::SaveConfig { key, options, then } => {
                    ui.heading("Save Configuration");
                    match then.format {
                        pnn_io::FileFormat::WebP => {
                            // pdn-webp's controls (bundled with paint.net 5).
                            ui.horizontal(|ui| {
                                ui.label("Preset:");
                                egui::ComboBox::from_id_salt("webp_preset")
                                    .selected_text(options.webp_preset.name())
                                    .show_ui(ui, |ui| {
                                        for p in pnn_io::WebPPreset::ALL {
                                            ui.selectable_value(&mut options.webp_preset, p, p.name());
                                        }
                                    });
                            });
                            ui.add_enabled_ui(!options.webp_lossless, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Quality:");
                                    ui.add(egui::Slider::new(&mut options.webp_quality, 0..=100));
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label("Effort:");
                                ui.add(egui::Slider::new(&mut options.webp_effort, 0..=9))
                                    .on_hover_text("Higher is slower and smaller");
                            });
                            ui.checkbox(&mut options.webp_lossless, "Lossless");
                        }
                        _ => {
                            ui.horizontal(|ui| {
                                ui.label("Quality:");
                                ui.add(egui::Slider::new(&mut options.jpeg_quality, 0..=100));
                            });
                        }
                    }
                    ui.add_space(8.0);
                    let ok = ModalResult::SaveConfigured {
                        key: *key,
                        options: *options,
                        then: then.clone(),
                    };
                    buttons(ui, &[("OK", ok), ("Cancel", ModalResult::Cancelled)])
                }
                Modal::FlattenForSave { key, then } => {
                    ui.heading("Flatten Image");
                    ui.label("Saving in this file format requires that the image is first \"flattened\", which reduces it to a single layer.");
                    ui.add_space(4.0);
                    ui.label("The image will be flattened, and then saved. You will be able to undo the flatten operation after saving.");
                    ui.add_space(8.0);
                    let ok = ModalResult::FlattenAndSave { key: *key, then: then.clone() };
                    buttons(ui, &[("Flatten", ok), ("Cancel", ModalResult::Cancelled)])
                }
                Modal::PasteLarger { new_layer } => {
                    ui.heading("Paste");
                    ui.label("The image being pasted is larger than the canvas size. What would you like to do?");
                    ui.add_space(8.0);
                    let n = *new_layer;
                    buttons(
                        ui,
                        &[
                            ("Expand canvas", ModalResult::PasteExpand { new_layer: n, expand: true }),
                            ("Keep canvas size", ModalResult::PasteExpand { new_layer: n, expand: false }),
                            ("Cancel", ModalResult::Cancelled),
                        ],
                    )
                }
                Modal::Recover { items } => {
                    ui.heading("Recover images");
                    ui.label("paintnotnet didn't close properly last time. These images had unsaved changes:");
                    ui.add_space(4.0);
                    for r in items.iter() {
                        let from = r
                            .original
                            .as_ref()
                            .map(|p| format!(" ({})", p.display()))
                            .unwrap_or_default();
                        ui.label(format!("• {}{from}", r.title));
                    }
                    ui.add_space(8.0);
                    buttons(
                        ui,
                        &[
                            ("Recover", ModalResult::Recover),
                            ("Discard", ModalResult::DiscardRecovered),
                        ],
                    )
                }
                Modal::About => {
                    ui.heading("paintnotnet");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("A paint.net 5 remake for Linux, written in Rust.");
                    ui.add_space(4.0);
                    ui.label("Includes code ported from Paint.NET 3.36 (MIT) and Paint.NET 3.36's icons (CC BY-NC-ND 2.5). See NOTICE.");
                    ui.add_space(8.0);
                    buttons(ui, &[("OK", ModalResult::Closed)])
                }
                Modal::Error { title, message } => {
                    ui.heading(title.as_str());
                    ui.label(message.as_str());
                    ui.add_space(8.0);
                    buttons(ui, &[("OK", ModalResult::Closed)])
                }
            };
        });
        if result.is_none() && response.should_close() {
            result = Some(ModalResult::Cancelled);
        }
        result
    }
}

fn buttons(ui: &mut egui::Ui, choices: &[(&str, ModalResult)]) -> Option<ModalResult> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            for (label, result) in choices.iter().rev() {
                if ui.button(*label).clicked() {
                    picked = Some(result.clone());
                }
            }
        });
    });
    // Enter accepts the first choice, as the default button does.
    if picked.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        picked = choices.first().map(|(_, r)| r.clone());
    }
    picked
}

fn new_image(ui: &mut egui::Ui, s: &mut NewImage) -> Option<ModalResult> {
    ui.heading("New Image");
    let bytes = s.width as u64 * s.height as u64 * 4;
    ui.label(format!("Image size: {}", human_bytes(bytes)));
    ui.add_space(4.0);
    let (old_w, old_h) = (s.width, s.height);
    egui::Grid::new("new_image").num_columns(3).show(ui, |ui| {
        ui.label("Width:");
        ui.add(egui::DragValue::new(&mut s.width).range(1..=MAX_DIMENSION));
        ui.label("pixels");
        ui.end_row();
        ui.label("Height:");
        ui.add(egui::DragValue::new(&mut s.height).range(1..=MAX_DIMENSION));
        ui.label("pixels");
        ui.end_row();
    });
    ui.checkbox(&mut s.keep_aspect, "Maintain aspect ratio");
    if s.keep_aspect {
        if s.width != old_w {
            s.height = ((s.width as f64 * old_h as f64 / old_w as f64).round() as u32)
                .clamp(1, MAX_DIMENSION);
        } else if s.height != old_h {
            s.width = ((s.height as f64 * old_w as f64 / old_h as f64).round() as u32)
                .clamp(1, MAX_DIMENSION);
        }
    }
    ui.add_space(8.0);
    buttons(
        ui,
        &[
            (
                "OK",
                ModalResult::CreateImage {
                    width: s.width,
                    height: s.height,
                },
            ),
            ("Cancel", ModalResult::Cancelled),
        ],
    )
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
