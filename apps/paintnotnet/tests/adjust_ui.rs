//! Phase 8a: the effect framework through the Adjustments menu shortcuts.

mod common;

use common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use pnn_core::{ColorBgra, Document, Layer, Surface};
use pnn_effects::framework::Value;

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

fn ctrl_shift() -> Modifiers {
    ctrl() | Modifiers::SHIFT
}

/// 100×80 horizontal grey ramp.
fn ramp() -> Document {
    let mut s = Surface::new_transparent(100, 80);
    for y in 0..80 {
        for x in 0..100 {
            let v = (x * 255 / 99) as u8;
            s.set(x, y, ColorBgra::from_rgba(v, v, v, 255));
        }
    }
    Document::from_layers(100, 80, vec![Layer::new("Background", s)])
}

fn settle(h: &mut H<'_>) {
    for _ in 0..6 {
        h.step();
    }
}

#[test]
fn invert_colors_applies_at_once_and_ctrl_f_repeats() {
    let mut h = harness();
    let n = history_names(&h).len();
    press(&mut h, ctrl_shift(), Key::I);
    assert_eq!(doc(&h).layer(0).surface.get(50, 40), ColorBgra::BLACK);
    assert_eq!(history_names(&h).last().unwrap(), "Invert Colors");
    press(&mut h, ctrl(), Key::F);
    assert_eq!(doc(&h).layer(0).surface.get(50, 40), ColorBgra::WHITE);
    assert_eq!(history_names(&h).len(), n + 2);
}

#[test]
fn adjustments_respect_the_selection() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (40.0, 40.0)],
    );
    press(&mut h, ctrl_shift(), Key::I);
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(20, 20), ColorBgra::BLACK);
    assert_eq!(s.get(50, 50), ColorBgra::WHITE);
    assert_eq!(s.get(9, 20), ColorBgra::WHITE);
}

#[test]
fn posterize_previews_live_then_ok_commits() {
    let mut h = harness_with(ramp());
    let n = history_names(&h).len();
    press(&mut h, ctrl_shift(), Key::P);
    assert!(h.state_mut().effect_mut().is_some(), "dialog open");
    settle(&mut h);
    // 16 levels: x = 50 (v = 128) maps to 136 (3.36 table).
    assert_eq!(doc(&h).layer(0).surface.get(50, 40).r, 136, "live preview");
    assert_eq!(history_names(&h).len(), n, "not committed yet");
    // Change values without a preview run: OK renders them synchronously.
    {
        let e = h.state_mut().effect_mut().unwrap();
        e.values = vec![
            Value::Int(2),
            Value::Int(2),
            Value::Int(2),
            Value::Bool(true),
        ];
    }
    h.get_by_label("OK").click();
    settle(&mut h);
    assert!(h.state_mut().effect_mut().is_none());
    let s = &doc(&h).layer(0).surface;
    assert_eq!((s.get(10, 40).r, s.get(90, 40).r), (0, 255), "two levels");
    assert_eq!(history_names(&h).last().unwrap(), "Posterize");
    assert_eq!(history_names(&h).len(), n + 1);
}

#[test]
fn cancel_restores_the_layer() {
    let mut h = harness_with(ramp());
    let n = history_names(&h).len();
    let original = doc(&h).layer(0).surface.get(50, 40);
    press(&mut h, ctrl_shift(), Key::P);
    settle(&mut h);
    assert_ne!(doc(&h).layer(0).surface.get(50, 40), original, "previewing");
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert_eq!(doc(&h).layer(0).surface.get(50, 40), original);
    assert_eq!(history_names(&h).len(), n);
    // Esc cancels too, and Enter is OK.
    press(&mut h, ctrl_shift(), Key::P);
    settle(&mut h);
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(doc(&h).layer(0).surface.get(50, 40), original);
    press(&mut h, ctrl_shift(), Key::P);
    settle(&mut h);
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(history_names(&h).last().unwrap(), "Posterize");
}

#[test]
fn hue_saturation_dialog_and_auto_level() {
    let mut h = harness_with(ramp());
    press(&mut h, ctrl_shift(), Key::U);
    {
        let e = h.state_mut().effect_mut().expect("Hue / Saturation dialog");
        e.values = vec![Value::Int(0), Value::Int(100), Value::Int(-100)];
    }
    h.get_by_label("OK").click();
    settle(&mut h);
    assert_eq!(history_names(&h).last().unwrap(), "Hue / Saturation");
    assert!(
        doc(&h).layer(0).surface.get(99, 40).r < 5,
        "lightness −100 → black"
    );
    // Auto-Level on a black image changes nothing visible but records a step.
    press(&mut h, ctrl_shift(), Key::L);
    assert_eq!(history_names(&h).last().unwrap(), "Auto-Level");
}
