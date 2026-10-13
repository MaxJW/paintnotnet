//! Canvas view input: wheel zoom (default) and paint.net's wheel scrolling (Settings), and
//! panning past the image edge.

use crate::common::*;
use eframe::egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::config::{Config, MouseWheel};
use pnn_core::Document;
use pnn_render::{View, view};

fn big() -> Harness<'static, PaintNotNetApp> {
    let mut h = harness_with(Document::new_blank(4000, 3000));
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.view = Some(View {
        zoom: 1.0,
        center: [2000.0, 1500.0],
        fit_to_window: false,
    });
    h.step();
    h
}

fn view(h: &Harness<'_, PaintNotNetApp>) -> View {
    h.state().workspace().active().unwrap().view.unwrap()
}

fn wheel(h: &mut Harness<'_, PaintNotNetApp>, at: Pos2, delta: [f32; 2], modifiers: Modifiers) {
    h.event(Event::PointerMoved(at));
    h.step();
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(delta[0], delta[1]),
        modifiers,
        phase: eframe::egui::TouchPhase::Move,
    });
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    for _ in 0..4 {
        h.step();
    }
}

#[test]
fn wheel_zooms_about_the_pointer() {
    let mut h = big();
    let at = screen(&h, 1800.0, 1400.0);
    wheel(&mut h, at, [0.0, 1.0], Modifiers::NONE);
    let v = view(&h);
    assert_eq!(v.zoom, view::next_zoom_in(1.0));
    // The image point under the pointer stays put.
    let p = screen(&h, 1800.0, 1400.0);
    assert!((p - at).length() < 0.5, "{p:?} vs {at:?}");

    wheel(&mut h, at, [0.0, -1.0], Modifiers::NONE);
    wheel(&mut h, at, [0.0, -1.0], Modifiers::NONE);
    assert_eq!(view(&h).zoom, view::next_zoom_out(1.0));
}

#[test]
fn shift_wheel_scrolls_sideways_without_zooming() {
    let mut h = big();
    let at = screen(&h, 2000.0, 1500.0);
    let before = view(&h);
    wheel(&mut h, at, [0.0, -1.0], Modifiers::SHIFT);
    let after = view(&h);
    assert_eq!(after.zoom, 1.0);
    assert!(after.center[0] > before.center[0] + 10.0, "{after:?}");
    assert_eq!(after.center[1], before.center[1]);
}

#[test]
fn scroll_setting_restores_paint_net_wheel() {
    let mut h = big();
    let ctx = h.ctx.clone();
    let config = Config {
        mouse_wheel: MouseWheel::Scroll,
        ..Config::default()
    };
    h.state_mut().set_config(&ctx, config, None);
    let at = screen(&h, 2000.0, 1500.0);
    let before = view(&h);
    wheel(&mut h, at, [0.0, -1.0], Modifiers::NONE);
    // egui smooths line scrolling over a few frames.
    for _ in 0..30 {
        h.step();
    }
    let after = view(&h);
    assert_eq!(after.zoom, 1.0);
    assert!(after.center[1] > before.center[1] + 10.0, "{after:?}");
    wheel(&mut h, at, [0.0, 1.0], Modifiers::CTRL);
    assert_eq!(view(&h).zoom, view::next_zoom_in(1.0));
}

#[test]
fn middle_drag_pans_past_the_image_edge() {
    let mut h = big();
    let rect = h.state().canvas().rect.unwrap();
    // Drag the image far to the right and down: its top-left corner stops at the middle of
    // the canvas instead of at the canvas corner.
    let start = rect.min + vec2(20.0, 20.0);
    h.event(Event::PointerMoved(start));
    h.step();
    h.event(Event::PointerButton {
        pos: start,
        button: PointerButton::Middle,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=40 {
        h.event(Event::PointerMoved(
            start + vec2(i as f32 * 100.0, i as f32 * 100.0),
        ));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: start + vec2(4000.0, 4000.0),
        button: PointerButton::Middle,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.step();
    let corner = screen(&h, 0.0, 0.0);
    assert!(
        (corner - rect.center()).length() < 1.0,
        "{corner:?} vs {:?}",
        rect.center()
    );
    // And the view stays there on later frames (no snap back).
    h.step();
    assert!((screen(&h, 0.0, 0.0) - rect.center()).length() < 1.0);
}
