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
    Close => ("x"),
    SaveAll => ("save-all"),
    Exit => ("log-out"),
    Undo => ("undo-2"),
    Redo => ("redo-2"),
    Cut => ("scissors"),
    Copy => ("copy"),
    Paste => ("clipboard-paste"),
    CopyMerged => ("copy-plus"),
    CopySelection => ("clipboard-copy"),
    PasteSelection => ("clipboard-paste"),
    FeatherSelection => ("feather"),
    PasteNewLayer => ("layers-plus"),
    PasteNewImage => ("image-plus"),
    EraseSelection => ("eraser"),
    FillSelection => ("paint-bucket"),
    InvertSelection => ("squares-exclude"),
    SelectAll => ("scan"),
    Deselect => ("square-x"),
    SelectSubject => ("scan-face"),
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
    MoveLayerToTop => ("arrow-up-to-line"),
    MoveLayerToBottom => ("arrow-down-to-line"),
    GoToTopLayer => ("chevrons-up"),
    GoToLayerAbove => ("chevron-up"),
    GoToLayerBelow => ("chevron-down"),
    GoToBottomLayer => ("chevrons-down"),
    ToggleLayerVisibility => ("eye"),
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
    Exposure => ("sun"),
    HighlightsShadows => ("sun-moon"),
    TemperatureTint => ("thermometer"),
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
    FxRemoveBackground => ("square-scissors"),
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
    // Window buttons (own title bar)
    WindowMinimize => ("minus"),
    WindowMaximize => ("square"),
    WindowRestore => ("copy"),
    WindowClose => ("x"),
    // The move tools' cursor outside the selection (drag to rotate).
    RotateCursor => ("rotate-cw"),
}

impl Icon {
    /// Whether the icon has artwork (some effects and adjustments have none).
    pub fn has_art(self) -> bool {
        self.svg().is_some()
    }

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
/// Whether icons are drawn in their colours (Settings › General › Coloured icons).
static COLORED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// How exported icons look now (dark theme, coloured): when it changes, their PNGs must be
/// sent again.
pub fn look() -> (bool, bool) {
    (
        DARK.load(std::sync::atomic::Ordering::Relaxed),
        COLORED.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// The colour an icon carries, following common interface conventions (Apple's HIG, Fluent,
/// Material): colour is kept for meaning, so red only marks destructive commands, green
/// creating something, blue the safe primary ones (undo, redo, save), yellow the folder of
/// Open (the desktop convention); amber, which reads as a warning, isn't used. Tools get a
/// quiet hue per kind so the rail is quick to scan. Everything else keeps the theme's icon
/// colour, and no icon relies on colour alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Undo, redo, history and saving.
    Primary,
    /// Opening files: a yellow folder.
    Folder,
    /// Making something new.
    Create,
    /// Deleting or erasing.
    Destructive,
    SelectTool,
    PaintTool,
    DrawTool,
}

impl Family {
    /// A lighter shade on the dark theme, a deeper one on the light.
    pub fn color(self, dark: bool) -> Color32 {
        let (d, l) = match self {
            Family::Primary => (0x6ea8fe, 0x2563eb),
            Family::Folder => (0xf2c94c, 0xb88a00),
            Family::Create => (0x5ccf85, 0x15803d),
            Family::Destructive => (0xf87171, 0xdc2626),
            Family::SelectTool => (0x4fc8bd, 0x0f8a80),
            Family::PaintTool => (0xf59e5b, 0xc2580f),
            Family::DrawTool => (0xb197fc, 0x7c3aed),
        };
        let c: u32 = if dark { d } else { l };
        Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
    }
}

impl Icon {
    /// The icon's colour family, if it has one.
    pub fn family(self) -> Option<Family> {
        use Icon::*;
        Some(match self {
            Undo | Redo | HistoryRewind | HistoryFastForward | Save | SaveAs | SaveAll => {
                Family::Primary
            }
            Open | ImportFromFile => Family::Folder,
            New | AddLayer | DuplicateLayer | PasteNewLayer | PasteNewImage => Family::Create,
            DeleteLayer | EraseSelection => Family::Destructive,
            RectangleSelect | LassoSelect | EllipseSelect | MagicWand | MoveSelection
            | MoveSelectedPixels => Family::SelectTool,
            Paintbrush | Pencil | Eraser | CloneStamp | Recolor | PaintBucket | Gradient => {
                Family::PaintTool
            }
            Text | LineCurve | Shapes => Family::DrawTool,
            _ => return None,
        })
    }

    /// The icon as a 16×16 PNG in its colour, for the KDE global menu.
    pub fn png(self) -> Option<Vec<u8>> {
        let (svg, filled) = self.svg()?;
        let dark = DARK.load(std::sync::atomic::Ordering::Relaxed);
        let palette = if dark {
            crate::theme::Palette::dark()
        } else {
            crate::theme::Palette::light()
        };
        let mask = rasterize(svg, filled, 16);
        let colored = COLORED.load(std::sync::atomic::Ordering::Relaxed);
        let color = match self.family() {
            Some(f) if colored => f.color(dark),
            _ => palette.icon,
        };
        let [r, g, b, _] = color.to_array();
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
    dark: bool,
    colored: bool,
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
            dark: crate::theme::palette(ctx).dark,
            colored: true,
        }
    }

    fn pixels(ppp: f32) -> u32 {
        (ICON_SIZE * ppp).round().max(1.0) as u32
    }

    /// Call once a frame: follow the theme, and re-rasterise when the display scale changes.
    pub fn update(&mut self, ctx: &egui::Context) {
        let palette = crate::theme::palette(ctx);
        self.tint = palette.icon;
        self.dark = palette.dark;
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

    /// The icon's texture when it has artwork (a dialog heading's tile).
    pub fn art(&self, icon: Icon) -> Option<egui::TextureId> {
        icon.has_art().then(|| self.texture(icon).id())
    }

    /// Draw icons in their colours, or all in the theme's icon colour.
    pub fn set_colored(&mut self, on: bool) {
        self.colored = on;
        COLORED.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// The colour to draw `icon` in: its family's, or the theme's icon colour.
    pub fn tint_of(&self, icon: Icon) -> Color32 {
        match icon.family() {
            Some(f) if self.colored => f.color(self.dark),
            _ => self.tint,
        }
    }

    /// The icon at 16×16 points in its colour.
    pub fn image(&self, icon: Icon) -> egui::Image<'static> {
        egui::Image::new((self.texture(icon).id(), egui::vec2(ICON_SIZE, ICON_SIZE)))
            .tint(self.tint_of(icon))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colour carries meaning (common interface conventions): blue for the safe primary
    /// actions, green for creating, red for destroying, a yellow folder for Open; nothing in
    /// amber, which reads as a warning.
    #[test]
    fn icon_colours_follow_the_conventions() {
        assert_eq!(Icon::Undo.family(), Some(Family::Primary));
        assert_eq!(Icon::Redo.family(), Some(Family::Primary));
        assert_eq!(Icon::Save.family(), Some(Family::Primary));
        assert_eq!(Icon::Open.family(), Some(Family::Folder));
        assert_eq!(Icon::New.family(), Some(Family::Create));
        assert_eq!(Icon::AddLayer.family(), Some(Family::Create));
        assert_eq!(Icon::DeleteLayer.family(), Some(Family::Destructive));
        // Neutral: commands with nothing to signal.
        for icon in [
            Icon::Cut,
            Icon::Copy,
            Icon::Crop,
            Icon::Grid,
            Icon::FxClouds,
        ] {
            assert_eq!(icon.family(), None, "{icon:?}");
        }
        // Only destructive commands are red.
        for &icon in Icon::ALL {
            if icon.family() == Some(Family::Destructive) {
                assert!(matches!(icon, Icon::DeleteLayer | Icon::EraseSelection));
            }
        }
    }

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
