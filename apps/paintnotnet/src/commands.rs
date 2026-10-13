//! Commands and the keymap.
//!
//! Every keyboard chord in paint.net 5's `KeyboardMouseCommands.html`
//! (`docs/reference/keybindings.md`) is bound here, including ones whose features arrive in
//! later phases. `tests/keymap_conformance.rs` parses the reference table and checks each
//! chord resolves to the documented command.
//!
//! Matching rules learned the hard way:
//! - egui has no `Key` for shifted symbols (e.g. `~`), so a chord matches the logical key
//!   *or* the physical key. Some bindings ignore Shift (`Ctrl++` needs Shift on US layouts).
//! - egui recomputes `Event::Key::repeat` itself; auto-repeat never counts as a new tap.

use std::time::{Duration, Instant};

use eframe::egui::{self, Key};
use pnn_effects::{Adjustment, EffectId};

/// A floating utility window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    Tools,
    History,
    Layers,
    Colors,
}

impl Panel {
    pub const ALL: [Panel; 4] = [Self::Tools, Self::History, Self::Layers, Self::Colors];
    pub fn name(self) -> &'static str {
        match self {
            Self::Tools => "Tools",
            Self::History => "History",
            Self::Layers => "Layers",
            Self::Colors => "Colors",
        }
    }
}

/// Top-level menus, opened with Alt+letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuId {
    File,
    Edit,
    View,
    Image,
    Layers,
    Adjustments,
    Effects,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Units {
    Pixels,
    Inches,
    Centimeters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    // File
    NewImage,
    Open,
    Acquire,
    Close,
    Save,
    SaveAs,
    SaveAll,
    Print,
    Exit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    CopyMerged,
    Paste,
    PasteIntoNewLayer,
    PasteIntoNewImage,
    CopySelection,
    PasteSelection,
    EraseSelection,
    FillSelection,
    FillSelectionSecondary,
    InvertSelection,
    SelectAll,
    Deselect,
    /// paintnotnet addition (Shift+F6).
    FeatherSelection,
    // View
    ZoomIn,
    ZoomOut,
    ZoomToWindow,
    ZoomToSelection,
    ActualSize,
    TogglePixelGrid,
    ToggleRulers,
    SetUnits(Units),
    // Image
    CropToSelection,
    Resize,
    CanvasSize,
    FlipImageHorizontal,
    FlipImageVertical,
    RotateImage90Clockwise,
    RotateImage90CounterClockwise,
    RotateImage180,
    ColorProfile,
    Flatten,
    // Layers
    AddNewLayer,
    DeleteLayer,
    DuplicateLayer,
    MergeLayerDown,
    ToggleLayerVisibility,
    ImportFromFile,
    FlipLayerHorizontal,
    FlipLayerVertical,
    RotateLayer180,
    RotateZoom,
    GoToTopLayer,
    GoToLayerAbove,
    GoToLayerBelow,
    GoToBottomLayer,
    MoveLayerToTop,
    MoveLayerUp,
    MoveLayerDown,
    MoveLayerToBottom,
    LayerProperties,
    // Adjustments / Effects
    Adjustment(Adjustment),
    RepeatEffect,
    Effect(EffectId),
    // Utility windows, settings, help
    ToggleWindow(Panel),
    ResetWindow(Panel),
    Settings,
    HelpTopics,
    About,
    OpenMenu(MenuId),
    // Image list
    NextImage,
    PreviousImage,
    MoveImageLeft,
    MoveImageRight,
    /// 1-based (Ctrl+N / Alt+N).
    SwitchToImage(u8),
    ImageContextMenu,
    // Scrolling
    ScrollPageUp,
    ScrollPageDown,
    ScrollPageLeft,
    ScrollPageRight,
    /// `Home`: ×1 scrolls a page left, ×2 goes to the top-left.
    HomeKey,
    /// `End`: ×1 scrolls a page right, ×2 goes to the bottom-right.
    EndKey,
    ScrollToTopLeft,
    ScrollToBottomRight,
    CenterTopLeft,
    CenterBottomRight,
    // Tools
    /// A tool letter; the tool is chosen from the letter, Shift and the tap count.
    ToolShortcut(char),
    OpenToolDropdown,
    /// `[` `]` ±1, `Ctrl+[` `Ctrl+]` ±5.
    BrushWidth(i8),
    SwapColors,
    SwitchActiveColor,
    /// Enter/Esc: commit the active tool's pending drawing; Enter also deselects.
    Finish,
    CycleShape {
        forward: bool,
    },
    LineStartCap,
    LineStyle,
    LineEndCap,
    // Diagnostics
    ToggleDiagnostics,
}

use Command as C;

impl Command {
    /// Shortcut text shown in menus and tooltips (first binding).
    pub fn shortcut_text(self) -> Option<String> {
        BINDINGS
            .iter()
            .find(|b| b.command == self)
            .map(|b| b.chord.text())
    }

    /// Whether the command does something yet. Unimplemented commands stay visible in
    /// menus (disabled), as paint.net shows its full menus.
    pub fn implemented(self) -> bool {
        matches!(
            self,
            C::NewImage
                | C::Open
                | C::Close
                | C::Save
                | C::SaveAs
                | C::SaveAll
                | C::Exit
                | C::ZoomIn
                | C::ZoomOut
                | C::ZoomToWindow
                | C::ActualSize
                | C::TogglePixelGrid
                | C::ToggleRulers
                | C::SetUnits(_)
                | C::ToggleWindow(_)
                | C::ResetWindow(_)
                | C::HelpTopics
                | C::About
                | C::OpenMenu(_)
                | C::NextImage
                | C::PreviousImage
                | C::MoveImageLeft
                | C::MoveImageRight
                | C::SwitchToImage(_)
                | C::ScrollPageUp
                | C::ScrollPageDown
                | C::ScrollPageLeft
                | C::ScrollPageRight
                | C::HomeKey
                | C::EndKey
                | C::ScrollToTopLeft
                | C::ScrollToBottomRight
                | C::CenterTopLeft
                | C::CenterBottomRight
                | C::ToolShortcut(_)
                | C::OpenToolDropdown
                | C::BrushWidth(_)
                | C::SwapColors
                | C::SwitchActiveColor
                | C::ToggleDiagnostics
                | C::Undo
                | C::Redo
                | C::AddNewLayer
                | C::DeleteLayer
                | C::DuplicateLayer
                | C::MergeLayerDown
                | C::ToggleLayerVisibility
                | C::ImportFromFile
                | C::FlipLayerHorizontal
                | C::FlipLayerVertical
                | C::RotateLayer180
                | C::RotateZoom
                | C::GoToTopLayer
                | C::GoToLayerAbove
                | C::GoToLayerBelow
                | C::GoToBottomLayer
                | C::MoveLayerToTop
                | C::MoveLayerUp
                | C::MoveLayerDown
                | C::MoveLayerToBottom
                | C::LayerProperties
                | C::FlipImageHorizontal
                | C::FlipImageVertical
                | C::RotateImage90Clockwise
                | C::RotateImage90CounterClockwise
                | C::RotateImage180
                | C::Resize
                | C::CanvasSize
                | C::Flatten
                | C::SelectAll
                | C::Deselect
                | C::InvertSelection
                | C::EraseSelection
                | C::FillSelection
                | C::FillSelectionSecondary
                | C::CropToSelection
                | C::ZoomToSelection
                | C::Finish
                | C::Cut
                | C::Copy
                | C::CopyMerged
                | C::Paste
                | C::PasteIntoNewLayer
                | C::PasteIntoNewImage
                | C::CopySelection
                | C::PasteSelection
        )
    }

    /// Commands that keep firing while their key auto-repeats.
    fn repeatable(self) -> bool {
        matches!(
            self,
            C::ZoomIn
                | C::ZoomOut
                | C::ScrollPageUp
                | C::ScrollPageDown
                | C::ScrollPageLeft
                | C::ScrollPageRight
                | C::NextImage
                | C::PreviousImage
                | C::BrushWidth(_)
                | C::Undo
                | C::Redo
                | C::GoToLayerAbove
                | C::GoToLayerBelow
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    /// `None` = Shift state ignored.
    pub shift: Option<bool>,
    pub key: Key,
}

impl Chord {
    pub fn matches(&self, key: Key, physical: Option<Key>, m: egui::Modifiers) -> bool {
        (key == self.key || physical == Some(self.key))
            && m.ctrl == self.ctrl
            && m.alt == self.alt
            && self.shift.is_none_or(|s| s == m.shift)
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift == Some(true) {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(key_text(self.key));
        s
    }
}

pub fn key_text(key: Key) -> &'static str {
    match key {
        Key::Plus => "+",
        Key::Minus => "-",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Slash => "/",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Backtick => "~",
        Key::PageUp => "PgUp",
        Key::PageDown => "PgDn",
        Key::Delete => "Del",
        Key::Insert => "Ins",
        Key::Backspace => "Backspace",
        Key::Escape => "Esc",
        k => k.symbol_or_name(),
    }
}

pub struct Binding {
    pub chord: Chord,
    pub command: Command,
}

const N: bool = false;
const Y: bool = true;

/// `(ctrl, alt, shift)` binding.
const fn b(ctrl: bool, alt: bool, shift: bool, key: Key, command: Command) -> Binding {
    Binding {
        chord: Chord {
            ctrl,
            alt,
            shift: Some(shift),
            key,
        },
        command,
    }
}

/// Binding that ignores Shift.
const fn any_shift(ctrl: bool, alt: bool, key: Key, command: Command) -> Binding {
    Binding {
        chord: Chord {
            ctrl,
            alt,
            shift: None,
            key,
        },
        command,
    }
}

/// The keymap. The first binding of a command is the one menus display.
pub static BINDINGS: &[Binding] = &[
    // File
    b(Y, N, N, Key::N, C::NewImage),
    b(Y, N, N, Key::O, C::Open),
    b(Y, N, N, Key::W, C::Close),
    b(Y, N, N, Key::F4, C::Close),
    b(Y, N, N, Key::S, C::Save),
    b(Y, N, Y, Key::S, C::SaveAs),
    b(Y, Y, N, Key::S, C::SaveAll),
    b(Y, N, N, Key::P, C::Print),
    // Edit
    b(Y, N, N, Key::Z, C::Undo),
    b(Y, N, N, Key::Y, C::Redo),
    b(Y, N, N, Key::X, C::Cut),
    b(N, N, Y, Key::Delete, C::Cut),
    b(Y, N, N, Key::C, C::Copy),
    b(Y, N, N, Key::Insert, C::Copy),
    b(Y, N, Y, Key::C, C::CopyMerged),
    b(Y, N, N, Key::V, C::Paste),
    b(N, N, Y, Key::Insert, C::Paste),
    b(Y, N, Y, Key::V, C::PasteIntoNewLayer),
    b(Y, Y, N, Key::V, C::PasteIntoNewImage),
    b(Y, Y, Y, Key::C, C::CopySelection),
    b(Y, Y, Y, Key::V, C::PasteSelection),
    b(N, N, N, Key::Delete, C::EraseSelection),
    b(N, N, N, Key::Backspace, C::FillSelection),
    b(N, N, Y, Key::Backspace, C::FillSelectionSecondary),
    b(Y, N, N, Key::I, C::InvertSelection),
    b(Y, N, N, Key::A, C::SelectAll),
    b(Y, N, N, Key::D, C::Deselect),
    b(N, N, Y, Key::F6, C::FeatherSelection),
    // View
    any_shift(Y, N, Key::Plus, C::ZoomIn),
    b(Y, N, N, Key::Equals, C::ZoomIn),
    b(Y, N, N, Key::Minus, C::ZoomOut),
    b(Y, N, N, Key::B, C::ZoomToWindow),
    b(Y, N, Y, Key::B, C::ZoomToSelection),
    b(Y, N, N, Key::Num0, C::ActualSize),
    // Image
    b(Y, N, Y, Key::X, C::CropToSelection),
    b(Y, N, N, Key::R, C::Resize),
    b(Y, N, Y, Key::R, C::CanvasSize),
    b(Y, N, N, Key::H, C::RotateImage90Clockwise),
    b(Y, N, N, Key::G, C::RotateImage90CounterClockwise),
    b(Y, N, Y, Key::F, C::Flatten),
    // Layers
    b(Y, N, Y, Key::N, C::AddNewLayer),
    b(Y, N, Y, Key::Delete, C::DeleteLayer),
    b(Y, N, Y, Key::D, C::DuplicateLayer),
    b(Y, N, N, Key::M, C::MergeLayerDown),
    b(Y, N, N, Key::Comma, C::ToggleLayerVisibility),
    b(Y, N, Y, Key::Z, C::RotateZoom),
    b(Y, Y, N, Key::PageUp, C::GoToTopLayer),
    b(N, Y, N, Key::PageUp, C::GoToLayerAbove),
    b(N, Y, N, Key::PageDown, C::GoToLayerBelow),
    b(Y, Y, N, Key::PageDown, C::GoToBottomLayer),
    b(N, N, N, Key::F4, C::LayerProperties),
    // Adjustments
    b(Y, N, Y, Key::L, C::Adjustment(Adjustment::AutoLevel)),
    b(Y, N, Y, Key::G, C::Adjustment(Adjustment::BlackAndWhite)),
    b(
        Y,
        N,
        Y,
        Key::T,
        C::Adjustment(Adjustment::BrightnessContrast),
    ),
    b(Y, N, Y, Key::M, C::Adjustment(Adjustment::Curves)),
    b(Y, N, Y, Key::U, C::Adjustment(Adjustment::HueSaturation)),
    b(Y, Y, N, Key::I, C::Adjustment(Adjustment::InvertAlpha)),
    b(Y, N, Y, Key::I, C::Adjustment(Adjustment::InvertColors)),
    b(Y, N, N, Key::L, C::Adjustment(Adjustment::Levels)),
    b(Y, N, Y, Key::P, C::Adjustment(Adjustment::Posterize)),
    b(Y, N, Y, Key::E, C::Adjustment(Adjustment::Sepia)),
    // Effects
    b(Y, N, N, Key::F, C::RepeatEffect),
    // Utility windows, settings, help
    b(N, N, N, Key::F5, C::ToggleWindow(Panel::Tools)),
    b(N, N, N, Key::F6, C::ToggleWindow(Panel::History)),
    b(N, N, N, Key::F7, C::ToggleWindow(Panel::Layers)),
    b(N, N, N, Key::F8, C::ToggleWindow(Panel::Colors)),
    b(Y, N, Y, Key::F5, C::ResetWindow(Panel::Tools)),
    b(Y, N, Y, Key::F6, C::ResetWindow(Panel::History)),
    b(Y, N, Y, Key::F7, C::ResetWindow(Panel::Layers)),
    b(Y, N, Y, Key::F8, C::ResetWindow(Panel::Colors)),
    b(N, Y, N, Key::X, C::Settings),
    b(N, N, N, Key::F1, C::HelpTopics),
    b(N, Y, N, Key::F, C::OpenMenu(MenuId::File)),
    b(N, Y, N, Key::E, C::OpenMenu(MenuId::Edit)),
    b(N, Y, N, Key::V, C::OpenMenu(MenuId::View)),
    b(N, Y, N, Key::I, C::OpenMenu(MenuId::Image)),
    b(N, Y, N, Key::L, C::OpenMenu(MenuId::Layers)),
    b(N, Y, N, Key::A, C::OpenMenu(MenuId::Adjustments)),
    b(N, Y, N, Key::C, C::OpenMenu(MenuId::Effects)),
    b(N, Y, N, Key::H, C::OpenMenu(MenuId::Help)),
    // Image list
    b(Y, N, N, Key::PageDown, C::NextImage),
    b(Y, N, N, Key::Tab, C::NextImage),
    b(Y, N, N, Key::PageUp, C::PreviousImage),
    b(Y, N, Y, Key::Tab, C::PreviousImage),
    b(Y, N, Y, Key::PageUp, C::MoveImageLeft),
    b(Y, N, Y, Key::PageDown, C::MoveImageRight),
    b(Y, N, N, Key::Num1, C::SwitchToImage(1)),
    b(Y, N, N, Key::Num2, C::SwitchToImage(2)),
    b(Y, N, N, Key::Num3, C::SwitchToImage(3)),
    b(Y, N, N, Key::Num4, C::SwitchToImage(4)),
    b(Y, N, N, Key::Num5, C::SwitchToImage(5)),
    b(Y, N, N, Key::Num6, C::SwitchToImage(6)),
    b(Y, N, N, Key::Num7, C::SwitchToImage(7)),
    b(Y, N, N, Key::Num8, C::SwitchToImage(8)),
    b(Y, N, N, Key::Num9, C::SwitchToImage(9)),
    b(N, Y, N, Key::Num1, C::SwitchToImage(1)),
    b(N, Y, N, Key::Num2, C::SwitchToImage(2)),
    b(N, Y, N, Key::Num3, C::SwitchToImage(3)),
    b(N, Y, N, Key::Num4, C::SwitchToImage(4)),
    b(N, Y, N, Key::Num5, C::SwitchToImage(5)),
    b(N, Y, N, Key::Num6, C::SwitchToImage(6)),
    b(N, Y, N, Key::Num7, C::SwitchToImage(7)),
    b(N, Y, N, Key::Num8, C::SwitchToImage(8)),
    b(N, Y, N, Key::Num9, C::SwitchToImage(9)),
    b(N, Y, N, Key::Minus, C::ImageContextMenu),
    // Scrolling
    b(N, N, N, Key::PageUp, C::ScrollPageUp),
    b(N, N, N, Key::PageDown, C::ScrollPageDown),
    b(N, N, Y, Key::PageUp, C::ScrollPageLeft),
    b(N, N, Y, Key::PageDown, C::ScrollPageRight),
    b(N, N, N, Key::Home, C::HomeKey),
    b(N, N, N, Key::End, C::EndKey),
    b(N, N, Y, Key::Home, C::ScrollToTopLeft),
    b(N, N, Y, Key::End, C::ScrollToBottomRight),
    b(Y, N, N, Key::Home, C::CenterTopLeft),
    b(Y, N, N, Key::End, C::CenterBottomRight),
    // Tools (letters with or without Shift; Shift walks shared groups backwards)
    any_shift(N, N, Key::S, C::ToolShortcut('S')),
    any_shift(N, N, Key::M, C::ToolShortcut('M')),
    any_shift(N, N, Key::O, C::ToolShortcut('O')),
    b(N, N, N, Key::Z, C::ToolShortcut('Z')),
    b(N, N, N, Key::H, C::ToolShortcut('H')),
    b(N, N, N, Key::F, C::ToolShortcut('F')),
    b(N, N, N, Key::G, C::ToolShortcut('G')),
    b(N, N, N, Key::B, C::ToolShortcut('B')),
    b(N, N, N, Key::E, C::ToolShortcut('E')),
    b(N, N, N, Key::P, C::ToolShortcut('P')),
    b(N, N, N, Key::K, C::ToolShortcut('K')),
    b(N, N, N, Key::L, C::ToolShortcut('L')),
    b(N, N, N, Key::R, C::ToolShortcut('R')),
    b(N, N, N, Key::T, C::ToolShortcut('T')),
    b(N, Y, N, Key::T, C::OpenToolDropdown),
    b(N, N, N, Key::OpenBracket, C::BrushWidth(-1)),
    b(N, N, N, Key::CloseBracket, C::BrushWidth(1)),
    b(Y, N, N, Key::OpenBracket, C::BrushWidth(-5)),
    b(Y, N, N, Key::CloseBracket, C::BrushWidth(5)),
    b(N, N, N, Key::X, C::SwapColors),
    b(N, N, N, Key::C, C::SwitchActiveColor),
    b(N, N, N, Key::Enter, C::Finish),
    b(N, N, N, Key::Escape, C::Finish),
    b(N, N, N, Key::A, C::CycleShape { forward: true }),
    b(N, N, Y, Key::A, C::CycleShape { forward: false }),
    b(N, N, N, Key::Comma, C::LineStartCap),
    b(N, N, N, Key::Period, C::LineStyle),
    b(N, N, N, Key::Slash, C::LineEndCap),
    // Diagnostics (paint.net's diagnostic aid chord)
    b(Y, Y, Y, Key::Backtick, C::ToggleDiagnostics),
];

/// Find the binding for a key event.
pub fn lookup(
    key: Key,
    physical: Option<Key>,
    modifiers: egui::Modifiers,
) -> Option<&'static Binding> {
    BINDINGS
        .iter()
        .find(|b| b.chord.matches(key, physical, modifiers))
}

/// A command triggered by a key press: how many times the chord was tapped in a row, and
/// whether Shift was held (tool letters use both).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triggered {
    pub command: Command,
    pub taps: u32,
    pub shift: bool,
}

impl Triggered {
    pub fn once(command: Command) -> Self {
        Self {
            command,
            taps: 1,
            shift: false,
        }
    }
}

/// paint.net's multi-tap rule: repeated presses of the same chord under 1 s apart count up.
#[derive(Default)]
pub struct MultiTap {
    last: Option<(Key, egui::Modifiers, Instant)>,
    count: u32,
}

const MULTI_TAP_WINDOW: Duration = Duration::from_secs(1);

impl MultiTap {
    pub fn press(&mut self, key: Key, modifiers: egui::Modifiers, now: Instant) -> u32 {
        self.count = match self.last {
            Some((k, m, t))
                if k == key && m == modifiers && now.duration_since(t) < MULTI_TAP_WINDOW =>
            {
                self.count + 1
            }
            _ => 1,
        };
        self.last = Some((key, modifiers, now));
        self.count
    }
}

/// Take key presses that match a binding out of this frame's events (so widgets don't also
/// act on them, e.g. Tab focus) and return the commands.
pub fn take_commands(ctx: &egui::Context, taps: &mut MultiTap, now: Instant) -> Vec<Triggered> {
    let mut out = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                repeat,
                modifiers,
            } = event
            else {
                return true;
            };
            let Some(binding) = lookup(*key, *physical_key, *modifiers) else {
                return true;
            };
            let count = if *repeat {
                1
            } else {
                taps.press(binding.chord.key, *modifiers, now)
            };
            if !*repeat || binding.command.repeatable() {
                out.push(Triggered {
                    command: binding.command,
                    taps: count,
                    shift: modifiers.shift,
                });
            }
            false
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, alt: bool, shift: bool) -> egui::Modifiers {
        egui::Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd: false,
            command: ctrl,
        }
    }

    fn find(key: Key, m: egui::Modifiers) -> Option<Command> {
        lookup(key, None, m).map(|b| b.command)
    }

    #[test]
    fn plus_matches_with_or_without_shift() {
        assert_eq!(find(Key::Plus, mods(Y, N, Y)), Some(C::ZoomIn));
        assert_eq!(find(Key::Plus, mods(Y, N, N)), Some(C::ZoomIn));
        assert_eq!(find(Key::Equals, mods(Y, N, N)), Some(C::ZoomIn));
    }

    #[test]
    fn no_chord_is_bound_twice() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in &BINDINGS[i + 1..] {
                let overlap = a.chord.key == b.chord.key
                    && a.chord.ctrl == b.chord.ctrl
                    && a.chord.alt == b.chord.alt
                    && (a.chord.shift.is_none()
                        || b.chord.shift.is_none()
                        || a.chord.shift == b.chord.shift);
                assert!(
                    !overlap,
                    "{} bound to both {:?} and {:?}",
                    a.chord.text(),
                    a.command,
                    b.command
                );
            }
        }
    }

    #[test]
    fn multi_tap_counts_same_chord_within_a_second() {
        let mut t = MultiTap::default();
        let t0 = Instant::now();
        let none = egui::Modifiers::NONE;
        assert_eq!(t.press(Key::S, none, t0), 1);
        assert_eq!(t.press(Key::S, none, t0 + Duration::from_millis(400)), 2);
        assert_eq!(
            t.press(
                Key::S,
                egui::Modifiers::SHIFT,
                t0 + Duration::from_millis(500)
            ),
            1,
            "Shift+S is a different chord"
        );
        assert_eq!(
            t.press(
                Key::S,
                egui::Modifiers::SHIFT,
                t0 + Duration::from_millis(1600)
            ),
            1,
            "too slow"
        );
    }

    #[test]
    fn shortcut_text() {
        assert_eq!(C::SaveAs.shortcut_text().as_deref(), Some("Ctrl+Shift+S"));
        assert_eq!(C::ZoomIn.shortcut_text().as_deref(), Some("Ctrl++"));
        assert_eq!(
            C::ToggleLayerVisibility.shortcut_text().as_deref(),
            Some("Ctrl+,")
        );
        assert_eq!(
            C::Adjustment(Adjustment::Levels).shortcut_text().as_deref(),
            Some("Ctrl+L")
        );
    }
}
