//! Settings (Alt+X) with shortcut rebinding, and the command palette (Ctrl+K).

#[path = "ui/common/mod.rs"]
mod common;

use common::*;
use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use pnn_core::ColorBgra;

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

fn type_text(h: &mut H<'_>, s: &str) {
    h.event(Event::Text(s.into()));
    h.step();
    h.step();
}

fn key(h: &mut H<'_>, k: Key, m: Modifiers) {
    for pressed in [true, false] {
        h.event(Event::Key {
            key: k,
            physical_key: Some(k),
            pressed,
            repeat: false,
            modifiers: m,
        });
        h.step();
    }
    h.step();
}

#[test]
fn rebinding_a_shortcut_in_settings() {
    let mut h = harness();
    press(&mut h, Modifiers::ALT, Key::X);
    h.get_by_label("General");
    h.get_by_label("Keyboard").click();
    h.step();
    h.step();
    // The search box has focus: filter to one command, then rebind it.
    type_text(&mut h, "Invert Colors");
    h.get_by_label("Change").click();
    h.step();
    h.step();
    h.get_by_label_contains("Press the new shortcut");
    key(&mut h, Key::J, ctrl());
    assert!(
        h.state()
            .config()
            .shortcuts
            .contains_key("Adjustment(InvertColors)"),
        "{:?}",
        h.state().config().shortcuts
    );
    // Close Settings (Esc isn't bound there; use the window's close button).
    h.get_by_label("Close window").click();
    h.step();
    h.step();
    // Ctrl+J now inverts; the old Ctrl+Shift+I does nothing.
    press(&mut h, ctrl(), Key::J);
    assert_eq!(doc(&h).layer(0).surface.get(5, 5), ColorBgra::BLACK);
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::I);
    assert_eq!(
        doc(&h).layer(0).surface.get(5, 5),
        ColorBgra::BLACK,
        "old chord unbound"
    );
    // Menus show the new shortcut.
    assert_eq!(
        paintnotnet::commands::Command::Adjustment(pnn_effects::Adjustment::InvertColors)
            .shortcut_text()
            .as_deref(),
        Some("Ctrl+J")
    );
    paintnotnet::commands::set_overrides(Vec::new());
}

#[test]
fn appearance_setting_switches_theme() {
    let mut h = harness();
    press(&mut h, Modifiers::ALT, Key::X);
    h.get_by_label("Light").click();
    h.step();
    h.step();
    assert_eq!(
        h.state().config().appearance,
        paintnotnet::config::Appearance::Light
    );
    assert!(!h.ctx.global_style().visuals.dark_mode, "light visuals");
}

#[test]
fn command_palette_finds_and_runs_a_command() {
    let mut h = harness();
    press(&mut h, ctrl(), Key::K);
    type_text(&mut h, "gauss");
    h.get_by_label_contains("Gaussian Blur");
    key(&mut h, Key::Enter, Modifiers::NONE);
    for _ in 0..3 {
        h.step();
    }
    assert!(
        h.state_mut().effect_mut().is_some(),
        "Gaussian Blur dialog opened from the palette"
    );
}
