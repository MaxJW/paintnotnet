//! The UI's icons: Lucide 1.48.0 (ISC, partly MIT from Feather; see
//! `assets/icons/lucide/LICENSE`), one line style throughout. The SVGs are embedded,
//! rasterised for the display's scale and tinted to the theme when drawn. Icons without a good
//! Lucide match (some effects and adjustments) have none.

use std::collections::HashMap;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use pnn_tools::ToolKind;
use resvg::{tiny_skia, usvg};

/// An icon's source: a Lucide file name (`filled` draws its shape solid), or none.
macro_rules! source {
    () => {
        None
    };
    ($file:literal) => {
        Some((
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/icons/lucide/",
                $file,
                ".svg"
            )),
            false,
        ))
    };
    (filled $file:literal) => {
        Some((
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/icons/lucide/",
                $file,
                ".svg"
            )),
            true,
        ))
    };
}

macro_rules! icons {
    ($($name:ident => ($($src:tt)*)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon { $($name),* }

        impl Icon {
            pub const ALL: &'static [Icon] = &[$(Icon::$name),*];

            /// The SVG and whether it's drawn filled; `None` for no icon.
            pub(crate) fn svg(self) -> Option<(&'static str, bool)> {
                match self {
                    $(Icon::$name => source!($($src)*)),*
                }
            }
        }
    };
}

icons! {
    // Tools
    RectangleSelect => ("square-dashed"),
    MoveSelectedPixels => ("move"),
    LassoSelect => ("lasso"),
    MoveSelection => ("square-dashed-mouse-pointer"),
    EllipseSelect => ("circle-dashed"),
    Zoom => ("zoom-in"),
    MagicWand => ("wand-sparkles"),
    Pan => ("hand"),
    PaintBucket => ("paint-bucket"),
    Gradient => ("blend"),
    Paintbrush => ("paintbrush"),
    Eraser => ("eraser"),
    Pencil => ("pencil"),
    ColorPicker => ("pipette"),
    CloneStamp => ("stamp"),
    Recolor => ("replace"),
    Text => ("type"),
    LineCurve => ("spline"),
    Shapes => ("shapes"),
    // File / Edit
    New => ("file-plus"),
    Open => ("folder-open"),
    Save => ("save"),
    SaveAs => ("file-pen"),
    Print => ("printer"),
    Close => ("x"),
    Acquire => ("camera"),
    Undo => ("undo-2"),
    Redo => ("redo-2"),
    Cut => ("scissors"),
    Copy => ("copy"),
    Paste => ("clipboard-paste"),
    PasteNewLayer => ("layers-plus"),
    PasteNewImage => ("image-plus"),
    EraseSelection => ("eraser"),
    FillSelection => ("paint-bucket"),
    InvertSelection => ("squares-exclude"),
    SelectAll => ("scan"),
    Deselect => ("square-x"),
    // View
    ZoomIn => ("zoom-in"),
    ZoomOut => ("zoom-out"),
    ZoomToWindow => ("fullscreen"),
    ZoomToSelection => ("scan-search"),
    ActualSize => ("ratio"),
    Grid => ("grid-3x3"),
    Rulers => ("ruler"),
    // Image
    Crop => ("crop"),
    Resize => ("scaling"),
    CanvasSize => ("frame"),
    FlipImageHorizontal => ("arrow-left-right"),
    FlipImageVertical => ("arrow-up-down"),
    Rotate90Cw => ("rotate-cw"),
    Rotate90Ccw => ("rotate-ccw"),
    Rotate180 => ("refresh-cw"),
    Flatten => ("square-stack"),
    // Layers
    AddLayer => ("layers-plus"),
    DeleteLayer => ("trash"),
    DuplicateLayer => ("copy"),
    MergeLayerDown => ("merge"),
    ImportFromFile => ("file-input"),
    FlipLayerHorizontal => ("arrow-left-right"),
    FlipLayerVertical => ("arrow-up-down"),
    LayerProperties => ("sliders-horizontal"),
    MoveLayerUp => ("arrow-up"),
    MoveLayerDown => ("arrow-down"),
    RotateZoom => ("rotate-3d"),
    // Adjustments
    AutoLevel => ("sparkles"),
    BlackAndWhite => ("contrast"),
    BrightnessContrast => ("sun-medium"),
    Curves => ("chart-spline"),
    HueSaturation => ("droplet"),
    InvertColors => (),
    Levels => ("chart-column"),
    Posterize => (),
    Sepia => (),
    // Effects (history entries): only clear matches get an icon
    FxBlur => (),
    FxAddNoise => ("dices"),
    FxBulge => (),
    FxClouds => ("cloud"),
    FxDents => (),
    FxEdgeDetect => (),
    FxEmboss => (),
    FxFragment => (),
    FxFrostedGlass => ("glass-water"),
    FxGlow => ("sparkle"),
    FxInkSketch => ("pen-tool"),
    FxJulia => (),
    FxMandelbrot => (),
    FxMedian => (),
    FxMotionBlur => ("wind"),
    FxOilPainting => ("palette"),
    FxOutline => (),
    FxPencilSketch => ("pencil"),
    FxPixelate => ("grid-3x3"),
    FxPolarInversion => (),
    FxRadialBlur => (),
    FxRedEye => ("eye"),
    FxReduceNoise => (),
    FxRelief => ("mountain"),
    FxSharpen => (),
    FxSoftenPortrait => (),
    FxSurfaceBlur => (),
    FxTile => ("layout-grid"),
    FxTwist => ("tornado"),
    FxVignette => ("aperture"),
    FxZoomBlur => ("focus"),
    // Windows, settings, help
    ToolsWindow => ("wrench"),
    HistoryWindow => ("rotate-ccw-clock"),
    LayersWindow => ("layers"),
    ColorsWindow => ("palette"),
    Settings => ("settings"),
    Help => ("circle-question-mark"),
    About => ("info"),
    // Toolbar options
    AntialiasingOn => ("circle"),
    AntialiasingOff => ("circle-dashed"),
    BlendNormal => ("layers"),
    BlendOverwrite => ("square-stack"),
    SelReplace => ("square"),
    SelUnion => ("squares-unite"),
    SelExclude => ("squares-subtract"),
    SelIntersect => ("squares-intersect"),
    SelXor => ("squares-exclude"),
    DrawNormal => ("square-dashed"),
    DrawFixedRatio => ("ratio"),
    DrawFixedSize => ("ruler-dimension-line"),
    FloodContiguous => ("droplet"),
    FloodGlobal => ("globe"),
    ShapeOutline => ("square"),
    ShapeInterior => (filled "square"),
    ShapeBoth => ("square-square"),
    GradientLinear => ("arrow-right"),
    GradientReflected => ("arrow-left-right"),
    GradientDiamond => ("diamond"),
    GradientRadial => ("circle-dot"),
    GradientConical => ("cone"),
    ColorMode => ("palette"),
    TransparencyMode => ("eye-dashed"),
    Bold => ("bold"),
    Italic => ("italic"),
    Underline => ("underline"),
    AlignLeft => ("text-align-start"),
    AlignCenter => ("text-align-center"),
    AlignRight => ("text-align-end"),
    Plus => ("plus"),
    Minus => ("minus"),
    // Status bar, colors, history
    ImageSize => ("image"),
    CursorXY => ("mouse-pointer-2"),
    Selection => ("square-dashed"),
    Swap => ("arrow-left-right"),
    Reset => ("rotate-ccw"),
    AddColor => ("plus"),
    Palettes => ("swatch-book"),
    Swatch => ("square"),
    HistoryRewind => ("rewind"),
    HistoryFastForward => ("fast-forward"),
    // Toolbar (Phase 14)
    Finish => ("check"),
    More => ("ellipsis"),
    ChevronDown => ("chevron-down"),
    // Dialogs (Phase 15 request)
    AspectLocked => ("link-2"),
    AspectFree => ("link-2-off"),
    SwapOrientation => ("rotate-ccw-square"),
    Warning => ("triangle-alert"),
}

impl Icon {
    pub fn for_tool(tool: ToolKind) -> Icon {
        match tool {
            ToolKind::RectangleSelect => Icon::RectangleSelect,
            ToolKind::MoveSelectedPixels => Icon::MoveSelectedPixels,
            ToolKind::LassoSelect => Icon::LassoSelect,
            ToolKind::MoveSelection => Icon::MoveSelection,
            ToolKind::EllipseSelect => Icon::EllipseSelect,
            ToolKind::Zoom => Icon::Zoom,
            ToolKind::MagicWand => Icon::MagicWand,
            ToolKind::Pan => Icon::Pan,
            ToolKind::PaintBucket => Icon::PaintBucket,
            ToolKind::Gradient => Icon::Gradient,
            ToolKind::Paintbrush => Icon::Paintbrush,
            ToolKind::Eraser => Icon::Eraser,
            ToolKind::Pencil => Icon::Pencil,
            ToolKind::ColorPicker => Icon::ColorPicker,
            ToolKind::CloneStamp => Icon::CloneStamp,
            ToolKind::Recolor => Icon::Recolor,
            ToolKind::Text => Icon::Text,
            ToolKind::LineCurve => Icon::LineCurve,
            ToolKind::Shapes => Icon::Shapes,
        }
    }
}

/// Whether the app is showing its dark theme (icons exported to the KDE global menu, which
/// Plasma draws in the system's colours, follow it).
static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

impl Icon {
    /// The icon as a 16×16 PNG in the theme's icon colour, for the KDE global menu.
    pub fn png(self) -> Option<Vec<u8>> {
        let (svg, filled) = self.svg()?;
        let dark = DARK.load(std::sync::atomic::Ordering::Relaxed);
        let palette = if dark {
            crate::theme::Palette::dark()
        } else {
            crate::theme::Palette::light()
        };
        let mask = rasterize(svg, filled, 16);
        let [r, g, b, _] = palette.icon.to_array();
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for p in &mask.pixels {
            rgba.extend_from_slice(&[r, g, b, p.a()]);
        }
        let mut png = Vec::new();
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut png),
            &rgba,
            16,
            16,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
        Some(png)
    }
}

/// Icon textures: white masks at the display's scale, tinted when drawn.
pub struct Icons {
    textures: HashMap<Icon, TextureHandle>,
    blank: TextureHandle,
    /// Pixels per point the textures were rasterised for.
    ppp: f32,
    tint: Color32,
}

/// Icon edge in points.
pub const ICON_SIZE: f32 = 16.0;

/// Rasterise a Lucide SVG at `px`×`px` as a white mask.
fn rasterize(svg: &str, filled: bool, px: u32) -> ColorImage {
    let mut text = svg.replace("currentColor", "#ffffff");
    if filled {
        text = text.replace("fill=\"none\"", "fill=\"#ffffff\"");
    }
    let tree =
        usvg::Tree::from_str(&text, &usvg::Options::default()).expect("embedded icon parses");
    let mut pixmap = tiny_skia::Pixmap::new(px, px).expect("icon size");
    let scale = px as f32 / tree.size().width();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data())
}

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        let ppp = ctx.pixels_per_point();
        let px = Self::pixels(ppp);
        let textures = Icon::ALL
            .iter()
            .filter_map(|&icon| {
                let (svg, filled) = icon.svg()?;
                let image = rasterize(svg, filled, px);
                Some((
                    icon,
                    ctx.load_texture(format!("icon-{icon:?}"), image, TextureOptions::LINEAR),
                ))
            })
            .collect();
        let blank = ctx.load_texture(
            "icon-blank",
            ColorImage::new([16, 16], vec![Color32::TRANSPARENT; 256]),
            TextureOptions::NEAREST,
        );
        Self {
            textures,
            blank,
            ppp,
            tint: crate::theme::palette(ctx).icon,
        }
    }

    fn pixels(ppp: f32) -> u32 {
        (ICON_SIZE * ppp).round().max(1.0) as u32
    }

    /// Call once a frame: follow the theme, and re-rasterise when the display scale changes.
    pub fn update(&mut self, ctx: &egui::Context) {
        let palette = crate::theme::palette(ctx);
        self.tint = palette.icon;
        DARK.store(palette.dark, std::sync::atomic::Ordering::Relaxed);
        let ppp = ctx.pixels_per_point();
        if (ppp - self.ppp).abs() > 0.01 {
            self.ppp = ppp;
            let px = Self::pixels(ppp);
            for (icon, handle) in &mut self.textures {
                if let Some((svg, filled)) = icon.svg() {
                    handle.set(rasterize(svg, filled, px), TextureOptions::LINEAR);
                }
            }
        }
    }

    /// The colour icons are drawn in for the current theme.
    pub fn tint(&self) -> Color32 {
        self.tint
    }

    /// A transparent 16×16 spacer.
    pub fn blank(&self) -> egui::Image<'static> {
        egui::Image::new((self.blank.id(), egui::vec2(ICON_SIZE, ICON_SIZE)))
    }

    /// The icon's texture (a white mask: draw it tinted), or a transparent one.
    pub fn texture(&self, icon: Icon) -> &TextureHandle {
        self.textures.get(&icon).unwrap_or(&self.blank)
    }

    /// The icon at 16×16 points in the theme's icon colour.
    pub fn image(&self, icon: Icon) -> egui::Image<'static> {
        egui::Image::new((self.texture(icon).id(), egui::vec2(ICON_SIZE, ICON_SIZE)))
            .tint(self.tint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_renders_something_at_16px() {
        for &icon in Icon::ALL {
            let Some((svg, filled)) = icon.svg() else {
                continue;
            };
            let image = rasterize(svg, filled, 16);
            let ink = image.pixels.iter().filter(|p| p.a() > 64).count();
            assert!(ink >= 12, "{icon:?} draws {ink} px");
        }
    }

    #[test]
    fn tools_and_toolbar_buttons_all_have_icons() {
        for tool in ToolKind::ALL {
            assert!(Icon::for_tool(tool).svg().is_some(), "{tool:?}");
        }
        let without: Vec<Icon> = Icon::ALL
            .iter()
            .copied()
            .filter(|i| i.svg().is_none())
            .collect();
        assert!(
            without.iter().all(|i| format!("{i:?}").starts_with("Fx")
                || matches!(i, Icon::InvertColors | Icon::Posterize | Icon::Sepia)),
            "only some effects and adjustments lack an icon: {without:?}"
        );
    }

    #[test]
    fn filled_icons_are_solid() {
        let (svg, _) = Icon::ShapeInterior.svg().unwrap();
        let outline = rasterize(svg, false, 16);
        let filled = rasterize(svg, true, 16);
        let ink = |i: &ColorImage| i.pixels.iter().filter(|p| p.a() > 64).count();
        assert!(ink(&filled) > ink(&outline) * 2);
    }
}
