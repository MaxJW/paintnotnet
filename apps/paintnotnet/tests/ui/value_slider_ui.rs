//! The dialogs' sliders (`value_slider`): precise to use with the pointer, wheel and keys.

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, accesskit::Role, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use paintnotnet::value_slider::ValueSlider;

/// A 340-point dialog column with one Radius slider (0–200, default 2).
fn harness(start: f64) -> Harness<'static, f64> {
    Harness::builder()
        .with_size(egui::vec2(400.0, 160.0))
        // Frames at 60 fps: two clicks a few frames apart make a double-click.
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, v: &mut f64| {
                ui.set_width(340.0);
                ValueSlider::new("Radius", 0.0, 200.0)
                    .default(2.0)
                    .show(ui, v);
            },
            start,
        )
}

fn track(h: &Harness<'static, f64>) -> egui::Rect {
    h.get_by_role_and_label(Role::Slider, "Radius").rect()
}

/// The x of `v` on the track (the rail runs between the knob's radius at each end).
fn x_of(h: &Harness<'static, f64>, v: f64) -> f32 {
    let r = track(h);
    let (a, b) = (r.min.x + 7.0, r.max.x - 7.0);
    a + (b - a) * (v / 200.0) as f32
}

fn press_drag_release(h: &mut Harness<'static, f64>, points: &[Pos2], modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(points[0]));
    h.step();
    let button = |pressed, pos| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers,
    };
    h.event(button(true, points[0]));
    h.step();
    for &p in &points[1..] {
        h.event(Event::PointerMoved(p));
        h.step();
    }
    h.event(button(false, *points.last().unwrap()));
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

#[test]
fn grabbing_the_knob_keeps_the_value_and_the_track_jumps() {
    let mut h = harness(50.0);
    h.step();
    let y = track(&h).center().y;
    // Pressed a couple of points off the knob's centre: no jump.
    let x = x_of(&h, 50.0) + 2.0;
    press_drag_release(&mut h, &[pos2(x, y)], Modifiers::NONE);
    assert_eq!(*h.state(), 50.0);
    // Dragged from there: moves by the distance dragged.
    let step = x_of(&h, 1.0) - x_of(&h, 0.0);
    press_drag_release(
        &mut h,
        &[pos2(x, y), pos2(x + 30.0 * step, y)],
        Modifiers::NONE,
    );
    assert_eq!(*h.state(), 80.0);
    // A press on the track jumps there.
    let at = pos2(x_of(&h, 150.0), y);
    press_drag_release(&mut h, &[at], Modifiers::NONE);
    assert_eq!(*h.state(), 150.0);
}

#[test]
fn shift_drags_ten_times_finer() {
    let mut h = harness(100.0);
    h.step();
    let y = track(&h).center().y;
    let x = x_of(&h, 100.0);
    let step = x_of(&h, 1.0) - x_of(&h, 0.0);
    press_drag_release(
        &mut h,
        &[pos2(x, y), pos2(x + 50.0 * step, y)],
        Modifiers::SHIFT,
    );
    assert_eq!(*h.state(), 105.0);
}

#[test]
fn wheel_keys_double_click_and_reset() {
    let mut h = harness(10.0);
    h.step();
    let c = track(&h).center();
    h.event(Event::PointerMoved(c));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.step();
    assert_eq!(*h.state(), 11.0, "a wheel notch is one unit");
    // Focus it (a press on the knob), then the keys.
    let y = c.y;
    let x = x_of(&h, 11.0);
    press_drag_release(&mut h, &[pos2(x, y)], Modifiers::NONE);
    for (key, m, want) in [
        (Key::ArrowRight, Modifiers::NONE, 12.0),
        (Key::ArrowLeft, Modifiers::SHIFT, 2.0),
        (Key::PageUp, Modifiers::NONE, 22.0),
        (Key::End, Modifiers::NONE, 200.0),
    ] {
        h.event(Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: m,
        });
        h.step();
        assert_eq!(*h.state(), want, "{key:?}");
    }
    // Double-click resets to the default.
    let x = x_of(&h, 200.0);
    h.event(Event::PointerMoved(pos2(x, y)));
    h.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: pos2(x, y),
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
    }
    assert_eq!(*h.state(), 2.0);
    // So does ↺.
    *h.state_mut() = 77.0;
    h.step();
    h.get_by_role_and_label(Role::Button, "Reset").click();
    h.step();
    assert_eq!(*h.state(), 2.0);
}

/// The toolbar's compact field: drag sideways, Shift for finer, wheel steps, click for the
/// slider pop-over.
#[test]
fn compact_field_drags_scrolls_and_opens() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(500.0, 300.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, v: &mut f64| {
                ValueSlider::new("Hardness", 0.0, 100.0)
                    .suffix("%")
                    .show_compact(ui, v, |_, _| {});
            },
            50.0,
        );
    h.step();
    let r = h.get_by_role_and_label(Role::Slider, "Hardness").rect();
    let (x, y) = (r.center().x, r.center().y);
    // The whole range is 240 points of drag: 24 points is 10 %.
    press_drag_release(
        &mut h,
        &[pos2(x, y), pos2(x + 12.0, y), pos2(x + 24.0, y)],
        Modifiers::NONE,
    );
    assert_eq!(*h.state(), 60.0);
    press_drag_release(
        &mut h,
        &[pos2(x, y), pos2(x + 12.0, y), pos2(x + 24.0, y)],
        Modifiers::SHIFT,
    );
    assert_eq!(*h.state(), 61.0, "Shift: a tenth as far");
    h.event(Event::PointerMoved(pos2(x, y)));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.step();
    assert_eq!(*h.state(), 60.0);
    // A click opens the full slider.
    h.get_by_role_and_label(Role::Slider, "Hardness").click();
    h.step();
    h.step();
    assert_eq!(
        h.get_all_by_role_and_label(Role::Slider, "Hardness")
            .count(),
        2,
        "the pop-over's slider too"
    );
}
