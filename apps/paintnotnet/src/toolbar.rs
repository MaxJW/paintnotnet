//! Tool bar: row 1 (common commands) and row 2 (tool choice + the active tool's options),
//! following paint.net 5's `Toolbar.html` and screenshots.

use eframe::egui::{self, vec2};
use pnn_core::BlendMode;
use pnn_tools::*;

use crate::commands::Command;
use crate::icons::{Icon, Icons};
use crate::theme::Palette;

fn icon_button(
    ui: &mut egui::Ui,
    icons: &Icons,
    icon: Icon,
    tip: &str,
    enabled: bool,
    selected: bool,
) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = vec2(5.0, 5.0);
        ui.add_enabled(
            enabled,
            egui::Button::image(icons.image(icon))
                .frame_when_inactive(selected)
                .selected(selected),
        )
    })
    .inner
    .on_hover_text(tip)
    .on_disabled_hover_text(tip)
}

fn tip(name: &str, command: Command) -> String {
    match command.shortcut_text() {
        Some(s) => format!("{name} ({s})"),
        None => name.to_owned(),
    }
}

/// Row 1: New, Open, Save | Print | Cut, Copy, Paste, Crop, Deselect | Undo, Redo | Grid, Rulers.
pub fn row1(
    ui: &mut egui::Ui,
    icons: &Icons,
    enabled: &dyn Fn(Command) -> bool,
    checked: &dyn Fn(Command) -> bool,
) -> Option<Command> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        let groups: &[&[(Icon, Command, &str)]] = &[
            &[
                (Icon::New, Command::NewImage, "New"),
                (Icon::Open, Command::Open, "Open"),
                (Icon::Save, Command::Save, "Save"),
            ],
            &[(Icon::Print, Command::Print, "Print")],
            &[
                (Icon::Cut, Command::Cut, "Cut"),
                (Icon::Copy, Command::Copy, "Copy"),
                (Icon::Paste, Command::Paste, "Paste"),
                (Icon::Crop, Command::CropToSelection, "Crop to Selection"),
                (Icon::Deselect, Command::Deselect, "Deselect"),
            ],
            &[
                (Icon::Undo, Command::Undo, "Undo"),
                (Icon::Redo, Command::Redo, "Redo"),
            ],
            &[
                (Icon::Grid, Command::TogglePixelGrid, "Pixel Grid"),
                (Icon::Rulers, Command::ToggleRulers, "Rulers"),
            ],
        ];
        for (gi, group) in groups.iter().enumerate() {
            if gi > 0 {
                ui.add_space(3.0);
                ui.separator();
                ui.add_space(3.0);
            }
            for &(icon, command, name) in group.iter() {
                let selected = checked(command);
                if icon_button(
                    ui,
                    icons,
                    icon,
                    &tip(name, command),
                    enabled(command),
                    selected,
                )
                .clicked()
                {
                    picked = Some(command);
                }
            }
        }
    });
    picked
}

/// Row 2 state that isn't part of the tool settings.
#[derive(Default)]
pub struct Row2State {
    /// The feather controls changed this frame / finished changing (commit to history).
    pub feather_changed: bool,
    pub feather_committed: bool,
    /// Show the feather preview overlay (quick-mask style tint outside the selection).
    pub feather_preview: bool,
    /// Alt+T: open the tool dropdown on the next frame.
    pub open_tool_dropdown: bool,
    /// Installed font families for the Text tool (filled when fonts load).
    pub font_families: Vec<String>,
}

pub enum Row2Action {
    SelectTool(ToolKind),
    Finish,
}

pub fn row2(
    ui: &mut egui::Ui,
    tool: ToolKind,
    s: &mut ToolSettings,
    state: &mut Row2State,
    icons: &Icons,
    palette: &Palette,
    finish_enabled: bool,
) -> Option<Row2Action> {
    let mut action = None;
    // Groups go straight into the caller's wrapping layout; each wraps as a unit.
    ui.spacing_mut().item_spacing.x = 12.0;
    let picked = g(ui, "tool", |ui| {
        let mut picked = None;
        let face = egui::RichText::new(tool.name()).font(egui::FontId::new(
            crate::theme::text::BODY,
            crate::theme::semibold(),
        ));
        let response = egui::containers::menu::MenuButton::from_button(
            egui::Button::image_and_text(icons.image(Icon::for_tool(tool)), face)
                .frame_when_inactive(false),
        )
        .ui(ui, |ui| {
            for &t in ToolKind::ALL.iter() {
                let button = egui::Button::image_and_text(icons.image(Icon::for_tool(t)), t.name())
                    .shortcut_text(t.shortcut().to_string())
                    .selected(t == tool);
                if ui.add(button).clicked() {
                    picked = Some(t);
                    ui.close();
                }
            }
        })
        .0
        .on_hover_text("Tool (Alt+T)");
        if std::mem::take(&mut state.open_tool_dropdown) {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&response));
        }
        picked
    });
    if let Some(t) = picked {
        action = Some(Row2Action::SelectTool(t));
    }
    options(ui, state, tool, s, icons, palette);
    if matches!(
        tool,
        ToolKind::Text
            | ToolKind::LineCurve
            | ToolKind::Shapes
            | ToolKind::Gradient
            | ToolKind::MagicWand
            | ToolKind::MoveSelectedPixels
            | ToolKind::MoveSelection
    ) {
        let finish = g(ui, "finish", |ui| {
            ui.add_enabled(
                finish_enabled,
                egui::Button::image_and_text(icons.image(Icon::Finish), "Finish"),
            )
            .on_hover_text("Finish (Enter)")
            .clicked()
        });
        if finish {
            action = Some(Row2Action::Finish);
        }
    }
    action
}

/// A group of controls that wraps as a unit, with tight spacing inside.
fn g<R>(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    crate::controls::group(ui, id, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        add(ui)
    })
}

fn label(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.label(egui::RichText::new(text).color(palette.weak_text));
}

fn color32(c: pnn_core::ColorBgra) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

fn options(
    ui: &mut egui::Ui,
    state: &mut Row2State,
    tool: ToolKind,
    s: &mut ToolSettings,
    icons: &Icons,
    palette: &Palette,
) {
    use crate::controls::{brush_size, more, opacity, percent};
    use ToolKind::*;
    match tool {
        RectangleSelect | LassoSelect | EllipseSelect => {
            g(ui, "selection mode", |ui| {
                selection_mode(ui, s, icons, palette)
            });
            if tool == RectangleSelect {
                g(ui, "draw mode", |ui| {
                    selection_draw_mode(ui, s, icons, palette)
                });
            }
            g(ui, "feather", |ui| feather(ui, s, state, palette));
            more(ui, icons, |ui| selection_quality(ui, s, icons, palette));
        }
        MagicWand => {
            g(ui, "selection mode", |ui| {
                selection_mode(ui, s, icons, palette)
            });
            g(ui, "flood mode", |ui| flood_mode(ui, s, icons, palette));
            percent(ui, "Tolerance", &mut s.tolerance, palette);
            g(ui, "feather", |ui| feather(ui, s, state, palette));
            more(ui, icons, |ui| {
                sampling(ui, s, icons, palette);
                selection_quality(ui, s, icons, palette);
            });
        }
        MoveSelectedPixels => {
            g(ui, "resampling", |ui| {
                label(ui, "Sampling", palette);
                crate::controls::segmented(
                    ui,
                    icons,
                    &mut s.resampling,
                    &[
                        (Resampling::Bicubic, Seg::Text("Bicubic"), "Bicubic"),
                        (Resampling::Bilinear, Seg::Text("Bilinear"), "Bilinear"),
                        (
                            Resampling::NearestNeighbor,
                            Seg::Text("Nearest"),
                            "Nearest Neighbor",
                        ),
                    ],
                    palette,
                );
            });
            more(ui, icons, |ui| selection_quality(ui, s, icons, palette));
        }
        MoveSelection | Zoom | Pan => {}
        PaintBucket => {
            g(ui, "flood mode", |ui| flood_mode(ui, s, icons, palette));
            percent(ui, "Tolerance", &mut s.tolerance, palette);
            g(ui, "fill", |ui| fill_style(ui, s, palette));
            opacity(ui, s, palette);
            g(ui, "rendering", |ui| rendering(ui, s, icons, palette, true));
            more(ui, icons, |ui| sampling(ui, s, icons, palette));
        }
        Gradient => {
            g(ui, "gradient type", |ui| {
                crate::controls::segmented(
                    ui,
                    icons,
                    &mut s.gradient_type,
                    &[
                        (
                            GradientType::Linear,
                            Seg::Icon(Icon::GradientLinear),
                            "Linear",
                        ),
                        (
                            GradientType::LinearReflected,
                            Seg::Icon(Icon::GradientReflected),
                            "Linear Reflected",
                        ),
                        (
                            GradientType::Diamond,
                            Seg::Icon(Icon::GradientDiamond),
                            "Diamond",
                        ),
                        (
                            GradientType::Radial,
                            Seg::Icon(Icon::GradientRadial),
                            "Radial",
                        ),
                        (
                            GradientType::Conical,
                            Seg::Icon(Icon::GradientConical),
                            "Conical",
                        ),
                        (
                            GradientType::SpiralClockwise,
                            Seg::Icon(Icon::Rotate90Cw),
                            "Spiral Clockwise",
                        ),
                        (
                            GradientType::SpiralCounterClockwise,
                            Seg::Icon(Icon::Rotate90Ccw),
                            "Spiral Counter-Clockwise",
                        ),
                    ],
                    palette,
                );
            });
            g(ui, "gradient mode", |ui| {
                crate::controls::segmented(
                    ui,
                    icons,
                    &mut s.gradient_color_mode,
                    &[
                        (
                            GradientColorMode::Color,
                            Seg::Icon(Icon::ColorMode),
                            "Color mode",
                        ),
                        (
                            GradientColorMode::Transparency,
                            Seg::Icon(Icon::TransparencyMode),
                            "Transparency mode",
                        ),
                    ],
                    palette,
                );
            });
            g(ui, "gradient repeat", |ui| {
                label(ui, "Repeat", palette);
                crate::controls::segmented(
                    ui,
                    icons,
                    &mut s.gradient_repeat,
                    &[
                        (GradientRepeat::NoRepeat, Seg::Text("None"), "No Repeat"),
                        (GradientRepeat::Wrapped, Seg::Text("Wrap"), "Repeat Wrapped"),
                        (
                            GradientRepeat::Reflected,
                            Seg::Text("Reflect"),
                            "Repeat Reflected",
                        ),
                    ],
                    palette,
                );
            });
            g(ui, "rendering", |ui| rendering(ui, s, icons, palette, true));
        }
        Paintbrush | Eraser | CloneStamp => {
            brush_size(ui, s, palette);
            percent(ui, "Hardness", &mut s.hardness, palette);
            if tool == Paintbrush {
                opacity(ui, s, palette);
            }
            g(ui, "smoothing", |ui| {
                ui.checkbox(&mut s.smoothing, "Smoothing")
                    .on_hover_text("Smooth the stroke as you paint");
            });
            g(ui, "rendering", |ui| {
                rendering(ui, s, icons, palette, tool != Eraser)
            });
            if tool != CloneStamp {
                more(ui, icons, |ui| {
                    percent(ui, "Spacing", &mut s.spacing, palette);
                    if tool == Paintbrush {
                        ui.horizontal(|ui| fill_style(ui, s, palette));
                    }
                });
            }
        }
        Pencil => {
            opacity(ui, s, palette);
            g(ui, "rendering", |ui| blend(ui, s, icons, palette));
        }
        ColorPicker => {
            g(ui, "sampling", |ui| sampling(ui, s, icons, palette));
            g(ui, "picker size", |ui| {
                label(ui, "Sample", palette);
                egui::ComboBox::from_id_salt("picker_size")
                    .selected_text(s.picker_sampling.name())
                    .show_ui(ui, |ui| {
                        for p in ColorPickerSampling::ALL {
                            ui.selectable_value(&mut s.picker_sampling, p, p.name());
                        }
                    });
            });
            g(ui, "after click", |ui| {
                label(ui, "After click", palette);
                egui::ComboBox::from_id_salt("picker_after")
                    .selected_text(match s.picker_after_click {
                        ColorPickerAfterClick::DoNotSwitch => "Do not switch tool",
                        ColorPickerAfterClick::SwitchToPrevious => "Switch to previous tool",
                        ColorPickerAfterClick::SwitchToPencil => "Switch to Pencil tool",
                    })
                    .show_ui(ui, |ui| {
                        for (v, name) in [
                            (ColorPickerAfterClick::DoNotSwitch, "Do not switch tool"),
                            (
                                ColorPickerAfterClick::SwitchToPrevious,
                                "Switch to previous tool",
                            ),
                            (
                                ColorPickerAfterClick::SwitchToPencil,
                                "Switch to Pencil tool",
                            ),
                        ] {
                            ui.selectable_value(&mut s.picker_after_click, v, name);
                        }
                    });
            });
        }
        Recolor => {
            brush_size(ui, s, palette);
            percent(ui, "Tolerance", &mut s.tolerance, palette);
            g(ui, "rendering", |ui| {
                crate::controls::toggle(
                    ui,
                    icons,
                    Icon::AntialiasingOn,
                    "Antialiasing",
                    &mut s.antialiasing,
                    palette,
                );
            });
        }
        Text => {
            g(ui, "font", |ui| {
                if state.font_families.is_empty() {
                    ui.add(egui::TextEdit::singleline(&mut s.font_family).desired_width(110.0));
                } else {
                    egui::ComboBox::from_id_salt("font_family")
                        .width(150.0)
                        .height(500.0)
                        .selected_text(s.font_family.clone())
                        .show_ui(ui, |ui| {
                            for f in &state.font_families {
                                ui.selectable_value(&mut s.font_family, f.clone(), f);
                            }
                        })
                        .response
                        .on_hover_text("Font");
                }
                ui.add(
                    egui::DragValue::new(&mut s.font_size)
                        .range(1.0..=2000.0)
                        .speed(0.5)
                        .suffix(match s.text_metric {
                            TextMetric::Points => " pt",
                            TextMetric::Fixed96 => " pt @96",
                        }),
                )
                .on_hover_text("Font size");
            });
            g(ui, "text style", |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (flag, icon, name) in [
                    (&mut s.bold, Icon::Bold, "Bold"),
                    (&mut s.italic, Icon::Italic, "Italic"),
                    (&mut s.underline, Icon::Underline, "Underline"),
                ] {
                    crate::controls::toggle(ui, icons, icon, name, flag, palette);
                }
            });
            g(ui, "align", |ui| {
                crate::controls::segmented(
                    ui,
                    icons,
                    &mut s.text_align,
                    &[
                        (
                            TextAlignment::Left,
                            Seg::Icon(Icon::AlignLeft),
                            "Align Left",
                        ),
                        (
                            TextAlignment::Center,
                            Seg::Icon(Icon::AlignCenter),
                            "Align Center",
                        ),
                        (
                            TextAlignment::Right,
                            Seg::Icon(Icon::AlignRight),
                            "Align Right",
                        ),
                    ],
                    palette,
                );
            });
            opacity(ui, s, palette);
            g(ui, "rendering", |ui| rendering(ui, s, icons, palette, true));
            more(ui, icons, |ui| {
                ui.horizontal(|ui| {
                    label(ui, "Size units", palette);
                    crate::controls::segmented(
                        ui,
                        icons,
                        &mut s.text_metric,
                        &[
                            (
                                TextMetric::Points,
                                Seg::Text("Points"),
                                "Points (image DPI)",
                            ),
                            (TextMetric::Fixed96, Seg::Text("96 DPI"), "Fixed (96 DPI)"),
                        ],
                        palette,
                    );
                });
            });
        }
        LineCurve | Shapes => {
            if tool == Shapes {
                g(ui, "shape", |ui| {
                    egui::ComboBox::from_id_salt("shape_type")
                        .width(140.0)
                        .selected_text(s.shape.name())
                        .height(600.0)
                        .show_ui(ui, |ui| {
                            for shape in ShapeType::ALL {
                                ui.selectable_value(&mut s.shape, shape, shape.name());
                            }
                        })
                        .response
                        .on_hover_text("Shape (A / Shift+A)");
                    if matches!(
                        s.shape,
                        ShapeType::RoundedRectangle | ShapeType::RoundedRectangleCallout
                    ) {
                        label(ui, "Corners", palette);
                        ui.add(
                            egui::DragValue::new(&mut s.corner_radius)
                                .range(0.0..=1000.0)
                                .speed(0.5)
                                .suffix(" px"),
                        );
                    }
                });
                g(ui, "shape draw", |ui| {
                    crate::controls::segmented(
                        ui,
                        icons,
                        &mut s.shape_draw,
                        &[
                            (
                                ShapeDrawType::Outline,
                                Seg::Icon(Icon::ShapeOutline),
                                "Draw Shape Outline",
                            ),
                            (
                                ShapeDrawType::Interior,
                                Seg::Icon(Icon::ShapeInterior),
                                "Draw Filled Shape",
                            ),
                            (
                                ShapeDrawType::Both,
                                Seg::Icon(Icon::ShapeBoth),
                                "Draw Filled Shape With Outline",
                            ),
                        ],
                        palette,
                    );
                });
            }
            brush_size(ui, s, palette);
            opacity(ui, s, palette);
            if tool == LineCurve {
                g(ui, "curve type", |ui| {
                    crate::controls::segmented(
                        ui,
                        icons,
                        &mut s.curve_type,
                        &[
                            (
                                CurveType::Straight,
                                Seg::Text("Straight"),
                                "Straight segments between nubs",
                            ),
                            (
                                CurveType::Spline,
                                Seg::Text("Spline"),
                                "Cubic spline through every nub",
                            ),
                            (
                                CurveType::Bezier,
                                Seg::Text("Bézier"),
                                "Bézier curve: the middle nubs pull it",
                            ),
                        ],
                        palette,
                    );
                });
            }
            g(ui, "line style", |ui| {
                line_style(ui, s, tool == LineCurve, palette)
            });
            g(ui, "rendering", |ui| rendering(ui, s, icons, palette, true));
            more(ui, icons, |ui| {
                ui.horizontal(|ui| fill_style(ui, s, palette));
            });
        }
    }
}

use crate::controls::Seg;

fn selection_mode(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons, palette: &Palette) {
    let items: Vec<(SelectionMode, Seg<'_>, String)> = [
        (SelectionMode::Replace, Icon::SelReplace, ""),
        (SelectionMode::Add, Icon::SelUnion, " (Ctrl + left click)"),
        (
            SelectionMode::Subtract,
            Icon::SelExclude,
            " (Alt + left click)",
        ),
        (
            SelectionMode::Intersect,
            Icon::SelIntersect,
            " (Alt + right click)",
        ),
        (SelectionMode::Invert, Icon::SelXor, " (Ctrl + right click)"),
    ]
    .into_iter()
    .map(|(m, icon, hint)| {
        (
            m,
            Seg::Icon(icon),
            format!("Selection mode: {}{hint}", m.name()),
        )
    })
    .collect();
    let items: Vec<(SelectionMode, Seg<'_>, &str)> =
        items.iter().map(|(m, f, t)| (*m, *f, t.as_str())).collect();
    crate::controls::segmented(ui, icons, &mut s.selection_mode, &items, palette);
}

fn selection_draw_mode(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons, palette: &Palette) {
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Normal,
        Ratio,
        Size,
    }
    let mut kind = match s.selection_draw {
        SelectionDrawMode::Normal => Kind::Normal,
        SelectionDrawMode::FixedRatio { .. } => Kind::Ratio,
        SelectionDrawMode::FixedSize { .. } => Kind::Size,
    };
    if crate::controls::segmented(
        ui,
        icons,
        &mut kind,
        &[
            (Kind::Normal, Seg::Icon(Icon::DrawNormal), "Normal"),
            (Kind::Ratio, Seg::Icon(Icon::DrawFixedRatio), "Fixed Ratio"),
            (Kind::Size, Seg::Icon(Icon::DrawFixedSize), "Fixed Size"),
        ],
        palette,
    ) {
        s.selection_draw = match kind {
            Kind::Normal => SelectionDrawMode::Normal,
            Kind::Ratio => SelectionDrawMode::FixedRatio {
                width: 1.0,
                height: 1.0,
            },
            Kind::Size => SelectionDrawMode::FixedSize {
                width: 100,
                height: 100,
            },
        };
    }
    match &mut s.selection_draw {
        SelectionDrawMode::Normal => {}
        SelectionDrawMode::FixedRatio { width, height } => {
            ui.add(egui::DragValue::new(width).range(0.01..=1000.0).speed(0.05))
                .on_hover_text("Width");
            label(ui, ":", palette);
            ui.add(
                egui::DragValue::new(height)
                    .range(0.01..=1000.0)
                    .speed(0.05),
            )
            .on_hover_text("Height");
        }
        SelectionDrawMode::FixedSize { width, height } => {
            ui.add(egui::DragValue::new(width).range(1..=65535).suffix(" px"))
                .on_hover_text("Width");
            label(ui, "×", palette);
            ui.add(egui::DragValue::new(height).range(1..=65535).suffix(" px"))
                .on_hover_text("Height");
        }
    }
}

/// paintnotnet's feather: radius, profile and position for new selections and, while a
/// selection exists, for that selection (live).
fn feather(ui: &mut egui::Ui, s: &mut ToolSettings, state: &mut Row2State, palette: &Palette) {
    use pnn_core::selection::{FeatherPosition, FeatherProfile};
    label(ui, "Feather", palette);
    let r = ui
        .add(
            egui::DragValue::new(&mut s.feather.radius)
                .range(0.0..=500.0)
                .speed(0.25)
                .suffix(" px"),
        )
        .on_hover_text("Soften selection edges (Edit > Feather Selection, Shift+F6)");
    state.feather_changed |= r.changed();
    state.feather_committed |= r.drag_stopped() || r.lost_focus();
    egui::ComboBox::from_id_salt("feather_profile")
        .selected_text(s.feather.profile.name())
        .width(70.0)
        .show_ui(ui, |ui| {
            for p in FeatherProfile::ALL {
                if ui
                    .selectable_value(&mut s.feather.profile, p, p.name())
                    .changed()
                {
                    state.feather_changed = true;
                    state.feather_committed = true;
                }
            }
        })
        .response
        .on_hover_text("Feather profile");
    egui::ComboBox::from_id_salt("feather_position")
        .selected_text(s.feather.position.name())
        .width(70.0)
        .show_ui(ui, |ui| {
            for p in FeatherPosition::ALL {
                if ui
                    .selectable_value(&mut s.feather.position, p, p.name())
                    .changed()
                {
                    state.feather_changed = true;
                    state.feather_committed = true;
                }
            }
        })
        .response
        .on_hover_text("Where the soft edge sits: inside, centred on, or outside the outline");
    ui.toggle_value(&mut state.feather_preview, "Preview")
        .on_hover_text("Tint everything outside the selection");
}

fn selection_quality(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons, palette: &Palette) {
    ui.horizontal(|ui| {
        label(ui, "Selection edges", palette);
        crate::controls::segmented(
            ui,
            icons,
            &mut s.selection_quality,
            &[
                (
                    SelectionQuality::Antialiased,
                    Seg::Text("Smooth"),
                    "Antialiased selection quality",
                ),
                (
                    SelectionQuality::Pixelated,
                    Seg::Text("Pixelated"),
                    "Pixelated selection quality",
                ),
            ],
            palette,
        );
    });
}

fn flood_mode(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons, palette: &Palette) {
    crate::controls::segmented(
        ui,
        icons,
        &mut s.flood_mode,
        &[
            (
                FloodMode::Contiguous,
                Seg::Icon(Icon::FloodContiguous),
                "Flood mode: Contiguous",
            ),
            (
                FloodMode::Global,
                Seg::Icon(Icon::FloodGlobal),
                "Flood mode: Global (hold Shift)",
            ),
        ],
        palette,
    );
}

fn sampling(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons, palette: &Palette) {
    ui.horizontal(|ui| {
        label(ui, "Sample", palette);
        crate::controls::segmented(
            ui,
            icons,
            &mut s.sampling,
            &[
                (SamplingMode::Layer, Seg::Text("Layer"), "Sample the layer"),
                (SamplingMode::Image, Seg::Text("Image"), "Sample the image"),
            ],
            palette,
        );
    });
}

/// Antialiasing toggle and, when `blending`, the blend mode.
fn rendering(
    ui: &mut egui::Ui,
    s: &mut ToolSettings,
    icons: &Icons,
    palette: &Palette,
    blending: bool,
) {
    let name = if s.antialiasing {
        "Antialiasing enabled"
    } else {
        "Antialiasing disabled"
    };
    crate::controls::toggle(
        ui,
        icons,
        Icon::AntialiasingOn,
        name,
        &mut s.antialiasing,
        palette,
    );
    if blending {
        blend(ui, s, icons, palette);
    }
}

/// Start cap (`,`), dash style (`.`) and end cap (`/`), each with a drawn preview; shapes
/// only have the dash style.
fn line_style(ui: &mut egui::Ui, s: &mut ToolSettings, caps: bool, palette: &Palette) {
    use crate::controls::{cap_sample, dash_sample};
    let ink = palette.text;
    let sample = |ui: &mut egui::Ui, draw: &dyn Fn(&egui::Painter, egui::Rect)| {
        let (rect, _) = ui.allocate_exact_size(vec2(40.0, 14.0), egui::Sense::hover());
        draw(ui.painter(), rect);
    };
    label(ui, "Style", palette);
    if caps {
        let start = s.start_cap;
        egui::ComboBox::from_id_salt("start_cap")
            .width(56.0)
            .selected_text(start.name())
            .show_ui(ui, |ui| {
                for c in LineCap::ALL {
                    ui.horizontal(|ui| {
                        sample(ui, &|p, r| cap_sample(p, mirror(r), c, ink));
                        ui.selectable_value(&mut s.start_cap, c, c.name());
                    });
                }
            })
            .response
            .on_hover_text("Start cap (,)");
    }
    let dash = s.dash;
    sample(ui, &|p, r| dash_sample(p, r, dash, ink));
    egui::ComboBox::from_id_salt("dash_style")
        .width(80.0)
        .selected_text(s.dash.name())
        .show_ui(ui, |ui| {
            for d in DashStyle::ALL {
                ui.horizontal(|ui| {
                    sample(ui, &|p, r| dash_sample(p, r, d, ink));
                    ui.selectable_value(&mut s.dash, d, d.name());
                });
            }
        })
        .response
        .on_hover_text("Line style (.)");
    if caps {
        egui::ComboBox::from_id_salt("end_cap")
            .width(56.0)
            .selected_text(s.end_cap.name())
            .show_ui(ui, |ui| {
                for c in LineCap::ALL {
                    ui.horizontal(|ui| {
                        sample(ui, &|p, r| cap_sample(p, r, c, ink));
                        ui.selectable_value(&mut s.end_cap, c, c.name());
                    });
                }
            })
            .response
            .on_hover_text("End cap (/)");
    }
}

/// Draw a start cap by mirroring an end-cap sample's box (the cap lands on the left).
fn mirror(r: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(r.max.x, r.min.y), egui::pos2(r.min.x, r.max.y))
}

/// Solid colour or a hatch (primary on secondary), as paint.net's Fill dropdown, with
/// each choice drawn in the current colours.
fn fill_style(ui: &mut egui::Ui, s: &mut ToolSettings, palette: &Palette) {
    let name = |f: FillStyle| match f {
        FillStyle::Solid => "Solid Color",
        FillStyle::Hatch(i) => brush::HATCHES[(i as usize).min(brush::HATCHES.len() - 1)],
    };
    let (primary, secondary) = (color32(s.primary), color32(s.secondary));
    let fills: Vec<FillStyle> = std::iter::once(FillStyle::Solid)
        .chain((0..brush::HATCHES.len() as u8).map(FillStyle::Hatch))
        .collect();
    let current = s.fill;
    crate::controls::preview_combo(
        ui,
        "fill_style",
        "Fill",
        name(current),
        |p, r| crate::controls::fill_swatch(p, r, current, primary, secondary),
        palette,
        |ui| {
            for f in fills {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 16.0), egui::Sense::hover());
                    crate::controls::fill_swatch(ui.painter(), rect, f, primary, secondary);
                    ui.selectable_value(&mut s.fill, f, name(f));
                });
            }
        },
    );
}

fn blend(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons, _palette: &Palette) {
    let (icon, text) = match s.blend {
        ToolBlend::Overwrite => (Icon::BlendOverwrite, "Overwrite".to_owned()),
        ToolBlend::Blend(m) => (Icon::BlendNormal, m.name().to_owned()),
    };
    egui::containers::menu::MenuButton::from_button(egui::Button::image_and_text(
        icons.image(icon),
        text.clone(),
    ))
    .ui(ui, |ui| {
        for m in BlendMode::ALL {
            if ui
                .add(egui::Button::new(m.name()).selected(s.blend == ToolBlend::Blend(m)))
                .clicked()
            {
                s.blend = ToolBlend::Blend(m);
                ui.close();
            }
        }
        ui.separator();
        if ui
            .add(
                egui::Button::image_and_text(icons.image(Icon::BlendOverwrite), "Overwrite")
                    .selected(s.blend == ToolBlend::Overwrite),
            )
            .clicked()
        {
            s.blend = ToolBlend::Overwrite;
            ui.close();
        }
    })
    .0
    .on_hover_text(format!("Blend mode: {text}"));
}
