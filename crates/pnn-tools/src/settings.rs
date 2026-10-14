//! Tool options shown on toolbar row 2, with paint.net's defaults.
//!
//! Defaults: paint.net 3.36 `AppEnvironment.SetToDefaults` (MIT, see NOTICE), updated for 5.x
//! where its docs/screenshots show otherwise (hardness 75%, spacing 15%, Bicubic move
//! sampling, spiral gradients, repeat modes, selection clipping, sampling layer/image).

use pnn_core::{BlendMode, ColorBgra};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectionMode {
    #[default]
    Replace,
    /// Union (Ctrl).
    Add,
    /// Exclude (Alt).
    Subtract,
    /// Intersect (Alt + right mouse).
    Intersect,
    /// Xor (Ctrl + right mouse).
    Invert,
}

impl SelectionMode {
    pub const ALL: [SelectionMode; 5] = [
        Self::Replace,
        Self::Add,
        Self::Subtract,
        Self::Intersect,
        Self::Invert,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Replace => "Replace",
            Self::Add => "Add (union)",
            Self::Subtract => "Subtract (exclude)",
            Self::Intersect => "Intersect",
            Self::Invert => "Invert (xor)",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SelectionDrawMode {
    #[default]
    Normal,
    FixedRatio {
        width: f32,
        height: f32,
    },
    FixedSize {
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionQuality {
    /// Edges are smoothed (paint.net 5 default; to confirm).
    #[default]
    Antialiased,
    Pixelated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloodMode {
    #[default]
    Contiguous,
    Global,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SamplingMode {
    #[default]
    Layer,
    Image,
}

/// How the Magic Wand and Paint Bucket measure colour differences (paintnotnet addition).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToleranceSpace {
    /// OKLab: as different as colours look.
    #[default]
    Perceptual,
    /// paint.net's squared RGB differences.
    Rgb,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToleranceAlphaMode {
    #[default]
    Premultiplied,
    Straight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorPickerAfterClick {
    #[default]
    DoNotSwitch,
    SwitchToPrevious,
    SwitchToPencil,
}

/// Square sample sizes for the Color Picker. paint.net offers six presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorPickerSampling {
    #[default]
    SinglePixel,
    Square3,
    Square5,
    Square7,
    Square9,
    Square11,
}

impl ColorPickerSampling {
    pub const ALL: [ColorPickerSampling; 6] = [
        Self::SinglePixel,
        Self::Square3,
        Self::Square5,
        Self::Square7,
        Self::Square9,
        Self::Square11,
    ];
    pub fn size(self) -> u32 {
        match self {
            Self::SinglePixel => 1,
            Self::Square3 => 3,
            Self::Square5 => 5,
            Self::Square7 => 7,
            Self::Square9 => 9,
            Self::Square11 => 11,
        }
    }
    pub fn name(self) -> String {
        if self.size() == 1 {
            "Single pixel".into()
        } else {
            format!("{0} x {0} region", self.size())
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Resampling {
    #[default]
    Bicubic,
    Bilinear,
    NearestNeighbor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientType {
    #[default]
    Linear,
    LinearReflected,
    Diamond,
    Radial,
    Conical,
    SpiralClockwise,
    SpiralCounterClockwise,
}

impl GradientType {
    pub const ALL: [GradientType; 7] = [
        Self::Linear,
        Self::LinearReflected,
        Self::Diamond,
        Self::Radial,
        Self::Conical,
        Self::SpiralClockwise,
        Self::SpiralCounterClockwise,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::LinearReflected => "Linear (Reflected)",
            Self::Diamond => "Diamond",
            Self::Radial => "Radial",
            Self::Conical => "Conical",
            Self::SpiralClockwise => "Spiral (Clockwise)",
            Self::SpiralCounterClockwise => "Spiral (Counter-Clockwise)",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientColorMode {
    #[default]
    Color,
    Transparency,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientRepeat {
    #[default]
    NoRepeat,
    Wrapped,
    Reflected,
}

/// Shape/line draw type: outline, fill, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShapeDrawType {
    #[default]
    Outline,
    Interior,
    Both,
}

/// Line/Curve interpolation through its four nubs (5.x docs: `LineCurveTool.html`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CurveType {
    Straight,
    #[default]
    Spline,
    Bezier,
}

/// 3.36 `LineCap2`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    #[default]
    Flat,
    Arrow,
    ArrowFilled,
    Rounded,
}

impl LineCap {
    pub const ALL: [LineCap; 4] = [Self::Flat, Self::Arrow, Self::ArrowFilled, Self::Rounded];
    pub fn name(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Arrow => "Arrow",
            Self::ArrowFilled => "Arrow (Filled)",
            Self::Rounded => "Rounded",
        }
    }
}

/// GDI+ `DashStyle` (3.36 `PenInfo.DashStyle`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DashStyle {
    #[default]
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

impl DashStyle {
    pub const ALL: [DashStyle; 5] = [
        Self::Solid,
        Self::Dash,
        Self::Dot,
        Self::DashDot,
        Self::DashDotDot,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Dash => "Dash",
            Self::Dot => "Dot",
            Self::DashDot => "Dash Dot",
            Self::DashDotDot => "Dash Dot Dot",
        }
    }
    /// On/off lengths in multiples of the pen width (GDI+'s patterns).
    pub fn pattern(self) -> &'static [f32] {
        match self {
            Self::Solid => &[],
            Self::Dash => &[3.0, 1.0],
            Self::Dot => &[1.0, 1.0],
            Self::DashDot => &[3.0, 1.0, 1.0, 1.0],
            Self::DashDotDot => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// The Shapes tool's 29 presets, in the 5.x docs' order (`ShapeTools.html`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShapeType {
    #[default]
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Diamond,
    Trapezoid,
    Parallelogram,
    Triangle,
    RightTriangle,
    Pentagon,
    Hexagon,
    Heptagon,
    Octagon,
    Star3,
    Star4,
    Star5,
    Star6,
    Arrow,
    NotchedArrow,
    PentagonArrow,
    ChevronArrow,
    RectangularCallout,
    RoundedRectangleCallout,
    EllipseCallout,
    CloudCallout,
    LightningBolt,
    CheckMark,
    Multiply,
    Gear,
    Heart,
}

impl ShapeType {
    pub const ALL: [ShapeType; 29] = [
        Self::Rectangle,
        Self::RoundedRectangle,
        Self::Ellipse,
        Self::Diamond,
        Self::Trapezoid,
        Self::Parallelogram,
        Self::Triangle,
        Self::RightTriangle,
        Self::Pentagon,
        Self::Hexagon,
        Self::Heptagon,
        Self::Octagon,
        Self::Star3,
        Self::Star4,
        Self::Star5,
        Self::Star6,
        Self::Arrow,
        Self::NotchedArrow,
        Self::PentagonArrow,
        Self::ChevronArrow,
        Self::RectangularCallout,
        Self::RoundedRectangleCallout,
        Self::EllipseCallout,
        Self::CloudCallout,
        Self::LightningBolt,
        Self::CheckMark,
        Self::Multiply,
        Self::Gear,
        Self::Heart,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::RoundedRectangle => "Rounded Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Diamond => "Diamond",
            Self::Trapezoid => "Trapezoid",
            Self::Parallelogram => "Parallelogram",
            Self::Triangle => "Triangle",
            Self::RightTriangle => "Right Triangle",
            Self::Pentagon => "Pentagon",
            Self::Hexagon => "Hexagon",
            Self::Heptagon => "Heptagon",
            Self::Octagon => "Octagon",
            Self::Star3 => "Three-point Star",
            Self::Star4 => "Four-point Star",
            Self::Star5 => "Five-point Star",
            Self::Star6 => "Six-point Star",
            Self::Arrow => "Arrow",
            Self::NotchedArrow => "Notched Arrow",
            Self::PentagonArrow => "Pentagon Arrow",
            Self::ChevronArrow => "Chevron Arrow",
            Self::RectangularCallout => "Rectangular Callout",
            Self::RoundedRectangleCallout => "Rounded Rectangle Callout",
            Self::EllipseCallout => "Ellipse Callout",
            Self::CloudCallout => "Cloud Callout",
            Self::LightningBolt => "Lightning Bolt",
            Self::CheckMark => "Check Mark",
            Self::Multiply => "Multiply",
            Self::Gear => "Gear",
            Self::Heart => "Heart",
        }
    }
    /// `A` / `Shift+A`.
    pub fn cycle(self, forward: bool) -> Self {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        let n = Self::ALL.len();
        Self::ALL[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
    }
}

/// Text size unit (5.x docs, `TextTool.html`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextMetric {
    /// Points at the image's DPI (the default since paint.net 4.1).
    #[default]
    Points,
    /// "Fixed (96 DPI)".
    Fixed96,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

/// Paint fill: solid, or one of the hatch patterns (both colours).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillStyle {
    #[default]
    Solid,
    Hatch(u8),
}

/// How a tool's pixels combine with the layer: a blend mode, or paint.net's Overwrite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolBlend {
    Blend(BlendMode),
    Overwrite,
}

impl Default for ToolBlend {
    fn default() -> Self {
        Self::Blend(BlendMode::Normal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveColor {
    Primary,
    Secondary,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolSettings {
    pub primary: ColorBgra,
    pub secondary: ColorBgra,
    pub active_color: ActiveColor,
    /// Brush width in px; fractional widths are allowed (antialiased).
    pub brush_width: f32,
    /// 0..=100 %.
    pub hardness: u8,
    /// Percent of brush width between stamps.
    pub spacing: u8,
    pub smoothing: bool,
    pub antialiasing: bool,
    pub blend: ToolBlend,
    pub fill: FillStyle,
    pub shape_draw: ShapeDrawType,
    pub selection_mode: SelectionMode,
    pub selection_draw: SelectionDrawMode,
    pub selection_quality: SelectionQuality,
    /// Soft edge for new selections, and the current one's while it's edited
    /// (paintnotnet addition; radius 0 = off).
    pub feather: pnn_core::selection::Feather,
    pub flood_mode: FloodMode,
    /// 0..=100 %.
    pub tolerance: u8,
    pub tolerance_alpha: ToleranceAlphaMode,
    pub tolerance_space: ToleranceSpace,
    pub sampling: SamplingMode,
    pub picker_sampling: ColorPickerSampling,
    pub picker_after_click: ColorPickerAfterClick,
    pub resampling: Resampling,
    pub gradient_type: GradientType,
    pub gradient_color_mode: GradientColorMode,
    pub gradient_repeat: GradientRepeat,
    pub curve_type: CurveType,
    pub start_cap: LineCap,
    pub end_cap: LineCap,
    pub dash: DashStyle,
    pub shape: ShapeType,
    /// Rounded Rectangle corner radius, px.
    pub corner_radius: f32,
    pub font_family: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub text_align: TextAlignment,
    pub text_metric: TextMetric,
}

/// paint.net's brush width limits.
pub const MIN_BRUSH_WIDTH: f32 = 1.0;
pub const MAX_BRUSH_WIDTH: f32 = 2000.0;

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            active_color: ActiveColor::Primary,
            brush_width: 2.0,
            hardness: 75,
            spacing: 15,
            smoothing: true,
            antialiasing: true,
            blend: ToolBlend::default(),
            fill: FillStyle::Solid,
            shape_draw: ShapeDrawType::Outline,
            selection_mode: SelectionMode::Replace,
            selection_draw: SelectionDrawMode::Normal,
            selection_quality: SelectionQuality::Antialiased,
            feather: pnn_core::selection::Feather::NONE,
            flood_mode: FloodMode::Contiguous,
            tolerance: 50,
            tolerance_alpha: ToleranceAlphaMode::Premultiplied,
            tolerance_space: ToleranceSpace::Perceptual,
            sampling: SamplingMode::Layer,
            picker_sampling: ColorPickerSampling::SinglePixel,
            picker_after_click: ColorPickerAfterClick::DoNotSwitch,
            resampling: Resampling::Bicubic,
            gradient_type: GradientType::Linear,
            gradient_color_mode: GradientColorMode::Color,
            gradient_repeat: GradientRepeat::NoRepeat,
            curve_type: CurveType::Spline,
            start_cap: LineCap::Flat,
            end_cap: LineCap::Flat,
            dash: DashStyle::Solid,
            shape: ShapeType::Rectangle,
            corner_radius: 20.0,
            font_family: "Arial".into(),
            font_size: 12.0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            text_align: TextAlignment::Left,
            text_metric: TextMetric::Points,
        }
    }
}

impl ToolSettings {
    /// `[` / `]` (±1) and `Ctrl+[` / `Ctrl+]` (±5).
    pub fn nudge_brush_width(&mut self, delta: f32) {
        self.brush_width = (self.brush_width + delta).clamp(MIN_BRUSH_WIDTH, MAX_BRUSH_WIDTH);
    }

    /// `X`.
    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.primary, &mut self.secondary);
    }

    /// `C`.
    pub fn switch_active_color(&mut self) {
        self.active_color = match self.active_color {
            ActiveColor::Primary => ActiveColor::Secondary,
            ActiveColor::Secondary => ActiveColor::Primary,
        };
    }

    pub fn active_color_value(&self) -> ColorBgra {
        match self.active_color {
            ActiveColor::Primary => self.primary,
            ActiveColor::Secondary => self.secondary,
        }
    }

    pub fn set_active_color_value(&mut self, c: ColorBgra) {
        match self.active_color {
            ActiveColor::Primary => self.primary = c,
            ActiveColor::Secondary => self.secondary = c,
        }
    }

    pub fn set_inactive_color_value(&mut self, c: ColorBgra) {
        match self.active_color {
            ActiveColor::Primary => self.secondary = c,
            ActiveColor::Secondary => self.primary = c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_net_defaults() {
        let s = ToolSettings::default();
        assert_eq!(
            (s.primary, s.secondary),
            (ColorBgra::BLACK, ColorBgra::WHITE)
        );
        assert_eq!(s.brush_width, 2.0);
        assert_eq!(s.tolerance, 50);
        assert!(s.antialiasing);
    }

    #[test]
    fn brush_width_nudges_clamp() {
        let mut s = ToolSettings::default();
        s.nudge_brush_width(-5.0);
        assert_eq!(s.brush_width, 1.0);
        s.nudge_brush_width(5.0);
        assert_eq!(s.brush_width, 6.0);
    }

    #[test]
    fn swap_and_switch() {
        let mut s = ToolSettings::default();
        s.swap_colors();
        assert_eq!(s.primary, ColorBgra::WHITE);
        s.switch_active_color();
        assert_eq!(s.active_color, ActiveColor::Secondary);
        s.set_active_color_value(ColorBgra::from_rgba(1, 2, 3, 255));
        assert_eq!(s.secondary, ColorBgra::from_rgba(1, 2, 3, 255));
    }
}
