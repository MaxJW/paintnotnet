//! Commands and their keyboard shortcuts.
//!
//! Bindings come from `docs/reference/keybindings.md` (paint.net 5.x). This is the seed of
//! Phase 2's full keymap engine. Lessons already learned:
//! - egui has no `Key` for shifted symbols (e.g. `~`, `+` via Shift+=), so a chord matches the
//!   logical key *or* the physical key, and a binding can ignore Shift.
//! - egui recomputes `repeat` itself, so auto-repeat is filtered via `repeat`.

use std::time::{Duration, Instant};

use eframe::egui::{self, Key};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    // File
    NewImage,
    Open,
    Close,
    Save,
    SaveAs,
    SaveAll,
    Exit,
    // Image list
    NextImage,
    PreviousImage,
    MoveImageLeft,
    MoveImageRight,
    /// 1-based, as in paint.net's Ctrl+N / Alt+N.
    SwitchToImage(u8),
    // View
    ZoomIn,
    ZoomOut,
    ZoomToWindow,
    ZoomToSelection,
    ActualSize,
    TogglePixelGrid,
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
    // Diagnostics
    ToggleDiagnostics,
}

impl Command {
    /// Shortcut text shown in menus (first binding).
    pub fn shortcut_text(self) -> Option<String> {
        BINDINGS
            .iter()
            .find(|b| b.command == self)
            .map(|b| b.chord.text())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    /// `None` = Shift state ignored (for keys that need Shift on some layouts, like `+`).
    pub shift: Option<bool>,
    pub key: Key,
}

impl Chord {
    const fn new(ctrl: bool, alt: bool, shift: bool, key: Key) -> Self {
        Self {
            ctrl,
            alt,
            shift: Some(shift),
            key,
        }
    }

    fn matches(&self, key: Key, physical: Option<Key>, m: egui::Modifiers) -> bool {
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
        s.push_str(match self.key {
            Key::Plus => "+",
            Key::Minus => "-",
            Key::PageUp => "PgUp",
            Key::PageDown => "PgDn",
            k => k.symbol_or_name(),
        });
        s
    }
}

pub struct Binding {
    pub chord: Chord,
    pub command: Command,
}

const fn bind(ctrl: bool, alt: bool, shift: bool, key: Key, command: Command) -> Binding {
    Binding {
        chord: Chord::new(ctrl, alt, shift, key),
        command,
    }
}

const fn bind_any_shift(ctrl: bool, key: Key, command: Command) -> Binding {
    Binding {
        chord: Chord {
            ctrl,
            alt: false,
            shift: None,
            key,
        },
        command,
    }
}

use Command as C;
const N: bool = false;
const Y: bool = true;

/// (ctrl, alt, shift, key) → command. First binding per command is the one shown in menus.
pub static BINDINGS: &[Binding] = &[
    bind(Y, N, N, Key::N, C::NewImage),
    bind(Y, N, N, Key::O, C::Open),
    bind(Y, N, N, Key::W, C::Close),
    bind(Y, N, N, Key::F4, C::Close),
    bind(Y, N, N, Key::S, C::Save),
    bind(Y, N, Y, Key::S, C::SaveAs),
    bind(Y, Y, N, Key::S, C::SaveAll),
    bind(Y, N, N, Key::PageDown, C::NextImage),
    bind(Y, N, N, Key::Tab, C::NextImage),
    bind(Y, N, N, Key::PageUp, C::PreviousImage),
    bind(Y, N, Y, Key::Tab, C::PreviousImage),
    bind(Y, N, Y, Key::PageUp, C::MoveImageLeft),
    bind(Y, N, Y, Key::PageDown, C::MoveImageRight),
    bind_any_shift(Y, Key::Plus, C::ZoomIn),
    bind(Y, N, N, Key::Equals, C::ZoomIn),
    bind(Y, N, N, Key::Minus, C::ZoomOut),
    bind(Y, N, N, Key::B, C::ZoomToWindow),
    bind(Y, N, Y, Key::B, C::ZoomToSelection),
    bind(Y, N, N, Key::Num0, C::ActualSize),
    bind(N, N, N, Key::PageUp, C::ScrollPageUp),
    bind(N, N, N, Key::PageDown, C::ScrollPageDown),
    bind(N, N, Y, Key::PageUp, C::ScrollPageLeft),
    bind(N, N, Y, Key::PageDown, C::ScrollPageRight),
    bind(N, N, N, Key::Home, C::HomeKey),
    bind(N, N, N, Key::End, C::EndKey),
    bind(N, N, Y, Key::Home, C::ScrollToTopLeft),
    bind(N, N, Y, Key::End, C::ScrollToBottomRight),
    bind(Y, N, N, Key::Home, C::CenterTopLeft),
    bind(Y, N, N, Key::End, C::CenterBottomRight),
    bind(Y, Y, Y, Key::Backtick, C::ToggleDiagnostics),
    // Ctrl+1..9 and Alt+1..9: switch to image N.
    bind(Y, N, N, Key::Num1, C::SwitchToImage(1)),
    bind(Y, N, N, Key::Num2, C::SwitchToImage(2)),
    bind(Y, N, N, Key::Num3, C::SwitchToImage(3)),
    bind(Y, N, N, Key::Num4, C::SwitchToImage(4)),
    bind(Y, N, N, Key::Num5, C::SwitchToImage(5)),
    bind(Y, N, N, Key::Num6, C::SwitchToImage(6)),
    bind(Y, N, N, Key::Num7, C::SwitchToImage(7)),
    bind(Y, N, N, Key::Num8, C::SwitchToImage(8)),
    bind(Y, N, N, Key::Num9, C::SwitchToImage(9)),
    bind(N, Y, N, Key::Num1, C::SwitchToImage(1)),
    bind(N, Y, N, Key::Num2, C::SwitchToImage(2)),
    bind(N, Y, N, Key::Num3, C::SwitchToImage(3)),
    bind(N, Y, N, Key::Num4, C::SwitchToImage(4)),
    bind(N, Y, N, Key::Num5, C::SwitchToImage(5)),
    bind(N, Y, N, Key::Num6, C::SwitchToImage(6)),
    bind(N, Y, N, Key::Num7, C::SwitchToImage(7)),
    bind(N, Y, N, Key::Num8, C::SwitchToImage(8)),
    bind(N, Y, N, Key::Num9, C::SwitchToImage(9)),
];

/// A command triggered by a key press, with how many times that key was tapped in a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triggered {
    pub command: Command,
    pub taps: u32,
}

/// paint.net's multi-tap rule: repeated presses of the same chord under 1 s apart count up.
#[derive(Default)]
pub struct MultiTap {
    last: Option<(Chord, Instant)>,
    count: u32,
}

const MULTI_TAP_WINDOW: Duration = Duration::from_secs(1);

impl MultiTap {
    pub fn press(&mut self, chord: Chord, now: Instant) -> u32 {
        self.count = match self.last {
            Some((c, t)) if c == chord && now.duration_since(t) < MULTI_TAP_WINDOW => {
                self.count + 1
            }
            _ => 1,
        };
        self.last = Some((chord, now));
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
            let Some(binding) = BINDINGS
                .iter()
                .find(|b| b.chord.matches(*key, *physical_key, *modifiers))
            else {
                return true;
            };
            // Auto-repeat re-triggers scrolling/zooming but never counts as a new tap.
            let taps = if *repeat {
                1
            } else {
                taps.press(binding.chord, now)
            };
            if !*repeat || is_repeatable(binding.command) {
                out.push(Triggered {
                    command: binding.command,
                    taps,
                });
            }
            false
        });
    });
    out
}

fn is_repeatable(c: Command) -> bool {
    matches!(
        c,
        C::ZoomIn
            | C::ZoomOut
            | C::ScrollPageUp
            | C::ScrollPageDown
            | C::ScrollPageLeft
            | C::ScrollPageRight
            | C::NextImage
            | C::PreviousImage
    )
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

    fn lookup(key: Key, physical: Option<Key>, m: egui::Modifiers) -> Option<Command> {
        BINDINGS
            .iter()
            .find(|b| b.chord.matches(key, physical, m))
            .map(|b| b.command)
    }

    #[test]
    fn plus_matches_with_or_without_shift() {
        assert_eq!(lookup(Key::Plus, None, mods(Y, N, Y)), Some(C::ZoomIn));
        assert_eq!(lookup(Key::Plus, None, mods(Y, N, N)), Some(C::ZoomIn));
        assert_eq!(lookup(Key::Equals, None, mods(Y, N, N)), Some(C::ZoomIn));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        assert_eq!(lookup(Key::S, None, mods(Y, N, N)), Some(C::Save));
        assert_eq!(lookup(Key::S, None, mods(Y, N, Y)), Some(C::SaveAs));
        assert_eq!(lookup(Key::S, None, mods(Y, Y, N)), Some(C::SaveAll));
        assert_eq!(lookup(Key::S, None, mods(N, N, N)), None);
        assert_eq!(
            lookup(Key::PageDown, None, mods(Y, N, Y)),
            Some(C::MoveImageRight)
        );
    }

    #[test]
    fn physical_key_fallback() {
        // Shift+` reports no usable logical key on a US layout.
        assert_eq!(
            lookup(Key::Space, Some(Key::Backtick), mods(Y, Y, Y)),
            Some(C::ToggleDiagnostics)
        );
    }

    #[test]
    fn multi_tap_counts_within_a_second() {
        let mut t = MultiTap::default();
        let home = BINDINGS
            .iter()
            .find(|b| b.command == C::HomeKey)
            .unwrap()
            .chord;
        let end = BINDINGS
            .iter()
            .find(|b| b.command == C::EndKey)
            .unwrap()
            .chord;
        let t0 = Instant::now();
        assert_eq!(t.press(home, t0), 1);
        assert_eq!(t.press(home, t0 + Duration::from_millis(400)), 2);
        assert_eq!(
            t.press(home, t0 + Duration::from_millis(1500)),
            1,
            "too slow"
        );
        assert_eq!(
            t.press(end, t0 + Duration::from_millis(1600)),
            1,
            "different key"
        );
    }

    #[test]
    fn shortcut_text() {
        assert_eq!(C::SaveAs.shortcut_text().as_deref(), Some("Ctrl+Shift+S"));
        assert_eq!(C::ZoomIn.shortcut_text().as_deref(), Some("Ctrl++"));
        assert_eq!(
            C::SwitchToImage(3).shortcut_text().as_deref(),
            Some("Ctrl+3")
        );
    }
}
