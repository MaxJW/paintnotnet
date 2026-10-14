//! Phase 14c: the Colors panel's square, strips, sliders and recent colours.

use eframe::egui::{Event, Modifiers, PointerButton, Pos2, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Triggered};
use pnn_core::{ColorBgra, Document};

fn harness() -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder()
        .with_size(vec2(1202.0, 867.0))
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.state_mut()
        .workspace_mut()
        .add(Document::new_blank(200, 150), None, None);
    let ctx = h.ctx.clone();
    if h.state().diagnostics().visible() {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
    }
    for _ in 0..4 {
        h.step();
    }
    h
}

fn rect_of(h: &Harness<'_, PaintNotNetApp>, label: &str) -> Rect {
    h.get_by_label(label).rect()
}

fn drag(h: &mut Harness<'_, PaintNotNetApp>, button: PointerButton, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton {
        pos: from,
        button,
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
        button,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.step();
}

fn near(c: ColorBgra, r: u8, g: u8, b: u8) -> bool {
    c.r.abs_diff(r) <= 3 && c.g.abs_diff(g) <= 3 && c.b.abs_diff(b) <= 3
}

#[test]
fn square_and_hue_strip_pick_and_become_recent() {
    let mut h = harness();
    let square = rect_of(&h, "Saturation and value");
    // From black (hue 0): to 75% saturation and 75% value is a muted red.
    let at = |x: f32, y: f32| square.min + vec2(square.width() * x, square.height() * y);
    drag(&mut h, PointerButton::Primary, at(0.2, 0.9), at(0.75, 0.25));
    let p = h.state().settings().primary;
    assert!(near(p, 191, 48, 48), "{p:?}");
    assert_eq!(h.state().settings().secondary, ColorBgra::WHITE);

    // A third of the way along the hue strip is green; the pick shows up under Recent.
    let hue = rect_of(&h, "Hue");
    let third = hue.left_center() + vec2(hue.width() / 3.0, 0.0);
    drag(&mut h, PointerButton::Primary, third, third);
    let p = h.state().settings().primary;
    assert!(near(p, 48, 191, 48), "{p:?}");
    assert!(h.query_by_label("Recent color 1").is_some());

    // Right button edits the other colour.
    drag(&mut h, PointerButton::Secondary, at(0.9, 0.1), at(1.0, 0.0));
    let s = h.state().settings().secondary;
    assert!(
        near(s, 255, 0, 0),
        "secondary from white (hue kept at 0): {s:?}"
    );
    assert!(near(h.state().settings().primary, 48, 191, 48));
}

#[test]
fn title_row_switches_the_sliders() {
    let mut h = harness();
    assert!(h.query_by_label("L slider").is_some(), "OKLCH by default");
    h.get_by_label("HSV sliders").click();
    h.step();
    h.step();
    assert!(h.query_by_label("S slider").is_some());
    h.get_by_label("RGB sliders").click();
    h.step();
    h.step();
    assert!(h.query_by_label("G slider").is_some());
    assert!(h.query_by_label("L slider").is_none());
}
