//! Headless UI test: the paint.net diagnostic chord (Ctrl+Alt+Shift+`) toggles the overlay.

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use paintnotnet::app::PaintNotNetApp;

fn chord() -> Modifiers {
    Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::COMMAND
}

fn backtick(pressed: bool, modifiers: Modifiers) -> Event {
    // egui recomputes `repeat` from its own key-down state, so it is irrelevant here.
    Event::Key {
        key: Key::Backtick,
        physical_key: Some(Key::Backtick),
        pressed,
        repeat: false,
        modifiers,
    }
}

/// Holds `modifiers`, sends `presses` key-down events for ` (more than one = auto-repeat),
/// then releases everything.
fn hold(harness: &mut Harness<'_, PaintNotNetApp>, modifiers: Modifiers, presses: usize) {
    let mut events = vec![Event::ModifiersChanged(modifiers)];
    events.extend((0..presses).map(|_| backtick(true, modifiers)));
    events.push(backtick(false, modifiers));
    events.push(Event::ModifiersChanged(Modifiers::NONE));

    let frames = events.len();
    for event in events {
        harness.event(event);
    }
    // Kittest feeds one queued event per frame. The overlay requests continuous repaints,
    // so step frames explicitly instead of `run()`.
    for _ in 0..frames {
        harness.step();
    }
}

fn harness() -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder().build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.step();
    h
}

#[test]
fn chord_toggles_overlay() {
    let mut h = harness();
    let initially = h.state().diagnostics().visible();
    hold(&mut h, chord(), 1);
    assert_eq!(h.state().diagnostics().visible(), !initially);
    hold(&mut h, chord(), 1);
    assert_eq!(h.state().diagnostics().visible(), initially);
}

#[test]
fn holding_the_chord_toggles_once() {
    let mut h = harness();
    let initially = h.state().diagnostics().visible();
    hold(&mut h, chord(), 5);
    assert_eq!(h.state().diagnostics().visible(), !initially);
}

#[test]
fn chord_without_alt_does_nothing() {
    let mut h = harness();
    let initially = h.state().diagnostics().visible();
    hold(
        &mut h,
        Modifiers::CTRL | Modifiers::SHIFT | Modifiers::COMMAND,
        1,
    );
    assert_eq!(h.state().diagnostics().visible(), initially);
}
