//! Keymap conformance: every keyboard chord in paint.net 5's official shortcut table
//! (`docs/reference/keybindings.md`) must resolve to the documented command.
//!
//! The test parses the reference tables, so the docs can't drift from the keymap unnoticed:
//! - any keyboard chord in the docs that is missing from `EXPECTED` fails the test;
//! - any `EXPECTED` entry that no longer appears in the docs fails the test;
//! - every expected chord must resolve through the real keymap to the stated command.
//!
//! Mouse gestures are skipped automatically; held-key and contextual chords are listed
//! explicitly as `Skip` with the reason.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{Key, Modifiers};
use paintnotnet::commands::{Command as C, MenuId, Panel, lookup};
use pnn_effects::Adjustment as A;

enum Expect {
    Is(C),
    /// The reason is documentation for readers of this table.
    Skip(#[allow(dead_code)] &'static str),
}
use Expect::*;

/// Doc chord text (normalised, without ×N tap counts) → expected command.
fn expected() -> Vec<(&'static str, Expect)> {
    vec![
        ("PgUp", Is(C::ScrollPageUp)),
        ("PgDn", Is(C::ScrollPageDown)),
        ("Shift+PgUp", Is(C::ScrollPageLeft)),
        ("Shift+PgDn", Is(C::ScrollPageRight)),
        ("Home", Is(C::HomeKey)),
        ("End", Is(C::EndKey)),
        ("Shift+Home", Is(C::ScrollToTopLeft)),
        ("Shift+End", Is(C::ScrollToBottomRight)),
        ("Ctrl+Home", Is(C::CenterTopLeft)),
        ("Ctrl+End", Is(C::CenterBottomRight)),
        ("Ctrl+-", Is(C::ZoomOut)),
        ("Ctrl++", Is(C::ZoomIn)),
        ("Ctrl+0", Is(C::ActualSize)),
        ("Ctrl+B", Is(C::ZoomToWindow)),
        ("Ctrl+Shift+B", Is(C::ZoomToSelection)),
        (
            "Space+Arrows",
            Skip("held Space, handled by keyboard panning"),
        ),
        (
            "Space+Ctrl+Arrows",
            Skip("held Space, handled by keyboard panning"),
        ),
        ("Ctrl+D", Is(C::Deselect)),
        ("Enter", Is(C::Finish)),
        ("Esc", Is(C::Finish)),
        (
            "Arrows",
            Skip("tool nudge, routed to the active tool (Phase 4)"),
        ),
        (
            "Ctrl+Arrows",
            Skip("tool nudge, routed to the active tool (Phase 4)"),
        ),
        ("Alt", Skip("menu hotkey cues, handled by the menu bar")),
        ("F5", Is(C::ToggleWindow(Panel::Tools))),
        ("F6", Is(C::ToggleWindow(Panel::History))),
        ("F7", Is(C::ToggleWindow(Panel::Layers))),
        ("F8", Is(C::ToggleWindow(Panel::Colors))),
        ("Ctrl+Shift+F5", Is(C::ResetWindow(Panel::Tools))),
        ("Ctrl+Shift+F6", Is(C::ResetWindow(Panel::History))),
        ("Ctrl+Shift+F7", Is(C::ResetWindow(Panel::Layers))),
        ("Ctrl+Shift+F8", Is(C::ResetWindow(Panel::Colors))),
        ("Ctrl+PgDn", Is(C::NextImage)),
        ("Ctrl+Tab", Is(C::NextImage)),
        ("Ctrl+PgUp", Is(C::PreviousImage)),
        ("Ctrl+Shift+Tab", Is(C::PreviousImage)),
        ("Ctrl+Shift+PgUp", Is(C::MoveImageLeft)),
        ("Ctrl+Shift+PgDn", Is(C::MoveImageRight)),
        ("Ctrl+*N*", Skip("expanded to Ctrl+1..9 below")),
        ("Alt+*N*", Skip("expanded to Alt+1..9 below")),
        ("Alt+-", Is(C::ImageContextMenu)),
        ("Alt+T", Is(C::OpenToolDropdown)),
        ("[", Is(C::BrushWidth(-1))),
        ("]", Is(C::BrushWidth(1))),
        ("Ctrl+[", Is(C::BrushWidth(-5))),
        ("Ctrl+]", Is(C::BrushWidth(5))),
        ("Alt+F", Is(C::OpenMenu(MenuId::File))),
        ("Ctrl+N", Is(C::NewImage)),
        ("Ctrl+O", Is(C::Open)),
        ("R", Is(C::ToolShortcut('R'))),
        ("Q", Skip("Alt+F, Q: menu item mnemonic (Acquire)")),
        ("Ctrl+W", Is(C::Close)),
        ("Ctrl+F4", Is(C::Close)),
        ("Ctrl+S", Is(C::Save)),
        ("Ctrl+Shift+S", Is(C::SaveAs)),
        ("Ctrl+Alt+S", Is(C::SaveAll)),
        ("Ctrl+P", Is(C::Print)),
        (
            "Alt+F4",
            Skip("window manager close; reaches the app as a close request"),
        ),
        ("X", Is(C::SwapColors)),
        ("Alt+E", Is(C::OpenMenu(MenuId::Edit))),
        ("Ctrl+Z", Is(C::Undo)),
        ("Ctrl+Y", Is(C::Redo)),
        ("Ctrl+X", Is(C::Cut)),
        ("Shift+Delete", Is(C::Cut)),
        ("Ctrl+C", Is(C::Copy)),
        ("Ctrl+Insert", Is(C::Copy)),
        ("Ctrl+Shift+C", Is(C::CopyMerged)),
        ("Ctrl+V", Is(C::Paste)),
        ("Shift+Insert", Is(C::Paste)),
        ("Ctrl+Shift+V", Is(C::PasteIntoNewLayer)),
        ("Ctrl+Alt+V", Is(C::PasteIntoNewImage)),
        ("Ctrl+Alt+Shift+C", Is(C::CopySelection)),
        ("Ctrl+Alt+Shift+V", Is(C::PasteSelection)),
        ("Delete", Is(C::EraseSelection)),
        ("Backspace", Is(C::FillSelection)),
        ("Shift+Backspace", Is(C::FillSelectionSecondary)),
        ("Ctrl+I", Is(C::InvertSelection)),
        ("Ctrl+A", Is(C::SelectAll)),
        ("Alt+V", Is(C::OpenMenu(MenuId::View))),
        ("Alt+I", Is(C::OpenMenu(MenuId::Image))),
        ("Ctrl+Shift+X", Is(C::CropToSelection)),
        ("Ctrl+R", Is(C::Resize)),
        ("Ctrl+Shift+R", Is(C::CanvasSize)),
        ("Ctrl+H", Is(C::RotateImage90Clockwise)),
        ("Ctrl+G", Is(C::RotateImage90CounterClockwise)),
        ("Ctrl+Shift+F", Is(C::Flatten)),
        ("Alt+L", Is(C::OpenMenu(MenuId::Layers))),
        ("Ctrl+Shift+N", Is(C::AddNewLayer)),
        ("Ctrl+Shift+Delete", Is(C::DeleteLayer)),
        ("Ctrl+Shift+D", Is(C::DuplicateLayer)),
        ("Ctrl+M", Is(C::MergeLayerDown)),
        ("Ctrl+,", Is(C::ToggleLayerVisibility)),
        ("Ctrl+Shift+Z", Is(C::RotateZoom)),
        ("Ctrl+Alt+PgUp", Is(C::GoToTopLayer)),
        ("Alt+PgUp", Is(C::GoToLayerAbove)),
        ("Alt+PgDn", Is(C::GoToLayerBelow)),
        ("Ctrl+Alt+PgDn", Is(C::GoToBottomLayer)),
        ("F4", Is(C::LayerProperties)),
        ("Alt+A", Is(C::OpenMenu(MenuId::Adjustments))),
        ("Ctrl+Shift+L", Is(C::Adjustment(A::AutoLevel))),
        ("Ctrl+Shift+G", Is(C::Adjustment(A::BlackAndWhite))),
        ("Ctrl+Shift+T", Is(C::Adjustment(A::BrightnessContrast))),
        ("Ctrl+Shift+M", Is(C::Adjustment(A::Curves))),
        ("Ctrl+Shift+U", Is(C::Adjustment(A::HueSaturation))),
        ("Ctrl+Alt+I", Is(C::Adjustment(A::InvertAlpha))),
        ("Ctrl+Shift+I", Is(C::Adjustment(A::InvertColors))),
        ("Ctrl+L", Is(C::Adjustment(A::Levels))),
        ("Ctrl+Shift+P", Is(C::Adjustment(A::Posterize))),
        ("Ctrl+Shift+E", Is(C::Adjustment(A::Sepia))),
        ("Alt+C", Is(C::OpenMenu(MenuId::Effects))),
        ("Ctrl+F", Is(C::RepeatEffect)),
        ("Alt+X", Is(C::Settings)),
        ("Alt+H", Is(C::OpenMenu(MenuId::Help))),
        ("F1", Is(C::HelpTopics)),
        ("C", Is(C::SwitchActiveColor)),
        ("S", Is(C::ToolShortcut('S'))),
        ("Shift+S", Is(C::ToolShortcut('S'))),
        ("F", Is(C::ToolShortcut('F'))),
        ("B", Is(C::ToolShortcut('B'))),
        ("P", Is(C::ToolShortcut('P'))),
        ("L", Is(C::ToolShortcut('L'))),
        ("T", Is(C::ToolShortcut('T'))),
        ("O", Is(C::ToolShortcut('O'))),
        ("Shift+O", Is(C::ToolShortcut('O'))),
        ("M", Is(C::ToolShortcut('M'))),
        ("Shift+M", Is(C::ToolShortcut('M'))),
        ("Z", Is(C::ToolShortcut('Z'))),
        ("H", Is(C::ToolShortcut('H'))),
        ("G", Is(C::ToolShortcut('G'))),
        ("E", Is(C::ToolShortcut('E'))),
        ("K", Is(C::ToolShortcut('K'))),
        (",", Is(C::LineStartCap)),
        (".", Is(C::LineStyle)),
        ("/", Is(C::LineEndCap)),
        ("A", Is(C::CycleShape { forward: true })),
        ("Shift+A", Is(C::CycleShape { forward: false })),
        ("Ctrl+Alt+Shift+~", Is(C::ToggleDiagnostics)),
        ("Shift+F6", Is(C::FeatherSelection)),
        (
            "Ctrl+K",
            Skip("command palette (paintnotnet addition, Phase 11)"),
        ),
        (
            "Shift",
            Skip("held modifier inside a tool (fill mode toggle), Phase 6"),
        ),
    ]
}

/// Words that mark an alternative as a mouse gesture, not a keyboard chord.
const MOUSE_WORDS: &[&str] = &[
    "LMB",
    "RMB",
    "MMB",
    "drag",
    "Drag",
    "Wheel",
    "click",
    "Click",
    "icon",
    "nub",
    "mouse",
    "slider",
    "box",
    "underlined",
    "Either button",
    "Finish",
    "View menu",
    "while",
    "Hold ",
    "selector",
    "Status Bar",
    "wheel",
    "above",
    "Move Selected Pixels",
    "hold button",
];

/// Keyboard chords from the doc's tables, normalised (tap counts and notes removed).
fn doc_chords() -> BTreeSet<String> {
    let md = include_str!("../../../docs/reference/keybindings.md");
    let mut out = BTreeSet::new();
    for line in md.lines() {
        let Some(row) = line.strip_prefix('|') else {
            continue;
        };
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let keys = cells[0];
        if keys.is_empty()
            || keys.starts_with("---")
            || keys.starts_with("Keys")
            || keys.starts_with("Mouse")
        {
            continue;
        }
        // "A / B" pairs, "PgUp/PgDn", and comma or semicolon alternatives are separate chords.
        // In "Ctrl+Shift+F5 / F6 / F7 / F8" the modifiers carry over to the bare keys.
        let keys = keys.replace("PgUp/PgDn", "PgUp / PgDn");
        let mut parts: Vec<String> = Vec::new();
        for group in keys.replace("; ", ", ").replace(" or ", ", ").split(", ") {
            let pair: Vec<&str> = group.split(" / ").collect();
            let prefix = match pair[0].rfind('+') {
                Some(i) if pair[0].len() > 1 && !pair[0].ends_with("++") => &pair[0][..=i],
                _ => "",
            };
            for (i, p) in pair.iter().enumerate() {
                let bare = p.split(" (").next().unwrap_or(p);
                let inherit = i > 0 && !bare.contains('+') && !prefix.is_empty();
                parts.push(if inherit {
                    format!("{prefix}{p}")
                } else {
                    p.to_string()
                });
            }
        }
        for alt in parts {
            let mut alt = alt.trim().to_owned();
            // "(History) Ctrl+Z": context prefix.
            if alt.starts_with('(')
                && let Some(i) = alt.find(") ")
            {
                alt = alt[i + 2..].to_owned();
            }
            if let Some(i) = alt.find(" (") {
                alt.truncate(i);
            }
            if MOUSE_WORDS.iter().any(|w| alt.contains(w)) {
                continue;
            }
            if let Some(i) = alt.find(" ×") {
                alt.truncate(i);
            }
            // "Alt+F, R" sequence leaves "Alt+F then X" etc.
            if let Some(i) = alt.find(" then ") {
                alt.truncate(i);
            }
            if !alt.is_empty() {
                out.insert(alt);
            }
        }
    }
    out
}

fn parse(chord: &str) -> (Modifiers, Key) {
    let (mods_part, key_part) = if let Some(rest) = chord.strip_suffix("++") {
        (format!("{rest}+"), "+".to_owned())
    } else {
        match chord.rfind('+') {
            Some(i) => (chord[..=i].to_owned(), chord[i + 1..].to_owned()),
            None => (String::new(), chord.to_owned()),
        }
    };
    let ctrl = mods_part.contains("Ctrl+");
    let m = Modifiers {
        alt: mods_part.contains("Alt+"),
        ctrl,
        shift: mods_part.contains("Shift+"),
        mac_cmd: false,
        command: ctrl,
    };
    let key = match key_part.as_str() {
        "+" => Key::Plus,
        "-" => Key::Minus,
        "," => Key::Comma,
        "." => Key::Period,
        "/" => Key::Slash,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        "~" => Key::Backtick,
        "PgUp" => Key::PageUp,
        "PgDn" => Key::PageDown,
        "Esc" => Key::Escape,
        other => {
            Key::from_name(other).unwrap_or_else(|| panic!("unknown key {other:?} in {chord:?}"))
        }
    };
    (m, key)
}

#[test]
fn every_documented_chord_is_accounted_for() {
    let expected: BTreeMap<&str, Expect> = expected().into_iter().collect();
    let doc = doc_chords();
    let missing: Vec<&String> = doc
        .iter()
        .filter(|c| !expected.contains_key(c.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "documented chords with no expectation: {missing:?}"
    );
    let stale: Vec<&&str> = expected.keys().filter(|k| !doc.contains(**k)).collect();
    assert!(stale.is_empty(), "expectations not in the docs: {stale:?}");
}

#[test]
fn documented_chords_resolve_to_documented_commands() {
    let mut failures = Vec::new();
    for (chord, expect) in expected() {
        let Is(command) = expect else { continue };
        let (m, key) = parse(chord);
        match lookup(key, None, m) {
            Some(b) if b.command == command => {}
            other => failures.push(format!(
                "{chord}: expected {command:?}, got {:?}",
                other.map(|b| b.command)
            )),
        }
    }
    for n in 1..=9u8 {
        for m in [Modifiers::CTRL | Modifiers::COMMAND, Modifiers::ALT] {
            let key = Key::from_name(&n.to_string()).unwrap();
            if lookup(key, None, m).map(|b| b.command) != Some(C::SwitchToImage(n)) {
                failures.push(format!("{m:?}+{n}: expected SwitchToImage({n})"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "keymap mismatches:\n{}",
        failures.join("\n")
    );
}

/// Not a pass/fail check: prints which documented commands are still unimplemented, so the
/// remaining gap is visible in test output (`cargo test -- --nocapture`).
#[test]
fn report_unimplemented() {
    let mut todo: Vec<String> = expected()
        .into_iter()
        .filter_map(|(chord, e)| match e {
            Is(c) if !c.implemented() => Some(format!("{chord} → {c:?}")),
            _ => None,
        })
        .collect();
    todo.sort();
    println!(
        "{} documented shortcuts not yet implemented:\n  {}",
        todo.len(),
        todo.join("\n  ")
    );
}
