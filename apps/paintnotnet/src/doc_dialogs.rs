//! Dialogs that edit the document with a live preview: Layer Properties, Resize, Canvas
//! Size, Rotate / Zoom. Each keeps the original document (a cheap copy-on-write clone); the
//! app rebuilds the preview from it whenever a value changes, and restores it on Cancel.
//!
//! Wording follows paint.net 3.36 `Strings.resx` (CC BY-NC-ND, used unmodified).

use eframe::egui::{self, Align, Layout, vec2};
use pnn_core::transform::{Anchor, Resampling};
use pnn_core::{BlendMode, ColorBgra, Document};
use pnn_effects::rotate_zoom::RotateZoom;

use crate::dialogs::MAX_DIMENSION;

pub enum Outcome {
    /// Still open; `changed` = a value changed this frame (rebuild the preview).
    Pending {
        changed: bool,
    },
    Ok,
    Cancel,
}

pub struct LayerProperties {
    pub index: usize,
    pub name: String,
    pub visible: bool,
    pub blend: BlendMode,
    pub opacity: u8,
}

/// What a canvas-size fill applies to the bottom layer (other layers stay transparent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasFill {
    Secondary,
    Primary,
    Transparent,
    White,
    Black,
}

impl CanvasFill {
    pub const ALL: [CanvasFill; 5] = [
        Self::Secondary,
        Self::Primary,
        Self::Transparent,
        Self::White,
        Self::Black,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Secondary => "Secondary Color",
            Self::Primary => "Primary Color",
            Self::Transparent => "Transparent",
            Self::White => "White",
            Self::Black => "Black",
        }
    }
}

pub struct SizeFields {
    pub by_percent: bool,
    pub percent: u32,
    pub width: u32,
    pub height: u32,
    pub keep_aspect: bool,
    original: (u32, u32),
}

impl SizeFields {
    fn new(w: u32, h: u32) -> Self {
        Self {
            by_percent: true,
            percent: 100,
            width: w,
            height: h,
            keep_aspect: true,
            original: (w, h),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        if self.by_percent {
            let f = self.percent as f64 / 100.0;
            (
                ((self.original.0 as f64 * f).round() as u32).clamp(1, MAX_DIMENSION),
                ((self.original.1 as f64 * f).round() as u32).clamp(1, MAX_DIMENSION),
            )
        } else {
            (self.width, self.height)
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, dpi: f64) -> bool {
        let mut changed = false;
        changed |= ui
            .radio_value(&mut self.by_percent, true, "By percentage:")
            .changed();
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            changed |= ui
                .add_enabled(
                    self.by_percent,
                    egui::DragValue::new(&mut self.percent)
                        .range(1..=10000)
                        .suffix(" %"),
                )
                .changed();
        });
        changed |= ui
            .radio_value(&mut self.by_percent, false, "By absolute size:")
            .changed();
        ui.add_enabled_ui(!self.by_percent, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                changed |= ui
                    .checkbox(&mut self.keep_aspect, "Maintain aspect ratio")
                    .changed();
            });
            let (ow, oh) = (self.width, self.height);
            egui::Grid::new("size_fields")
                .num_columns(4)
                .show(ui, |ui| {
                    ui.add_space(20.0);
                    ui.label("Width:");
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.width).range(1..=MAX_DIMENSION))
                        .changed();
                    ui.label(format!("pixels  ({:.2} in)", self.width as f64 / dpi));
                    ui.end_row();
                    ui.add_space(20.0);
                    ui.label("Height:");
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.height).range(1..=MAX_DIMENSION))
                        .changed();
                    ui.label(format!("pixels  ({:.2} in)", self.height as f64 / dpi));
                    ui.end_row();
                });
            if self.keep_aspect {
                let (w0, h0) = (self.original.0 as f64, self.original.1 as f64);
                if self.width != ow {
                    self.height =
                        ((self.width as f64 * h0 / w0).round() as u32).clamp(1, MAX_DIMENSION);
                } else if self.height != oh {
                    self.width =
                        ((self.height as f64 * w0 / h0).round() as u32).clamp(1, MAX_DIMENSION);
                }
            }
        });
        let (w, h) = self.size();
        ui.label(format!(
            "New size: {} × {}  ({})",
            w,
            h,
            crate::dialogs::human_bytes(w as u64 * h as u64 * 4)
        ));
        changed
    }
}

pub struct Resize {
    pub fields: SizeFields,
    pub resampling: Resampling,
    pub gamma_correct: bool,
}

pub struct CanvasSize {
    pub fields: SizeFields,
    pub anchor: Anchor,
    pub fill: CanvasFill,
}

pub enum Kind {
    LayerProperties(LayerProperties),
    Resize(Resize),
    CanvasSize(CanvasSize),
    RotateZoom(RotateZoom),
}

pub struct DocDialog {
    pub key: u64,
    pub original: Document,
    pub kind: Kind,
}

impl DocDialog {
    pub fn layer_properties(key: u64, doc: &Document) -> Self {
        let index = doc.active_index();
        let l = doc.layer(index);
        Self {
            key,
            original: doc.clone(),
            kind: Kind::LayerProperties(LayerProperties {
                index,
                name: l.name.clone(),
                visible: l.visible,
                blend: l.blend_mode,
                opacity: l.opacity,
            }),
        }
    }

    pub fn resize(key: u64, doc: &Document) -> Self {
        let fields = SizeFields::new(doc.width(), doc.height());
        Self {
            key,
            original: doc.clone(),
            kind: Kind::Resize(Resize {
                fields,
                resampling: Resampling::Bicubic,
                gamma_correct: false,
            }),
        }
    }

    pub fn canvas_size(key: u64, doc: &Document) -> Self {
        let fields = SizeFields::new(doc.width(), doc.height());
        Self {
            key,
            original: doc.clone(),
            kind: Kind::CanvasSize(CanvasSize {
                fields,
                anchor: Anchor::Center,
                fill: CanvasFill::Secondary,
            }),
        }
    }

    pub fn rotate_zoom(key: u64, doc: &Document) -> Self {
        Self {
            key,
            original: doc.clone(),
            kind: Kind::RotateZoom(RotateZoom::default()),
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind {
            Kind::LayerProperties(_) => "Layer Properties",
            Kind::Resize(_) => "Resize",
            Kind::CanvasSize(_) => "Canvas Size",
            Kind::RotateZoom(_) => "Rotate / Zoom",
        }
    }

    /// Whether the dialog previews live on the canvas (size-changing ones apply on OK).
    pub fn live_preview(&self) -> bool {
        matches!(self.kind, Kind::LayerProperties(_) | Kind::RotateZoom(_))
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome {
        let mut changed = false;
        let mut outcome = None;
        let dpi = self.original.dpi;
        let title = self.title();
        // A window rather than a modal so the canvas preview stays visible beside it.
        egui::Window::new(title)
            .id(egui::Id::new("doc_dialog"))
            .collapsible(false)
            .resizable(false)
            .auto_sized()
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(300.0);
                match &mut self.kind {
                    Kind::LayerProperties(p) => {
                        ui.label(egui::RichText::new("General").strong());
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            changed |= ui.text_edit_singleline(&mut p.name).changed();
                        });
                        changed |= ui.checkbox(&mut p.visible, "Visible").changed();
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new("Blending").strong());
                        ui.horizontal(|ui| {
                            ui.label("Mode:");
                            egui::ComboBox::from_id_salt("blend_mode")
                                .selected_text(p.blend.name())
                                .show_ui(ui, |ui| {
                                    for m in BlendMode::ALL {
                                        changed |= ui
                                            .selectable_value(&mut p.blend, m, m.name())
                                            .changed();
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Opacity:");
                            changed |= ui.add(egui::Slider::new(&mut p.opacity, 0..=255)).changed();
                        });
                    }
                    Kind::Resize(r) => {
                        ui.horizontal(|ui| {
                            ui.label("Resampling:");
                            egui::ComboBox::from_id_salt("resampling")
                                .selected_text(r.resampling.name())
                                .show_ui(ui, |ui| {
                                    for m in Resampling::ALL {
                                        changed |= ui
                                            .selectable_value(&mut r.resampling, m, m.name())
                                            .changed();
                                    }
                                });
                        });
                        changed |= r.fields.ui(ui, dpi);
                        changed |= ui
                            .checkbox(&mut r.gamma_correct, "Gamma correction")
                            .changed();
                    }
                    Kind::CanvasSize(c) => {
                        changed |= c.fields.ui(ui, dpi);
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Anchor").strong());
                        egui::Grid::new("anchor")
                            .spacing(vec2(2.0, 2.0))
                            .show(ui, |ui| {
                                for (i, a) in Anchor::ALL.iter().enumerate() {
                                    let text = if *a == c.anchor { "●" } else { "○" };
                                    if ui
                                        .add(egui::Button::new(text).min_size(vec2(24.0, 24.0)))
                                        .clicked()
                                    {
                                        c.anchor = *a;
                                        changed = true;
                                    }
                                    if i % 3 == 2 {
                                        ui.end_row();
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label("Fill:");
                            egui::ComboBox::from_id_salt("canvas_fill")
                                .selected_text(c.fill.name())
                                .show_ui(ui, |ui| {
                                    for f in CanvasFill::ALL {
                                        changed |=
                                            ui.selectable_value(&mut c.fill, f, f.name()).changed();
                                    }
                                });
                        });
                    }
                    Kind::RotateZoom(rz) => {
                        ui.label(egui::RichText::new("Roll / Rotate").strong());
                        ui.horizontal(|ui| {
                            ui.label("Angle:");
                            changed |= ui
                                .add(egui::Slider::new(&mut rz.angle, -180.0..=180.0).suffix("°"))
                                .changed();
                        });
                        ui.label(egui::RichText::new("Pan").strong());
                        ui.horizontal(|ui| {
                            ui.label("X Pan:");
                            changed |= ui
                                .add(egui::Slider::new(&mut rz.pan[0], -1.0..=1.0))
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Y Pan:");
                            changed |= ui
                                .add(egui::Slider::new(&mut rz.pan[1], -1.0..=1.0))
                                .changed();
                        });
                        ui.label(egui::RichText::new("Zoom").strong());
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut rz.zoom, 1.0 / 16.0..=16.0)
                                    .logarithmic(true)
                                    .suffix("x"),
                            )
                            .changed();
                        changed |= ui.checkbox(&mut rz.tiling, "Tiling").changed();
                        changed |= ui
                            .checkbox(&mut rz.preserve_background, "Preserve background")
                            .changed();
                        if ui.button("Reset All").clicked() {
                            *rz = RotateZoom::default();
                            changed = true;
                        }
                    }
                }
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        outcome = Some(Outcome::Cancel);
                    }
                    if ui.button("OK").clicked() {
                        outcome = Some(Outcome::Ok);
                    }
                });
            });
        let (enter, escape) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if outcome.is_none() && escape {
            outcome = Some(Outcome::Cancel);
        }
        if outcome.is_none() && enter && !ctx.egui_wants_keyboard_input() {
            outcome = Some(Outcome::Ok);
        }
        outcome.unwrap_or(Outcome::Pending { changed })
    }

    /// The document as it would be after OK.
    pub fn apply(&self, primary: ColorBgra, secondary: ColorBgra) -> Document {
        use pnn_core::transform;
        let mut doc = self.original.clone();
        match &self.kind {
            Kind::LayerProperties(p) => {
                doc.modify_layer(p.index, None, |l| {
                    l.name = p.name.clone();
                    l.visible = p.visible;
                    l.blend_mode = p.blend;
                    l.opacity = p.opacity;
                });
            }
            Kind::Resize(r) => {
                let (w, h) = r.fields.size();
                if (w, h) != (doc.width(), doc.height()) {
                    let layers = map_layers(&doc, |l| {
                        transform::resample(&l.surface, w, h, r.resampling, r.gamma_correct)
                    });
                    doc.replace(w, h, layers);
                }
            }
            Kind::CanvasSize(c) => {
                let (w, h) = c.fields.size();
                if (w, h) != (doc.width(), doc.height()) {
                    let fill = match c.fill {
                        CanvasFill::Secondary => secondary,
                        CanvasFill::Primary => primary,
                        CanvasFill::Transparent => ColorBgra::TRANSPARENT,
                        CanvasFill::White => ColorBgra::WHITE,
                        CanvasFill::Black => ColorBgra::BLACK,
                    };
                    let layers: Vec<_> = doc
                        .layers()
                        .iter()
                        .enumerate()
                        .map(|(i, l)| {
                            // The fill goes on the bottom (background) layer; others stay clear.
                            let f = if i == 0 { fill } else { ColorBgra::TRANSPARENT };
                            with_surface(l, transform::canvas_resize(&l.surface, w, h, c.anchor, f))
                        })
                        .collect();
                    doc.replace(w, h, layers);
                }
            }
            Kind::RotateZoom(rz) => {
                let i = doc.active_index();
                doc.modify_layer(i, None, |l| {
                    l.surface = pnn_effects::rotate_zoom::rotate_zoom(&l.surface, rz)
                });
            }
        }
        doc
    }

    /// History step name and icon for OK.
    pub fn history_name(&self) -> &'static str {
        match self.kind {
            Kind::LayerProperties(_) => "Layer Properties",
            Kind::Resize(_) => "Resize",
            Kind::CanvasSize(_) => "Canvas Size",
            Kind::RotateZoom(_) => "Rotate / Zoom",
        }
    }
}

/// A copy of `layer` (same identity and properties) with new pixels.
pub fn with_surface(layer: &pnn_core::Layer, surface: pnn_core::Surface) -> pnn_core::Layer {
    let mut l = layer.clone();
    l.surface = surface;
    l
}

pub fn map_layers(
    doc: &Document,
    f: impl Fn(&pnn_core::Layer) -> pnn_core::Surface + Sync,
) -> Vec<pnn_core::Layer> {
    doc.layers().iter().map(|l| with_surface(l, f(l))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_and_absolute_sizes() {
        let mut f = SizeFields::new(800, 600);
        f.percent = 50;
        assert_eq!(f.size(), (400, 300));
        f.by_percent = false;
        f.width = 1000;
        assert_eq!(f.size(), (1000, 600));
    }

    #[test]
    fn layer_properties_apply_to_a_copy() {
        let doc = Document::new_blank(10, 10);
        let mut d = DocDialog::layer_properties(1, &doc);
        if let Kind::LayerProperties(p) = &mut d.kind {
            p.opacity = 100;
            p.blend = BlendMode::Multiply;
            p.name = "Renamed".into();
        }
        let out = d.apply(ColorBgra::BLACK, ColorBgra::WHITE);
        assert_eq!(out.layer(0).opacity, 100);
        assert_eq!(out.layer(0).name, "Renamed");
        assert_eq!(doc.layer(0).opacity, 255, "original untouched");
    }

    #[test]
    fn canvas_size_fills_background_with_secondary() {
        let doc = Document::new_blank(10, 10);
        let mut d = DocDialog::canvas_size(1, &doc);
        if let Kind::CanvasSize(c) = &mut d.kind {
            c.fields.by_percent = false;
            c.fields.keep_aspect = false;
            c.fields.width = 20;
            c.anchor = Anchor::TopLeft;
        }
        let red = ColorBgra::from_rgba(255, 0, 0, 255);
        let out = d.apply(ColorBgra::BLACK, red);
        assert_eq!((out.width(), out.height()), (20, 10));
        assert_eq!(out.layer(0).surface.get(15, 5), red);
        assert_eq!(out.layer(0).surface.get(5, 5), ColorBgra::WHITE);
    }
}
