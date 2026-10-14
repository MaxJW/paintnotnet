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
    // Items go straight into the caller's (wrapping) layout so a long row breaks between them.
    {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label("Tool:");
        let response = egui::containers::menu::MenuButton::from_button(
            egui::Button::image_and_text(icons.image(Icon::for_tool(tool)), "⏷")
                .frame_when_inactive(false),
        )
        .ui(ui, |ui| {
            for &t in ToolKind::ALL.iter() {
                let button = egui::Button::image_and_text(icons.image(Icon::for_tool(t)), t.name())
                    .shortcut_text(t.shortcut().to_string())
                    .selected(t == tool);
                if ui.add(button).clicked() {
                    action = Some(Row2Action::SelectTool(t));
                    ui.close();
                }
            }
        })
        .0
        .on_hover_text("Tool (Alt+T)");
        if std::mem::take(&mut state.open_tool_dropdown) {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&response));
        }
        sep(ui);
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
            sep(ui);
            if ui
                .add_enabled(finish_enabled, egui::Button::new("✔ Finish"))
                .on_hover_text("Finish (Enter)")
                .clicked()
            {
                action = Some(Row2Action::Finish);
            }
        }
    }
    action
}

fn sep(ui: &mut egui::Ui) {
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(2.0);
}

fn options(
    ui: &mut egui::Ui,
    state: &mut Row2State,
    tool: ToolKind,
    s: &mut ToolSettings,
    icons: &Icons,
    palette: &Palette,
) {
    use ToolKind::*;
    match tool {
        RectangleSelect | LassoSelect | EllipseSelect => {
            selection_mode(ui, s, icons);
            if tool == RectangleSelect {
                sep(ui);
                selection_draw_mode(ui, s, icons);
            }
            sep(ui);
            feather(ui, s, state);
            sep(ui);
            selection_quality(ui, s);
        }
        MagicWand => {
            selection_mode(ui, s, icons);
            sep(ui);
            flood_mode(ui, s, icons);
            sep(ui);
            crate::controls::percent(ui, "Tolerance", &mut s.tolerance, palette);
            sep(ui);
            sampling(ui, s);
            sep(ui);
            feather(ui, s, state);
            sep(ui);
            selection_quality(ui, s);
        }
        MoveSelectedPixels => {
            ui.label("Sampling:");
            egui::ComboBox::from_id_salt("resampling")
                .selected_text(format!("{:?}", s.resampling))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.resampling, Resampling::Bicubic, "Bicubic");
                    ui.selectable_value(&mut s.resampling, Resampling::Bilinear, "Bilinear");
                    ui.selectable_value(
                        &mut s.resampling,
                        Resampling::NearestNeighbor,
                        "Nearest Neighbor",
                    );
                });
            sep(ui);
            selection_quality(ui, s);
        }
        MoveSelection | Zoom | Pan => {}
        PaintBucket => {
            flood_mode(ui, s, icons);
            sep(ui);
            crate::controls::percent(ui, "Tolerance", &mut s.tolerance, palette);
            sep(ui);
            sampling(ui, s);
            sep(ui);
            fill_style(ui, s);
            crate::controls::opacity(ui, s, palette);
            sep(ui);
            antialiasing(ui, s, icons);
            blend(ui, s, icons);
        }
        Gradient => {
            for (g, icon) in [
                (GradientType::Linear, Icon::GradientLinear),
                (GradientType::LinearReflected, Icon::GradientReflected),
                (GradientType::Diamond, Icon::GradientDiamond),
                (GradientType::Radial, Icon::GradientRadial),
                (GradientType::Conical, Icon::GradientConical),
            ] {
                if icon_button(ui, icons, icon, g.name(), true, s.gradient_type == g).clicked() {
                    s.gradient_type = g;
                }
            }
            for g in [
                GradientType::SpiralClockwise,
                GradientType::SpiralCounterClockwise,
            ] {
                let label = if g == GradientType::SpiralClockwise {
                    "↻"
                } else {
                    "↺"
                };
                if ui
                    .add(egui::Button::new(label).selected(s.gradient_type == g))
                    .on_hover_text(g.name())
                    .clicked()
                {
                    s.gradient_type = g;
                }
            }
            sep(ui);
            for (m, icon, name) in [
                (GradientColorMode::Color, Icon::ColorMode, "Color mode"),
                (
                    GradientColorMode::Transparency,
                    Icon::TransparencyMode,
                    "Transparency mode",
                ),
            ] {
                if icon_button(ui, icons, icon, name, true, s.gradient_color_mode == m).clicked() {
                    s.gradient_color_mode = m;
                }
            }
            sep(ui);
            egui::ComboBox::from_id_salt("gradient_repeat")
                .selected_text(match s.gradient_repeat {
                    GradientRepeat::NoRepeat => "No Repeat",
                    GradientRepeat::Wrapped => "Repeat Wrapped",
                    GradientRepeat::Reflected => "Repeat Reflected",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut s.gradient_repeat,
                        GradientRepeat::NoRepeat,
                        "No Repeat",
                    );
                    ui.selectable_value(
                        &mut s.gradient_repeat,
                        GradientRepeat::Wrapped,
                        "Repeat Wrapped",
                    );
                    ui.selectable_value(
                        &mut s.gradient_repeat,
                        GradientRepeat::Reflected,
                        "Repeat Reflected",
                    );
                });
            sep(ui);
            antialiasing(ui, s, icons);
            blend(ui, s, icons);
        }
        Paintbrush | Eraser | CloneStamp => {
            crate::controls::brush_size(ui, s, palette);
            sep(ui);
            crate::controls::percent(ui, "Hardness", &mut s.hardness, palette);
            if tool == Paintbrush {
                crate::controls::opacity(ui, s, palette);
            }
            if tool != CloneStamp {
                sep(ui);
                crate::controls::percent(ui, "Spacing", &mut s.spacing, palette);
            }
            sep(ui);
            ui.checkbox(&mut s.smoothing, "Smoothing");
            if tool == Paintbrush {
                sep(ui);
                fill_style(ui, s);
            }
            sep(ui);
            antialiasing(ui, s, icons);
            if tool != Eraser {
                blend(ui, s, icons);
            }
        }
        Pencil => {
            crate::controls::opacity(ui, s, palette);
            sep(ui);
            blend(ui, s, icons);
        }
        ColorPicker => {
            sampling(ui, s);
            sep(ui);
            egui::ComboBox::from_id_salt("picker_size")
                .selected_text(s.picker_sampling.name())
                .show_ui(ui, |ui| {
                    for p in ColorPickerSampling::ALL {
                        ui.selectable_value(&mut s.picker_sampling, p, p.name());
                    }
                });
            sep(ui);
            ui.label("After click:");
            egui::ComboBox::from_id_salt("picker_after")
                .selected_text(match s.picker_after_click {
                    ColorPickerAfterClick::DoNotSwitch => "Do not switch tool",
                    ColorPickerAfterClick::SwitchToPrevious => "Switch to previous tool",
                    ColorPickerAfterClick::SwitchToPencil => "Switch to Pencil tool",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut s.picker_after_click,
                        ColorPickerAfterClick::DoNotSwitch,
                        "Do not switch tool",
                    );
                    ui.selectable_value(
                        &mut s.picker_after_click,
                        ColorPickerAfterClick::SwitchToPrevious,
                        "Switch to previous tool",
                    );
                    ui.selectable_value(
                        &mut s.picker_after_click,
                        ColorPickerAfterClick::SwitchToPencil,
                        "Switch to Pencil tool",
                    );
                });
        }
        Recolor => {
            crate::controls::brush_size(ui, s, palette);
            sep(ui);
            crate::controls::percent(ui, "Tolerance", &mut s.tolerance, palette);
            sep(ui);
            antialiasing(ui, s, icons);
        }
        Text => {
            ui.label("Font:");
            if state.font_families.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut s.font_family).desired_width(110.0));
            } else {
                egui::ComboBox::from_id_salt("font_family")
                    .width(140.0)
                    .height(500.0)
                    .selected_text(s.font_family.clone())
                    .show_ui(ui, |ui| {
                        for f in &state.font_families {
                            ui.selectable_value(&mut s.font_family, f.clone(), f);
                        }
                    });
            }
            ui.add(
                egui::DragValue::new(&mut s.font_size)
                    .range(1.0..=2000.0)
                    .speed(0.5),
            );
            egui::ComboBox::from_id_salt("text_metric")
                .width(60.0)
                .selected_text(match s.text_metric {
                    TextMetric::Points => "pt",
                    TextMetric::Fixed96 => "96 DPI",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut s.text_metric,
                        TextMetric::Points,
                        "Points (image DPI)",
                    );
                    ui.selectable_value(&mut s.text_metric, TextMetric::Fixed96, "Fixed (96 DPI)");
                });
            for (flag, icon, name) in [
                (&mut s.bold, Icon::Bold, "Bold"),
                (&mut s.italic, Icon::Italic, "Italic"),
                (&mut s.underline, Icon::Underline, "Underline"),
            ] {
                if icon_button(ui, icons, icon, name, true, *flag).clicked() {
                    *flag = !*flag;
                }
            }
            sep(ui);
            for (a, icon, name) in [
                (TextAlignment::Left, Icon::AlignLeft, "Align Left"),
                (TextAlignment::Center, Icon::AlignCenter, "Align Center"),
                (TextAlignment::Right, Icon::AlignRight, "Align Right"),
            ] {
                if icon_button(ui, icons, icon, name, true, s.text_align == a).clicked() {
                    s.text_align = a;
                }
            }
            sep(ui);
            crate::controls::opacity(ui, s, palette);
            sep(ui);
            antialiasing(ui, s, icons);
            blend(ui, s, icons);
        }
        LineCurve | Shapes => {
            if tool == Shapes {
                ui.label("Shape:");
                egui::ComboBox::from_id_salt("shape_type")
                    .width(130.0)
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
                    ui.label("Corner Size:");
                    ui.add(
                        egui::DragValue::new(&mut s.corner_radius)
                            .range(0.0..=1000.0)
                            .speed(0.5),
                    );
                }
                sep(ui);
            }
            crate::controls::brush_size(ui, s, palette);
            crate::controls::opacity(ui, s, palette);
            sep(ui);
            if tool == LineCurve {
                for (c, label, hint) in [
                    (
                        CurveType::Straight,
                        "Straight",
                        "Straight segments between nubs",
                    ),
                    (
                        CurveType::Spline,
                        "Spline",
                        "Cubic spline through every nub",
                    ),
                    (
                        CurveType::Bezier,
                        "Bézier",
                        "Bézier curve: the middle nubs pull it",
                    ),
                ] {
                    if ui
                        .add(egui::Button::new(label).selected(s.curve_type == c))
                        .on_hover_text(hint)
                        .clicked()
                    {
                        s.curve_type = c;
                    }
                }
                sep(ui);
            }
            line_style(ui, s, tool == LineCurve);
            sep(ui);
            fill_style(ui, s);
            sep(ui);
            if tool == Shapes {
                for (d, icon, name) in [
                    (
                        ShapeDrawType::Outline,
                        Icon::ShapeOutline,
                        "Draw Shape Outline",
                    ),
                    (
                        ShapeDrawType::Interior,
                        Icon::ShapeInterior,
                        "Draw Filled Shape",
                    ),
                    (
                        ShapeDrawType::Both,
                        Icon::ShapeBoth,
                        "Draw Filled Shape With Outline",
                    ),
                ] {
                    if icon_button(ui, icons, icon, name, true, s.shape_draw == d).clicked() {
                        s.shape_draw = d;
                    }
                }
                sep(ui);
            }
            antialiasing(ui, s, icons);
            blend(ui, s, icons);
        }
    }
}

fn selection_mode(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons) {
    for (mode, icon, hint) in [
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
    ] {
        let tip = format!("Selection mode: {}{hint}", mode.name());
        if icon_button(ui, icons, icon, &tip, true, s.selection_mode == mode).clicked() {
            s.selection_mode = mode;
        }
    }
}

fn selection_draw_mode(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons) {
    let (icon, text) = match s.selection_draw {
        SelectionDrawMode::Normal => (Icon::DrawNormal, "Normal"),
        SelectionDrawMode::FixedRatio { .. } => (Icon::DrawFixedRatio, "Fixed Ratio"),
        SelectionDrawMode::FixedSize { .. } => (Icon::DrawFixedSize, "Fixed Size"),
    };
    ui.add(icons.image(icon));
    egui::ComboBox::from_id_salt("sel_draw")
        .selected_text(text)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(
                    matches!(s.selection_draw, SelectionDrawMode::Normal),
                    "Normal",
                )
                .clicked()
            {
                s.selection_draw = SelectionDrawMode::Normal;
            }
            if ui
                .selectable_label(
                    matches!(s.selection_draw, SelectionDrawMode::FixedRatio { .. }),
                    "Fixed Ratio",
                )
                .clicked()
            {
                s.selection_draw = SelectionDrawMode::FixedRatio {
                    width: 1.0,
                    height: 1.0,
                };
            }
            if ui
                .selectable_label(
                    matches!(s.selection_draw, SelectionDrawMode::FixedSize { .. }),
                    "Fixed Size",
                )
                .clicked()
            {
                s.selection_draw = SelectionDrawMode::FixedSize {
                    width: 100,
                    height: 100,
                };
            }
        });
    match &mut s.selection_draw {
        SelectionDrawMode::Normal => {}
        SelectionDrawMode::FixedRatio { width, height } => {
            ui.label("Width:");
            ui.add(egui::DragValue::new(width).range(0.01..=1000.0).speed(0.05));
            ui.label("Height:");
            ui.add(
                egui::DragValue::new(height)
                    .range(0.01..=1000.0)
                    .speed(0.05),
            );
        }
        SelectionDrawMode::FixedSize { width, height } => {
            ui.label("Width:");
            ui.add(egui::DragValue::new(width).range(1..=65535));
            ui.label("Height:");
            ui.add(egui::DragValue::new(height).range(1..=65535));
        }
    }
}

/// paintnotnet's feather: radius, profile and position for new selections and, while a
/// selection exists, for that selection (live).
fn feather(ui: &mut egui::Ui, s: &mut ToolSettings, state: &mut Row2State) {
    use pnn_core::selection::{FeatherPosition, FeatherProfile};
    ui.label("Feather:");
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

fn selection_quality(ui: &mut egui::Ui, s: &mut ToolSettings) {
    egui::ComboBox::from_id_salt("sel_quality")
        .selected_text(match s.selection_quality {
            SelectionQuality::Antialiased => "Antialiased",
            SelectionQuality::Pixelated => "Pixelated",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut s.selection_quality,
                SelectionQuality::Antialiased,
                "Antialiased selection quality",
            );
            ui.selectable_value(
                &mut s.selection_quality,
                SelectionQuality::Pixelated,
                "Pixelated selection quality",
            );
        })
        .response
        .on_hover_text("Selection clipping mode");
}

fn flood_mode(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons) {
    ui.label("Flood mode:");
    for (m, icon, name) in [
        (FloodMode::Contiguous, Icon::FloodContiguous, "Contiguous"),
        (FloodMode::Global, Icon::FloodGlobal, "Global (hold Shift)"),
    ] {
        if icon_button(ui, icons, icon, name, true, s.flood_mode == m).clicked() {
            s.flood_mode = m;
        }
    }
}

fn sampling(ui: &mut egui::Ui, s: &mut ToolSettings) {
    ui.label("Sampling:");
    egui::ComboBox::from_id_salt("sampling")
        .selected_text(match s.sampling {
            SamplingMode::Layer => "Layer",
            SamplingMode::Image => "Image",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut s.sampling, SamplingMode::Layer, "Layer");
            ui.selectable_value(&mut s.sampling, SamplingMode::Image, "Image");
        });
}

fn antialiasing(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons) {
    let (icon, name) = if s.antialiasing {
        (Icon::AntialiasingOn, "Antialiasing enabled")
    } else {
        (Icon::AntialiasingOff, "Antialiasing disabled")
    };
    if icon_button(ui, icons, icon, name, true, false).clicked() {
        s.antialiasing = !s.antialiasing;
    }
}

/// Start cap (`,`), dash style (`.`) and end cap (`/`); shapes only have the dash style.
fn line_style(ui: &mut egui::Ui, s: &mut ToolSettings, caps: bool) {
    ui.label("Style:");
    if caps {
        egui::ComboBox::from_id_salt("start_cap")
            .width(70.0)
            .selected_text(s.start_cap.name())
            .show_ui(ui, |ui| {
                for c in LineCap::ALL {
                    ui.selectable_value(&mut s.start_cap, c, c.name());
                }
            })
            .response
            .on_hover_text("Start cap (,)");
    }
    egui::ComboBox::from_id_salt("dash_style")
        .width(80.0)
        .selected_text(s.dash.name())
        .show_ui(ui, |ui| {
            for d in DashStyle::ALL {
                ui.selectable_value(&mut s.dash, d, d.name());
            }
        })
        .response
        .on_hover_text("Line style (.)");
    if caps {
        egui::ComboBox::from_id_salt("end_cap")
            .width(70.0)
            .selected_text(s.end_cap.name())
            .show_ui(ui, |ui| {
                for c in LineCap::ALL {
                    ui.selectable_value(&mut s.end_cap, c, c.name());
                }
            })
            .response
            .on_hover_text("End cap (/)");
    }
}

/// Solid colour or a hatch (primary on secondary), as paint.net's Fill dropdown.
fn fill_style(ui: &mut egui::Ui, s: &mut ToolSettings) {
    let name = |f: FillStyle| match f {
        FillStyle::Solid => "Solid Color",
        FillStyle::Hatch(i) => brush::HATCHES[(i as usize).min(brush::HATCHES.len() - 1)],
    };
    ui.label("Fill:");
    egui::ComboBox::from_id_salt("fill_style")
        .selected_text(name(s.fill))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut s.fill, FillStyle::Solid, name(FillStyle::Solid));
            for i in 0..brush::HATCHES.len() as u8 {
                let f = FillStyle::Hatch(i);
                ui.selectable_value(&mut s.fill, f, name(f));
            }
        });
}

fn blend(ui: &mut egui::Ui, s: &mut ToolSettings, icons: &Icons) {
    let (icon, text) = match s.blend {
        ToolBlend::Overwrite => (Icon::BlendOverwrite, "Overwrite".to_owned()),
        ToolBlend::Blend(m) => (Icon::BlendNormal, m.name().to_owned()),
    };
    egui::containers::menu::MenuButton::from_button(
        egui::Button::image_and_text(icons.image(icon), "⏷").frame_when_inactive(false),
    )
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
