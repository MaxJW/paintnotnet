//! Effect and document dialogs: movable, previewing live behind them, Cancel reverts.

mod common;

use common::*;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use pnn_core::{ColorBgra, Document, Layer, Surface};

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

/// Drag with the mouse in screen coordinates.
fn screen_drag(h: &mut H<'_>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=4 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 4.0)));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.step();
}

#[test]
fn effect_dialog_can_be_dragged_aside() {
    let mut h = harness();
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::P);
    let title = h.get_by_label("Posterize").rect();
    let ok = h.get_by_label("OK").rect();
    // The window's node spans the whole window; grab its title bar.
    let grab = Pos2::new(title.center().x, title.top() + 10.0);
    screen_drag(&mut h, grab, grab + vec2(-300.0, 200.0));
    let moved = h.get_by_label("OK").rect();
    assert!(
        (moved.center() - ok.center() - vec2(-300.0, 200.0)).length() < 2.0,
        "dialog moved with the drag: {ok:?} → {moved:?}"
    );
    // It still works where it is.
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    assert!(h.state_mut().effect_mut().is_none());
}

#[test]
fn feather_object_previews_live_and_cancel_reverts() {
    let mut s = Surface::new_transparent(100, 80);
    for y in 20..60 {
        for x in 20..80 {
            s.set(x, y, ColorBgra::from_rgba(0, 0, 255, 255));
        }
    }
    let mut h = harness_with(Document::from_layers(100, 80, vec![Layer::new("L", s)]));
    let id = pnn_effects::EFFECTS
        .iter()
        .position(|e| e.name == "Feather Object")
        .unwrap() as u16;
    let ctx = h.ctx.clone();
    h.state_mut().execute(
        &ctx,
        paintnotnet::commands::Triggered::once(paintnotnet::commands::Command::Effect(
            pnn_effects::EffectId(id),
        )),
    );
    h.step();
    h.step();
    // Move the dialog away, then nudge a setting through the dialog's UI.
    let title = h.get_by_label("Feather Object").rect();
    let grab = Pos2::new(title.center().x, title.top() + 10.0);
    screen_drag(&mut h, grab, grab + vec2(350.0, 250.0));
    // The default feather previews as soon as the dialog opens: the rim is already soft.
    let rim = doc(&h).layer(0).surface.get(20, 40).a;
    assert!(rim < 255, "previewed on open: {rim}");
    // Changing the profile in the dialog re-renders behind it.
    h.get_by_label("Linear").click();
    h.step();
    h.step();
    let live = doc(&h).layer(0).surface.get(20, 40).a;
    assert_ne!(live, rim, "live update after a change");
    let n = history_names(&h).len();
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    assert_eq!(
        doc(&h).layer(0).surface.get(20, 40),
        ColorBgra::from_rgba(0, 0, 255, 255)
    );
    assert_eq!(history_names(&h).len(), n, "cancel records nothing");
}
