//! The 19 paint.net tools, the pending-session model (editable until Finish),
//! and the overlays/nubs each tool draws.
//!
//! Names and help text are paint.net 3.36's (`Strings.resx`), shown in the status bar as
//! "Name: help" as paint.net 5 does. Tools window order and shortcut letters are from the
//! paint.net 5 docs (`ToolsWindow.html`, `KeyboardMouseCommands.html`).

pub mod brush;
pub mod gradient;
pub mod settings;
pub mod vector;

pub use settings::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    RectangleSelect,
    MoveSelectedPixels,
    LassoSelect,
    MoveSelection,
    EllipseSelect,
    Zoom,
    MagicWand,
    Pan,
    PaintBucket,
    Gradient,
    Paintbrush,
    Eraser,
    Pencil,
    ColorPicker,
    CloneStamp,
    Recolor,
    Text,
    LineCurve,
    Shapes,
}

impl ToolKind {
    /// Tools window order: two columns, read left-to-right then down, as in paint.net 5.
    pub const ALL: [ToolKind; 19] = [
        Self::RectangleSelect,
        Self::MoveSelectedPixels,
        Self::LassoSelect,
        Self::MoveSelection,
        Self::EllipseSelect,
        Self::Zoom,
        Self::MagicWand,
        Self::Pan,
        Self::PaintBucket,
        Self::Gradient,
        Self::Paintbrush,
        Self::Eraser,
        Self::Pencil,
        Self::ColorPicker,
        Self::CloneStamp,
        Self::Recolor,
        Self::Text,
        Self::LineCurve,
        Self::Shapes,
    ];

    /// paint.net's default tool at startup.
    pub const DEFAULT: ToolKind = Self::Paintbrush;

    pub fn name(self) -> &'static str {
        match self {
            Self::RectangleSelect => "Rectangle Select",
            Self::MoveSelectedPixels => "Move Selected Pixels",
            Self::LassoSelect => "Lasso Select",
            Self::MoveSelection => "Move Selection",
            Self::EllipseSelect => "Ellipse Select",
            Self::Zoom => "Zoom",
            Self::MagicWand => "Magic Wand",
            Self::Pan => "Pan",
            Self::PaintBucket => "Paint Bucket",
            Self::Gradient => "Gradient",
            Self::Paintbrush => "Paintbrush",
            Self::Eraser => "Eraser",
            Self::Pencil => "Pencil",
            Self::ColorPicker => "Color Picker",
            Self::CloneStamp => "Clone Stamp",
            Self::Recolor => "Recolor",
            Self::Text => "Text",
            Self::LineCurve => "Line / Curve",
            Self::Shapes => "Shapes",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Self::RectangleSelect => {
                "Click and drag to draw a rectangular selection. Hold shift to constrain to a square."
            }
            Self::MoveSelectedPixels | Self::MoveSelection => {
                "Drag the selection to move. Drag the nubs to scale. Drag with right mouse button to rotate."
            }
            Self::LassoSelect => "Click and drag to draw the outline for a selection area.",
            Self::EllipseSelect => {
                "Click and drag to draw an elliptical selection. Hold shift to constrain to a circle."
            }
            Self::Zoom => "Left click to zoom in, right click to zoom out, middle click to slide",
            Self::MagicWand => "Click to select an area of similar color.",
            Self::Pan => "When zoomed in close, click and drag to navigate the image",
            Self::PaintBucket => {
                "Left click to fill a region with the primary color, right click to fill with the secondary color"
            }
            Self::Gradient => {
                "Click and drag to start drawing. Holding shift constrains the angle. Right mouse button reverses colors."
            }
            Self::Paintbrush => {
                "Left click to draw with primary color, right click to draw with secondary color"
            }
            Self::Eraser => "Click and drag to erase a portion of the image",
            Self::Pencil => {
                "Left click to draw freeform, one-pixel wide lines with the primary color, right click to use the secondary color"
            }
            Self::ColorPicker => {
                "Left click to set primary color, right click to set secondary color"
            }
            Self::CloneStamp => {
                "Hold Ctrl and left click to select an origin. Afterwards, left click and draw to copy"
            }
            Self::Recolor => "Left click to replace the secondary color with the primary color.",
            Self::Text => {
                "Left click to place the text cursor, and then type to enter text. Text is drawn with the primary color."
            }
            Self::LineCurve => {
                "Left click to draw with primary color, right click to use secondary color"
            }
            Self::Shapes => {
                "Click and drag to draw a shape (right click for secondary color). Hold shift to constrain."
            }
        }
    }

    /// The letter that selects this tool (with multi-tap for shared letters).
    pub fn shortcut(self) -> char {
        match self {
            Self::RectangleSelect | Self::LassoSelect | Self::EllipseSelect | Self::MagicWand => {
                'S'
            }
            Self::MoveSelectedPixels | Self::MoveSelection => 'M',
            Self::Zoom => 'Z',
            Self::Pan => 'H',
            Self::PaintBucket => 'F',
            Self::Gradient => 'G',
            Self::Paintbrush => 'B',
            Self::Eraser => 'E',
            Self::Pencil => 'P',
            Self::ColorPicker => 'K',
            Self::CloneStamp => 'L',
            Self::Recolor => 'R',
            Self::Text => 'T',
            Self::LineCurve | Self::Shapes => 'O',
        }
    }

    /// Tooltip text, e.g. "Lasso Select (S)".
    pub fn tooltip(self) -> String {
        format!("{} ({})", self.name(), self.shortcut())
    }

    /// Whether the tool's toolbar shows the selection mode controls.
    pub fn is_selection_tool(self) -> bool {
        matches!(
            self,
            Self::RectangleSelect | Self::LassoSelect | Self::EllipseSelect | Self::MagicWand
        )
    }
}

/// Tools sharing a shortcut letter, in tap order (`KeyboardMouseCommands.html`).
pub fn shortcut_group(letter: char) -> &'static [ToolKind] {
    use ToolKind::*;
    match letter.to_ascii_uppercase() {
        'S' => &[RectangleSelect, LassoSelect, EllipseSelect, MagicWand],
        'M' => &[MoveSelectedPixels, MoveSelection],
        'O' => &[LineCurve, Shapes],
        'Z' => &[Zoom],
        'H' => &[Pan],
        'F' => &[PaintBucket],
        'G' => &[Gradient],
        'B' => &[Paintbrush],
        'E' => &[Eraser],
        'P' => &[Pencil],
        'K' => &[ColorPicker],
        'L' => &[CloneStamp],
        'R' => &[Recolor],
        'T' => &[Text],
        _ => &[],
    }
}

/// Which tool a tool-letter press selects. `taps` counts presses of the same chord under
/// 1 s apart. Shift walks the group backwards: "S, Shift+S ×4 → Rectangle Select;
/// S ×4, Shift+S → Magic Wand".
pub fn tool_for_shortcut(letter: char, shift: bool, taps: u32) -> Option<ToolKind> {
    let group = shortcut_group(letter);
    if group.is_empty() || (shift && group.len() == 1) {
        return None;
    }
    let i = (taps.max(1) as usize - 1) % group.len();
    Some(if shift {
        group[group.len() - 1 - i]
    } else {
        group[i]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ToolKind::*;

    #[test]
    fn documented_tool_cycles() {
        // From KeyboardMouseCommands.html, Tool Shortcuts.
        assert_eq!(tool_for_shortcut('S', false, 1), Some(RectangleSelect));
        assert_eq!(tool_for_shortcut('S', false, 2), Some(LassoSelect));
        assert_eq!(tool_for_shortcut('S', false, 3), Some(EllipseSelect));
        assert_eq!(tool_for_shortcut('S', false, 4), Some(MagicWand));
        assert_eq!(tool_for_shortcut('S', true, 1), Some(MagicWand));
        assert_eq!(tool_for_shortcut('S', true, 2), Some(EllipseSelect));
        assert_eq!(tool_for_shortcut('S', true, 3), Some(LassoSelect));
        assert_eq!(tool_for_shortcut('S', true, 4), Some(RectangleSelect));
        assert_eq!(tool_for_shortcut('M', false, 2), Some(MoveSelection));
        assert_eq!(tool_for_shortcut('M', true, 1), Some(MoveSelection));
        assert_eq!(tool_for_shortcut('O', false, 1), Some(LineCurve));
        assert_eq!(tool_for_shortcut('O', false, 2), Some(Shapes));
        assert_eq!(tool_for_shortcut('O', true, 1), Some(Shapes));
        assert_eq!(
            tool_for_shortcut('S', false, 5),
            Some(RectangleSelect),
            "wraps"
        );
        assert_eq!(tool_for_shortcut('B', false, 1), Some(Paintbrush));
        assert_eq!(
            tool_for_shortcut('B', true, 1),
            None,
            "Shift+B is not a tool key"
        );
        assert_eq!(tool_for_shortcut('Q', false, 1), None);
    }

    #[test]
    fn every_tool_reachable_by_its_letter() {
        for tool in ToolKind::ALL {
            let group = shortcut_group(tool.shortcut());
            let taps = group.iter().position(|&t| t == tool).unwrap() as u32 + 1;
            assert_eq!(tool_for_shortcut(tool.shortcut(), false, taps), Some(tool));
        }
    }
}
