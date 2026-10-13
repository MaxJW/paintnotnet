//! paint.net 3.36's 16×16 icons (CC BY-NC-ND 2.5, used unmodified; see
//! `assets/icons/pdn336/README.md`), embedded in the binary.

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use pnn_tools::ToolKind;

macro_rules! icons {
    ($($name:ident => $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon { $($name),* }

        impl Icon {
            pub const ALL: &'static [Icon] = &[$(Icon::$name),*];

            fn png(self) -> &'static [u8] {
                match self {
                    $(Icon::$name => include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"), "/../../assets/icons/pdn336/", $file, ".png"
                    ))),*
                }
            }
        }
    };
}

icons! {
    // Tools
    RectangleSelect => "RectangleSelectToolIcon",
    MoveSelectedPixels => "MoveToolIcon",
    LassoSelect => "LassoSelectToolIcon",
    MoveSelection => "MoveSelectionToolIcon",
    EllipseSelect => "EllipseSelectToolIcon",
    Zoom => "ZoomToolIcon",
    MagicWand => "MagicWandToolIcon",
    Pan => "PanToolIcon",
    PaintBucket => "PaintBucketIcon",
    Gradient => "GradientToolIcon",
    Paintbrush => "PaintBrushToolIcon",
    Eraser => "EraserToolIcon",
    Pencil => "PencilToolIcon",
    ColorPicker => "ColorPickerToolIcon",
    CloneStamp => "CloneStampToolIcon",
    Recolor => "RecoloringToolIcon",
    Text => "TextToolIcon",
    LineCurve => "LineToolIcon",
    Shapes => "RectangleToolIcon",
    // File / Edit
    New => "MenuFileNewIcon",
    Open => "MenuFileOpenIcon",
    Save => "MenuFileSaveIcon",
    SaveAs => "MenuFileSaveAsIcon",
    Print => "MenuFilePrintIcon",
    Close => "MenuFileCloseIcon",
    Acquire => "MenuFileAcquireFromScannerOrCameraIcon",
    Undo => "MenuEditUndoIcon",
    Redo => "MenuEditRedoIcon",
    Cut => "MenuEditCutIcon",
    Copy => "MenuEditCopyIcon",
    Paste => "MenuEditPasteIcon",
    PasteNewLayer => "MenuEditPasteInToNewLayerIcon",
    PasteNewImage => "MenuEditPasteInToNewImageIcon",
    EraseSelection => "MenuEditEraseSelectionIcon",
    FillSelection => "MenuEditFillSelectionIcon",
    InvertSelection => "MenuEditInvertSelectionIcon",
    SelectAll => "MenuEditSelectAllIcon",
    Deselect => "MenuEditDeselectIcon",
    // View
    ZoomIn => "MenuViewZoomInIcon",
    ZoomOut => "MenuViewZoomOutIcon",
    ZoomToWindow => "MenuViewZoomToWindowIcon",
    ZoomToSelection => "MenuViewZoomToSelectionIcon",
    ActualSize => "MenuViewActualSizeIcon",
    Grid => "MenuViewGridIcon",
    Rulers => "MenuViewRulersIcon",
    // Image
    Crop => "MenuImageCropIcon",
    Resize => "MenuImageResizeIcon",
    CanvasSize => "MenuImageCanvasSizeIcon",
    FlipImageHorizontal => "MenuImageFlipHorizontalIcon",
    FlipImageVertical => "MenuImageFlipVerticalIcon",
    Rotate90Cw => "MenuImageRotate90CWIcon",
    Rotate90Ccw => "MenuImageRotate90CCWIcon",
    Rotate180 => "MenuImageRotate180Icon",
    Flatten => "MenuImageFlattenIcon",
    // Layers
    AddLayer => "MenuLayersAddNewLayerIcon",
    DeleteLayer => "MenuLayersDeleteLayerIcon",
    DuplicateLayer => "MenuLayersDuplicateLayerIcon",
    MergeLayerDown => "MenuLayersMergeLayerDownIcon",
    ImportFromFile => "MenuLayersImportFromFileIcon",
    FlipLayerHorizontal => "MenuLayersFlipHorizontalIcon",
    FlipLayerVertical => "MenuLayersFlipVerticalIcon",
    LayerProperties => "MenuLayersLayerPropertiesIcon",
    MoveLayerUp => "MenuLayersMoveLayerUpIcon",
    MoveLayerDown => "MenuLayersMoveLayerDownIcon",
    RotateZoom => "RotateZoomIcon",
    // Adjustments
    AutoLevel => "AutoLevel",
    BlackAndWhite => "BlackAndWhiteIcon",
    BrightnessContrast => "BrightnessAndContrastAdjustment",
    Curves => "CurvesEffect",
    HueSaturation => "HueAndSaturationAdjustment",
    InvertColors => "InvertColorsEffect",
    Levels => "LevelsEffect",
    Posterize => "PosterizeEffectIcon",
    Sepia => "SepiaEffect",
    // Effects (3.36 icons; history entries)
    FxBlur => "BlurEffect",
    FxAddNoise => "AddNoiseEffect",
    FxBulge => "BulgeEffect",
    FxClouds => "CloudsEffect",
    FxDents => "DentsEffectIcon",
    FxEdgeDetect => "EdgeDetectEffect",
    FxEmboss => "EmbossEffect",
    FxFragment => "FragmentEffectIcon",
    FxFrostedGlass => "FrostedGlassEffect",
    FxGlow => "GlowEffect",
    FxInkSketch => "InkSketchEffectIcon",
    FxJulia => "JuliaFractalEffectIcon",
    FxMandelbrot => "MandelbrotFractalEffectIcon",
    FxMedian => "MedianEffectIcon",
    FxMotionBlur => "MotionBlurEffect",
    FxOilPainting => "OilPaintingEffect",
    FxOutline => "OutlineEffectIcon",
    FxPencilSketch => "PencilSketchEffectIcon",
    FxPixelate => "PixelateEffect",
    FxPolarInversion => "PolarInversionEffect",
    FxRadialBlur => "RadialBlurEffect",
    FxRedEye => "RedEyeRemoveEffect",
    FxReduceNoise => "ReduceNoiseEffectIcon",
    FxRelief => "ReliefEffect",
    FxSharpen => "SharpenEffect",
    FxSoftenPortrait => "SoftenPortraitEffectIcon",
    FxSurfaceBlur => "SurfaceBlurEffectIcon",
    FxTile => "TileEffect",
    FxTwist => "TwistEffect",
    FxVignette => "VignetteEffectIcon",
    FxZoomBlur => "ZoomBlurEffect",
    // Windows, settings, help
    ToolsWindow => "MenuWindowToolsIcon",
    HistoryWindow => "MenuWindowHistoryIcon",
    LayersWindow => "MenuWindowLayersIcon",
    ColorsWindow => "MenuWindowColorsIcon",
    Settings => "SettingsIcon",
    Help => "MenuHelpHelpTopicsIcon",
    About => "MenuHelpAboutIcon",
    // Toolbar options
    AntialiasingOn => "AntiAliasingEnabledIcon",
    AntialiasingOff => "AntiAliasingDisabledIcon",
    BlendNormal => "BlendingEnabledIcon",
    BlendOverwrite => "BlendingOverwriteIcon",
    SelReplace => "ToolConfigStrip.SelectionCombineMode.Replace",
    SelUnion => "ToolConfigStrip.SelectionCombineMode.Union",
    SelExclude => "ToolConfigStrip.SelectionCombineMode.Exclude",
    SelIntersect => "ToolConfigStrip.SelectionCombineMode.Intersect",
    SelXor => "ToolConfigStrip.SelectionCombineMode.Xor",
    DrawNormal => "ToolConfigStrip.SelectionDrawModeSplitButton.Normal",
    DrawFixedRatio => "ToolConfigStrip.SelectionDrawModeSplitButton.FixedRatio",
    DrawFixedSize => "ToolConfigStrip.SelectionDrawModeSplitButton.FixedSize",
    FloodContiguous => "ToolConfigStrip.FloodMode.Local",
    FloodGlobal => "ToolConfigStrip.FloodMode.Global",
    ShapeOutline => "ShapeOutlineIcon",
    ShapeInterior => "ShapeInteriorIcon",
    ShapeBoth => "ShapeBothIcon",
    GradientLinear => "LinearClampedGradientIcon",
    GradientReflected => "LinearReflectedGradientIcon",
    GradientDiamond => "LinearDiamondGradientIcon",
    GradientRadial => "RadialGradientIcon",
    GradientConical => "ConicalGradientIcon",
    ColorMode => "AllColorChannelsIcon",
    TransparencyMode => "AlphaChannelOnlyIcon",
    Bold => "FontBoldIcon",
    Italic => "FontItalicIcon",
    Underline => "FontUnderlineIcon",
    AlignLeft => "TextAlignLeftIcon",
    AlignCenter => "TextAlignCenterIcon",
    AlignRight => "TextAlignRightIcon",
    Plus => "PlusButtonIcon",
    Minus => "MinusButtonIcon",
    // Status bar, colors, history
    ImageSize => "ImageSizeIcon",
    CursorXY => "CursorXYIcon",
    Selection => "SelectionIcon",
    Swap => "SwapIcon",
    Reset => "ResetIcon",
    AddColor => "ColorAddOverlay",
    Palettes => "ColorPalettes",
    Swatch => "SwatchIcon",
    HistoryRewind => "HistoryRewindIcon",
    HistoryFastForward => "HistoryFastForwardIcon",
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

/// Icon textures, uploaded once at startup.
pub struct Icons {
    textures: HashMap<Icon, TextureHandle>,
    blank: TextureHandle,
}

/// Icon edge in points.
pub const ICON_SIZE: f32 = 16.0;

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        let textures = Icon::ALL
            .iter()
            .map(|&icon| {
                let rgba = image::load_from_memory_with_format(icon.png(), image::ImageFormat::Png)
                    .expect("embedded icon decodes")
                    .into_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let image = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                (
                    icon,
                    ctx.load_texture(format!("icon-{icon:?}"), image, TextureOptions::NEAREST),
                )
            })
            .collect();
        let blank = ctx.load_texture(
            "icon-blank",
            ColorImage::new([16, 16], vec![egui::Color32::TRANSPARENT; 256]),
            TextureOptions::NEAREST,
        );
        Self { textures, blank }
    }

    /// A transparent 16×16 spacer.
    pub fn blank(&self) -> egui::Image<'static> {
        egui::Image::new((self.blank.id(), egui::vec2(ICON_SIZE, ICON_SIZE)))
    }

    pub fn texture(&self, icon: Icon) -> &TextureHandle {
        &self.textures[&icon]
    }

    /// The icon at its native 16×16 size.
    pub fn image(&self, icon: Icon) -> egui::Image<'static> {
        egui::Image::new((self.textures[&icon].id(), egui::vec2(ICON_SIZE, ICON_SIZE)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_decodes_at_16px() {
        for &icon in Icon::ALL {
            let img =
                image::load_from_memory(icon.png()).unwrap_or_else(|e| panic!("{icon:?}: {e}"));
            assert!(
                img.width() <= 16 && img.height() <= 16,
                "{icon:?} is {}x{}",
                img.width(),
                img.height()
            );
        }
    }
}
