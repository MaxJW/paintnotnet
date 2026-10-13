//! Adjustments and effects. Each declares typed properties so dialogs can be
//! generated, and renders `(src, dst, mask, roi)` so soft selections apply for free.
//!
//! Menu contents follow the paint.net 5 docs (`AdjustmentsMenu.html`, `Effects*Menu.html`).

pub mod adjustments;
pub mod curves;
pub mod feather;
pub mod framework;
pub mod rotate_zoom;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Adjustment {
    AutoLevel,
    BlackAndWhite,
    BrightnessContrast,
    Curves,
    Exposure,
    HighlightsShadows,
    HueSaturation,
    InvertAlpha,
    InvertColors,
    Levels,
    Posterize,
    Sepia,
    TemperatureTint,
}

impl Adjustment {
    /// Adjustments menu order (alphabetical, as in paint.net).
    pub const ALL: [Adjustment; 13] = [
        Self::AutoLevel,
        Self::BlackAndWhite,
        Self::BrightnessContrast,
        Self::Curves,
        Self::Exposure,
        Self::HighlightsShadows,
        Self::HueSaturation,
        Self::InvertAlpha,
        Self::InvertColors,
        Self::Levels,
        Self::Posterize,
        Self::Sepia,
        Self::TemperatureTint,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::AutoLevel => "Auto-Level",
            Self::BlackAndWhite => "Black and White",
            Self::BrightnessContrast => "Brightness / Contrast",
            Self::Curves => "Curves",
            Self::Exposure => "Exposure",
            Self::HighlightsShadows => "Highlights / Shadows",
            Self::HueSaturation => "Hue / Saturation",
            Self::InvertAlpha => "Invert Alpha",
            Self::InvertColors => "Invert Colors",
            Self::Levels => "Levels",
            Self::Posterize => "Posterize",
            Self::Sepia => "Sepia",
            Self::TemperatureTint => "Temperature and Tint",
        }
    }

    /// Whether choosing it opens a dialog (shown with "..." in the menu).
    pub fn has_dialog(self) -> bool {
        !matches!(
            self,
            Self::AutoLevel | Self::BlackAndWhite | Self::InvertAlpha | Self::InvertColors
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Artistic,
    Blurs,
    Color,
    Distort,
    Noise,
    Object,
    Photo,
    Render,
    Selection,
    Stylize,
}

impl Category {
    /// Effects menu submenu order.
    pub const ALL: [Category; 10] = [
        Self::Artistic,
        Self::Blurs,
        Self::Color,
        Self::Distort,
        Self::Noise,
        Self::Object,
        Self::Photo,
        Self::Render,
        Self::Selection,
        Self::Stylize,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Artistic => "Artistic",
            Self::Blurs => "Blurs",
            Self::Color => "Color",
            Self::Distort => "Distort",
            Self::Noise => "Noise",
            Self::Object => "Object",
            Self::Photo => "Photo",
            Self::Render => "Render",
            Self::Selection => "Selection",
            Self::Stylize => "Stylize",
        }
    }
}

/// Index into [`EFFECTS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EffectId(pub u16);

#[derive(Clone, Copy, Debug)]
pub struct EffectInfo {
    pub name: &'static str,
    pub category: Category,
    /// Built in to paint.net 5 (false = BoltBait-pack style addition).
    pub stock: bool,
}

const fn stock(name: &'static str, category: Category) -> EffectInfo {
    EffectInfo {
        name,
        category,
        stock: true,
    }
}

const fn extra(name: &'static str, category: Category) -> EffectInfo {
    EffectInfo {
        name,
        category,
        stock: false,
    }
}

use Category::*;

/// Every effect in the Effects menu. Stock entries are paint.net 5's; extras are the
/// BoltBait-pack effects paintnotnet builds in (Phases 5 and 10).
pub static EFFECTS: &[EffectInfo] = &[
    stock("Ink Sketch", Artistic),
    stock("Oil Painting", Artistic),
    stock("Pencil Sketch", Artistic),
    stock("Bokeh Blur", Blurs),
    stock("Fragment Blur", Blurs),
    stock("Gaussian Blur", Blurs),
    stock("Median Blur", Blurs),
    stock("Motion Blur", Blurs),
    stock("Radial Blur", Blurs),
    stock("Sketch Blur", Blurs),
    stock("Square Blur", Blurs),
    stock("Surface Blur", Blurs),
    stock("Zoom Blur", Blurs),
    stock("Quantize", Color),
    stock("Bulge", Distort),
    stock("Crystalize", Distort),
    stock("Dents", Distort),
    stock("Frosted Glass", Distort),
    stock("Morphology", Distort),
    stock("Pixelate", Distort),
    stock("Polar Inversion", Distort),
    stock("Tile Reflection", Distort),
    stock("Twist", Distort),
    stock("Add Noise", Noise),
    stock("Reduce Noise", Noise),
    stock("Drop Shadow", Object),
    extra("Feather Object", Object),
    extra("Outline Object", Object),
    extra("Inner Shadow", Object),
    extra("Bevel Object", Object),
    stock("Glow", Photo),
    stock("Red Eye Removal", Photo),
    stock("Sharpen", Photo),
    stock("Soften Portrait", Photo),
    stock("Straighten", Photo),
    stock("Vignette", Photo),
    stock("Clouds", Render),
    stock("Julia Fractal", Render),
    stock("Mandelbrot Fractal", Render),
    stock("Turbulence", Render),
    extra("Feather Selection", Selection),
    extra("Blur Selection Edge", Selection),
    extra("Outline Selection", Selection),
    extra("Inner Shadow", Selection),
    extra("Bevel Selection", Selection),
    stock("Edge Detect", Stylize),
    stock("Emboss", Stylize),
    stock("Outline", Stylize),
    stock("Relief", Stylize),
];

pub fn effect(id: EffectId) -> &'static EffectInfo {
    &EFFECTS[id.0 as usize]
}

/// Effects in a category, in menu order.
pub fn effects_in(category: Category) -> impl Iterator<Item = (EffectId, &'static EffectInfo)> {
    EFFECTS
        .iter()
        .enumerate()
        .filter(move |(_, e)| e.category == category)
        .map(|(i, e)| (EffectId(i as u16), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_effects() {
        for c in Category::ALL {
            assert!(effects_in(c).count() > 0, "{c:?}");
        }
    }

    #[test]
    fn documented_counts() {
        // Blurs: 10 effects (EffectsBlursMenu.html); Distort: 9.
        assert_eq!(effects_in(Blurs).filter(|(_, e)| e.stock).count(), 10);
        assert_eq!(effects_in(Distort).count(), 9);
        assert_eq!(Adjustment::ALL.len(), 13);
    }
}
