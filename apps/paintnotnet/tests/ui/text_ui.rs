//! Phase 7c: the Text tool, end to end through real events.

use crate::common::*;
use eframe::egui::{Event, Key, Modifiers, PointerButton};
use pnn_core::ColorBgra;
use pnn_tools::ToolKind;

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

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
}

/// Type as a keyboard would: a key press, the text, a release.
fn type_text(h: &mut H<'_>, s: &str) {
    for c in s.chars() {
        let k = Key::from_name(&c.to_ascii_uppercase().to_string());
        if let Some(k) = k {
            h.event(Event::Key {
                key: k,
                physical_key: Some(k),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
        h.event(Event::Text(c.to_string()));
        h.step();
        if let Some(k) = k {
            h.event(Event::Key {
                key: k,
                physical_key: Some(k),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
    }
    h.step();
}

fn click(h: &mut H<'_>, x: f64, y: f64) {
    drag(
        h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(x, y), (x, y)],
    );
}

fn start(h: &mut H<'_>) {
    press(h, Modifiers::NONE, Key::T);
    assert_eq!(h.state().tool(), ToolKind::Text);
    let s = h.state_mut().settings_mut();
    s.font_family = "DejaVu Sans".into();
    s.font_size = 15.0; // 20 px at 96 DPI
}

/// Dark pixels inside [x0, x1) × [y0, y1).
fn ink(h: &H<'_>, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
    let s = &doc(h).layer(0).surface;
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| s.get(x, y).r < 128)
        .count()
}

#[test]
fn typing_draws_live_text_and_esc_commits() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    click(&mut h, 10.0, 10.0);
    type_text(&mut h, "Hie");
    assert_eq!(
        h.state().tool(),
        ToolKind::Text,
        "'e' typed, not the Eraser shortcut"
    );
    assert!(ink(&h, 10, 10, 50, 35) > 30, "text drawn");
    assert_eq!(ink(&h, 0, 40, 100, 80), 0, "nothing below");
    assert_eq!(history_names(&h).len(), n, "live");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(history_names(&h).last().unwrap(), "Text");
    assert!(ink(&h, 10, 10, 50, 35) > 30, "kept");
}

#[test]
fn enter_backspace_and_restyling() {
    let mut h = harness();
    start(&mut h);
    click(&mut h, 10.0, 5.0);
    type_text(&mut h, "Ab");
    key(&mut h, Key::Enter, Modifiers::NONE);
    type_text(&mut h, "Cd");
    assert!(ink(&h, 10, 30, 50, 55) > 20, "second line");
    let two = ink(&h, 0, 0, 100, 80);
    key(&mut h, Key::Backspace, Modifiers::NONE);
    key(&mut h, Key::Backspace, Modifiers::NONE);
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(
        ink(&h, 10, 30, 50, 55),
        0,
        "second line and its newline deleted"
    );
    let plain = ink(&h, 0, 0, 100, 80);
    assert!(plain < two);
    h.state_mut().settings_mut().bold = true;
    h.step();
    assert!(
        ink(&h, 0, 0, 100, 80) > plain,
        "bold restyles the live text"
    );
}

#[test]
fn undo_drops_live_text_and_clicking_elsewhere_commits() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    click(&mut h, 10.0, 10.0);
    type_text(&mut h, "ab");
    key(&mut h, Key::Z, ctrl());
    assert_eq!(ink(&h, 0, 0, 100, 80), 0, "Ctrl+Z took back the live text");
    assert_eq!(history_names(&h).len(), n);
    click(&mut h, 10.0, 10.0);
    type_text(&mut h, "ab");
    click(&mut h, 50.0, 50.0);
    assert_eq!(
        history_names(&h).len(),
        n + 1,
        "clicking elsewhere committed it"
    );
    type_text(&mut h, "cd");
    assert!(ink(&h, 50, 50, 90, 75) > 20, "new text at the new click");
}

#[test]
fn move_nub_drags_the_text() {
    let mut h = harness();
    start(&mut h);
    click(&mut h, 10.0, 10.0);
    type_text(&mut h, "ab");
    let nub = h.state().canvas().text_handles.expect("caret shown").2;
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[
            (nub[0], nub[1]),
            (nub[0] + 20.0, nub[1] + 15.0),
            (nub[0] + 40.0, nub[1] + 30.0),
        ],
    );
    assert_eq!(ink(&h, 0, 0, 45, 38), 0, "moved away");
    assert!(ink(&h, 50, 40, 90, 70) > 20, "now 40 px right, 30 down");
    assert_eq!(doc(&h).layer(0).surface.get(5, 5), ColorBgra::WHITE);
}
