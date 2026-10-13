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
    pub sampling: SamplingMode,
    pub picker_sampling: ColorPickerSampling,
    pub picker_after_click: ColorPickerAfterClick,
    pub resampling: Resampling,
    pub gradient_type: GradientType,
    pub gradient_color_mode: GradientColorMode,
    pub gradient_repeat: GradientRepeat,
    pub font_family: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub text_align: TextAlignment,
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
            sampling: SamplingMode::Layer,
            picker_sampling: ColorPickerSampling::SinglePixel,
            picker_after_click: ColorPickerAfterClick::DoNotSwitch,
            resampling: Resampling::Bicubic,
            gradient_type: GradientType::Linear,
            gradient_color_mode: GradientColorMode::Color,
            gradient_repeat: GradientRepeat::NoRepeat,
            font_family: "Arial".into(),
            font_size: 12.0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            text_align: TextAlignment::Left,
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
