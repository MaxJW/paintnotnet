//! Dialogs that edit the document with a live preview: Layer Properties, Resize, Canvas
//! Size, Rotate / Zoom. Each keeps the original document (a cheap copy-on-write clone); the
//! app rebuilds the preview from it whenever a value changes, and restores it on Cancel.
//!
//! Wording follows paint.net 3.36 `Strings.resx` (CC BY-NC-ND, used unmodified).

use eframe::egui::{self, vec2};
use pnn_core::selection::{Feather, FeatherPosition, FeatherProfile};
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

#[derive(Clone)]
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

#[derive(Clone)]
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
                changed |=
                    crate::controls::checkbox(ui, &mut self.keep_aspect, "Maintain aspect ratio")
                        .changed();
            });
            let (ow, oh) = (self.width, self.height);
            // Indented as a whole (`add_space` inside a grid isn't a cell: egui asserts).
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                egui::Grid::new("size_fields")
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.label("Width:");
                        changed |= ui
                            .add(egui::DragValue::new(&mut self.width).range(1..=MAX_DIMENSION))
                            .changed();
                        ui.label(format!("pixels  ({:.2} in)", self.width as f64 / dpi));
                        ui.end_row();
                        ui.label("Height:");
                        changed |= ui
                            .add(egui::DragValue::new(&mut self.height).range(1..=MAX_DIMENSION))
                            .changed();
                        ui.label(format!("pixels  ({:.2} in)", self.height as f64 / dpi));
                        ui.end_row();
                    });
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

#[derive(Clone)]
pub struct Resize {
    pub fields: SizeFields,
    pub resampling: Resampling,
    pub gamma_correct: bool,
}

#[derive(Clone)]
pub struct CanvasSize {
    pub fields: SizeFields,
    pub anchor: Anchor,
    pub fill: CanvasFill,
}

/// Effects > Object > Feather Object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeatherObject {
    pub radius: f32,
    pub profile: FeatherProfile,
    /// Alpha at or above this counts as part of the object.
    pub threshold: u8,
    /// Treat the canvas border as an edge to feather against.
    pub canvas_edge: bool,
}

impl Default for FeatherObject {
    fn default() -> Self {
        Self {
            radius: 8.0,
            profile: FeatherProfile::Smooth,
            threshold: 128,
            canvas_edge: false,
        }
    }
}

#[derive(Clone)]
pub enum Kind {
    LayerProperties(LayerProperties),
    Resize(Resize),
    CanvasSize(CanvasSize),
    RotateZoom(RotateZoom),
    /// Edit > Feather Selection (Shift+F6) and Effects > Selection > Feather Selection.
    FeatherSelection(Feather),
    FeatherObject(FeatherObject),
    /// Effects > Selection > Blur Selection Edge: soften the selected pixels inward from the
    /// selection's edge.
    BlurSelectionEdge(Feather),
}

#[derive(Clone)]
pub struct DocDialog {
    pub key: u64,
    pub original: Document,
    pub kind: Kind,
    /// Whether the canvas shows this dialog's settings yet (effects preview their defaults
    /// as soon as they open, as paint.net does).
    pub previewed: bool,
}

impl DocDialog {
    pub fn layer_properties(key: u64, doc: &Document) -> Self {
        let index = doc.active_index();
        let l = doc.layer(index);
        Self {
            key,
            original: doc.clone(),
            previewed: false,
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
            previewed: false,
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
            previewed: false,
            kind: Kind::CanvasSize(CanvasSize {
                fields,
                anchor: Anchor::Center,
                fill: CanvasFill::Secondary,
            }),
        }
    }

    pub fn feather_selection(key: u64, doc: &Document) -> Self {
        let mut f = doc.selection().feather();
        if f.is_none() {
            f.radius = 8.0;
        }
        Self {
            key,
            original: doc.clone(),
            previewed: false,
            kind: Kind::FeatherSelection(f),
        }
    }

    pub fn feather_object(key: u64, doc: &Document) -> Self {
        Self {
            key,
            original: doc.clone(),
            previewed: false,
            kind: Kind::FeatherObject(FeatherObject::default()),
        }
    }

    pub fn blur_selection_edge(key: u64, doc: &Document) -> Self {
        let f = Feather {
            radius: 8.0,
            profile: FeatherProfile::Smooth,
            position: FeatherPosition::Inside,
        };
        Self {
            key,
            original: doc.clone(),
            previewed: false,
            kind: Kind::BlurSelectionEdge(f),
        }
    }

    pub fn rotate_zoom(key: u64, doc: &Document) -> Self {
        Self {
            key,
            original: doc.clone(),
            previewed: false,
            kind: Kind::RotateZoom(RotateZoom::default()),
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind {
            Kind::LayerProperties(_) => "Layer Properties",
            Kind::Resize(_) => "Resize",
            Kind::CanvasSize(_) => "Canvas Size",
            Kind::RotateZoom(_) => "Rotate / Zoom",
            Kind::FeatherSelection(_) => "Feather Selection",
            Kind::FeatherObject(_) => "Feather Object",
            Kind::BlurSelectionEdge(_) => "Blur Selection Edge",
        }
    }

    /// Whether the dialog previews live on the canvas (size-changing ones apply on OK).
    /// Kinds whose result takes whole-image work (done in the background on large images).
    pub fn heavy(&self) -> bool {
        matches!(
            self.kind,
            Kind::Resize(_)
                | Kind::CanvasSize(_)
                | Kind::RotateZoom(_)
                | Kind::FeatherObject(_)
                | Kind::BlurSelectionEdge(_)
        )
    }

    pub fn live_preview(&self) -> bool {
        !matches!(self.kind, Kind::Resize(_) | Kind::CanvasSize(_))
    }

    /// Show the selection overlay tint while this dialog is open.
    pub fn wants_feather_preview(&self) -> bool {
        matches!(self.kind, Kind::FeatherSelection(_))
    }

    pub fn show(&mut self, ctx: &egui::Context, icons: &crate::icons::Icons) -> Outcome {
        let mut changed = false;
        let mut outcome = None;
        let dpi = self.original.dpi;
        let title = self.title();
        // A movable window rather than a modal so the live canvas preview can be watched
        // behind or beside it; opens centred, then stays where it was dragged.
        let icon = self.icon();
        crate::dialogs::movable_dialog(
            ctx,
            egui::Window::new(title)
                .id(egui::Id::new(("doc_dialog", title)))
                .collapsible(false)
                .resizable(false)
                .auto_sized()
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center()),
            icons.art(icon),
            title,
            None,
            |ui| {
                ui.set_min_width(300.0);
                ui.set_max_width(420.0);
                match &mut self.kind {
                    Kind::LayerProperties(p) => {
                        ui.label(egui::RichText::new("General").strong());
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            changed |= ui.text_edit_singleline(&mut p.name).changed();
                        });
                        changed |=
                            crate::controls::checkbox(ui, &mut p.visible, "Visible").changed();
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
                        changed |=
                            crate::controls::checkbox(ui, &mut r.gamma_correct, "Gamma correction")
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
                    Kind::FeatherSelection(f) => changed |= feather_controls(ui, f, true),
                    Kind::BlurSelectionEdge(f) => changed |= feather_controls(ui, f, false),
                    Kind::FeatherObject(fo) => {
                        ui.horizontal(|ui| {
                            ui.label("Radius:");
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut fo.radius, 0.5..=250.0)
                                        .logarithmic(true)
                                        .suffix(" px"),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Profile:");
                            for p in FeatherProfile::ALL {
                                changed |=
                                    ui.selectable_value(&mut fo.profile, p, p.name()).changed();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Object threshold:");
                            changed |= ui
                                .add(egui::Slider::new(&mut fo.threshold, 1..=255))
                                .on_hover_text("Alpha at or above this counts as the object")
                                .changed();
                        });
                        changed |= crate::controls::checkbox(
                            ui,
                            &mut fo.canvas_edge,
                            "Feather along the canvas edge too",
                        )
                        .changed();
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
                        changed |=
                            crate::controls::checkbox(ui, &mut rz.tiling, "Tiling").changed();
                        changed |= crate::controls::checkbox(
                            ui,
                            &mut rz.preserve_background,
                            "Preserve background",
                        )
                        .changed();
                        if ui.button("Reset All").clicked() {
                            *rz = RotateZoom::default();
                            changed = true;
                        }
                    }
                }
                ui.add_space(8.0);
                match crate::dialogs::ok_cancel(ui, true) {
                    Some(true) => outcome = Some(Outcome::Ok),
                    Some(false) => outcome = Some(Outcome::Cancel),
                    None => {}
                }
            },
        );
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
    pub fn apply(&self, primary: ColorBgra, secondary: ColorBgra, antialias: bool) -> Document {
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
            Kind::FeatherSelection(f) => {
                let sel = doc.selection().clone().with_feather(*f);
                doc.set_selection(sel);
            }
            Kind::FeatherObject(fo) => {
                let (w, h) = (doc.width(), doc.height());
                let selection = doc.selection().clone();
                let i = doc.active_index();
                doc.modify_layer(i, None, |l| {
                    let feathered = pnn_effects::feather::feather_object(
                        &l.surface,
                        fo.radius,
                        fo.profile,
                        fo.threshold,
                        fo.canvas_edge,
                    );
                    l.surface =
                        clip_to_selection(&l.surface, feathered, &selection, w, h, antialias);
                });
            }
            Kind::BlurSelectionEdge(f) => {
                let (w, h) = (doc.width(), doc.height());
                let selection = doc.selection().clone();
                if !selection.is_empty() {
                    let hard = selection.rasterize_hard(w, h, antialias);
                    let soft = selection
                        .clone()
                        .with_feather(Feather {
                            position: FeatherPosition::Inside,
                            ..*f
                        })
                        .rasterize(w, h, antialias);
                    let i = doc.active_index();
                    let bounds = selection.pixel_bounds(w, h);
                    doc.modify_layer(i, bounds, |l| {
                        pnn_effects::feather::scale_alpha_inside(&mut l.surface, &hard, &soft)
                    });
                }
            }
        }
        doc
    }

    /// The dialog's icon (its heading and its history step).
    pub fn icon(&self) -> crate::icons::Icon {
        use crate::icons::Icon;
        match self.kind {
            Kind::LayerProperties(_) => Icon::LayerProperties,
            Kind::Resize(_) => Icon::Resize,
            Kind::CanvasSize(_) => Icon::CanvasSize,
            Kind::RotateZoom(_) => Icon::RotateZoom,
            Kind::FeatherSelection(_) | Kind::FeatherObject(_) | Kind::BlurSelectionEdge(_) => {
                Icon::Selection
            }
        }
    }

    /// History step name for OK.
    pub fn history_name(&self) -> &'static str {
        match self.kind {
            Kind::LayerProperties(_) => "Layer Properties",
            Kind::Resize(_) => "Resize",
            Kind::CanvasSize(_) => "Canvas Size",
            Kind::RotateZoom(_) => "Rotate / Zoom",
            Kind::FeatherSelection(_) => "Feather Selection",
            Kind::FeatherObject(_) => "Feather Object",
            Kind::BlurSelectionEdge(_) => "Blur Selection Edge",
        }
    }
}

/// Radius, profile and (optionally) position controls. Returns true if anything changed.
fn feather_controls(ui: &mut egui::Ui, f: &mut Feather, with_position: bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Radius:");
        let slider = egui::Slider::new(&mut f.radius, 0.0..=250.0)
            .logarithmic(true)
            .suffix(" px");
        changed |= ui.add(slider).changed();
    });
    ui.horizontal(|ui| {
        ui.label("Profile:");
        for p in FeatherProfile::ALL {
            changed |= ui.selectable_value(&mut f.profile, p, p.name()).changed();
        }
    });
    if with_position {
        ui.horizontal(|ui| {
            ui.label("Position:");
            for p in FeatherPosition::ALL {
                changed |= ui.selectable_value(&mut f.position, p, p.name()).changed();
            }
        });
    }
    changed
}

/// Keep `result` only inside the selection (blended by coverage); everything when nothing
/// is selected.
fn clip_to_selection(
    original: &pnn_core::Surface,
    result: pnn_core::Surface,
    selection: &pnn_core::Selection,
    w: u32,
    h: u32,
    antialias: bool,
) -> pnn_core::Surface {
    if selection.is_empty() {
        return result;
    }
    let mask = selection.rasterize(w, h, antialias);
    let mut out = original.clone();
    for c in mask.tile_coords().collect::<Vec<_>>() {
        let Some(m) = mask.tile(c) else { continue };
        let src = result.tile(c);
        let dst = out.tile_mut(c);
        for (i, (p, &cov)) in dst.pixels_mut().iter_mut().zip(m.pixels()).enumerate() {
            if cov == 0 {
                continue;
            }
            let new = src.map_or(pnn_core::ColorBgra::TRANSPARENT, |t| t.pixels()[i]);
            *p = if cov == 255 {
                new
            } else {
                crate::selection_tools::lerp_premultiplied(*p, new, cov)
            };
        }
    }
    out
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
        let out = d.apply(ColorBgra::BLACK, ColorBgra::WHITE, true);
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
        let out = d.apply(ColorBgra::BLACK, red, true);
        assert_eq!((out.width(), out.height()), (20, 10));
        assert_eq!(out.layer(0).surface.get(15, 5), red);
        assert_eq!(out.layer(0).surface.get(5, 5), ColorBgra::WHITE);
    }
}
