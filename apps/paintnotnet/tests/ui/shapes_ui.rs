//! Phase 7b: Shapes, end to end through real events.

use crate::common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use paintnotnet::commands::{Command, Triggered};
use pnn_core::ColorBgra;
use pnn_tools::{ShapeDrawType, ShapeType, ToolKind};

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

const RED: ColorBgra = ColorBgra::from_rgba(255, 0, 0, 255);

fn drag2(h: &mut H<'_>, b: PointerButton, m: Modifiers, from: (f64, f64), to: (f64, f64)) {
    let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    drag(h, b, m, &[from, mid, to]);
}

/// Shapes tool, filled rectangles (easy to probe), width 2.
fn start(h: &mut H<'_>) {
    press(h, Modifiers::SHIFT, Key::O);
    assert_eq!(h.state().tool(), ToolKind::Shapes);
    let s = h.state_mut().settings_mut();
    s.shape = ShapeType::Rectangle;
    s.shape_draw = ShapeDrawType::Interior;
    s.brush_width = 2.0;
}

fn px_at(h: &H<'_>, x: u32, y: u32) -> ColorBgra {
    doc(h).layer(0).surface.get(x, y)
}

fn black(h: &H<'_>, x: u32, y: u32) -> bool {
    px_at(h, x, y) == ColorBgra::BLACK
}

#[test]
fn draw_modes_and_commit() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (20.0, 20.0),
        (60.0, 50.0),
    );
    assert!(black(&h, 21, 21) && black(&h, 40, 35) && black(&h, 59, 49));
    assert!(!black(&h, 19, 35) && !black(&h, 61, 35) && !black(&h, 40, 51));
    assert_eq!(history_names(&h).len(), n, "live");
    // Outline mode re-renders the live shape: the inside clears.
    h.state_mut().settings_mut().shape_draw = ShapeDrawType::Outline;
    h.step();
    assert!(!black(&h, 40, 35) && black(&h, 20, 35));
    // Filled with outline: the fill takes the secondary colour.
    {
        let s = h.state_mut().settings_mut();
        s.shape_draw = ShapeDrawType::Both;
        s.secondary = RED;
    }
    h.step();
    assert_eq!(px_at(&h, 40, 35), RED);
    assert!(black(&h, 20, 35));
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(history_names(&h).last().unwrap(), "Shapes");
    assert_eq!(history_names(&h).len(), n + 1);
}

#[test]
fn shift_draws_a_square_and_corner_nubs_resize() {
    let mut h = harness();
    start(&mut h);
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::SHIFT,
        (20.0, 20.0),
        (60.0, 40.0),
    );
    assert!(black(&h, 40, 58) && !black(&h, 40, 61), "40×40 square");
    // Drag the bottom-right corner from (60, 60) to (80, 70).
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (60.0, 60.0),
        (80.0, 70.0),
    );
    assert!(black(&h, 78, 68) && !black(&h, 82, 50) && !black(&h, 50, 72));
    assert!(black(&h, 21, 21), "the opposite corner stays put");
}

#[test]
fn drag_inside_moves_and_arrows_nudge() {
    let mut h = harness();
    start(&mut h);
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (20.0, 20.0),
        (40.0, 40.0),
    );
    // Off-centre: the centre holds the rotation point.
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (25.0, 35.0),
        (55.0, 55.0),
    );
    assert!(!black(&h, 25, 25) && black(&h, 55, 45) && black(&h, 69, 59));
    press(&mut h, Modifiers::CTRL, Key::ArrowLeft);
    assert!(!black(&h, 69, 59) && black(&h, 59, 59), "moved 10 px left");
}

#[test]
fn right_drag_rotates_about_the_rotation_point() {
    let mut h = harness();
    start(&mut h);
    // A wide bar 20..60 × 30..40, centred on (40, 35).
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (20.0, 30.0),
        (60.0, 40.0),
    );
    drag(
        &mut h,
        PointerButton::Secondary,
        Modifiers::SHIFT,
        &[(60.0, 35.0), (55.0, 47.0), (41.0, 55.0)],
    );
    assert!(
        black(&h, 40, 18) && black(&h, 40, 52),
        "now tall (snapped to 90°)"
    );
    assert!(!black(&h, 25, 35) && !black(&h, 55, 35));
}

#[test]
fn a_picks_the_next_shape_and_undo_drops_the_live_one() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (20.0, 20.0),
        (60.0, 60.0),
    );
    // A picks the next shape for the next drawing; the live rectangle stays a rectangle.
    press(&mut h, Modifiers::NONE, Key::A);
    assert_eq!(
        h.state_mut().settings_mut().shape,
        ShapeType::RoundedRectangle
    );
    assert!(black(&h, 21, 21) && black(&h, 40, 40));
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Triggered::once(Command::Undo));
    h.step();
    assert!(!black(&h, 40, 40), "undo took back the live shape");
    assert_eq!(history_names(&h).len(), n);
    // The next shape is a rounded rectangle (corner radius 20): its corner pixel is clear.
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (20.0, 20.0),
        (60.0, 60.0),
    );
    assert!(!black(&h, 21, 21) && black(&h, 40, 40));
}

#[test]
fn pressing_far_away_commits_and_starts_another() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (10.0, 10.0),
        (30.0, 30.0),
    );
    drag2(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        (60.0, 50.0),
        (90.0, 70.0),
    );
    assert_eq!(history_names(&h).len(), n + 1);
    assert!(black(&h, 20, 20) && black(&h, 75, 60));
}

/// The Line/Curve and Shapes tools' keys (5.x: A / Shift+A cycle shapes; , . / the caps and
/// line style) act only with their tool, and show as available only then.
#[test]
fn tool_keys_act_only_with_their_tool() {
    let mut h = harness();
    h.state_mut().set_tool(ToolKind::Paintbrush);
    h.step();
    let (shape, dash, cap) = {
        let s = h.state_mut().settings_mut();
        (s.shape, s.dash, s.end_cap)
    };
    press(&mut h, Modifiers::NONE, Key::A);
    press(&mut h, Modifiers::NONE, Key::Period);
    press(&mut h, Modifiers::NONE, Key::Slash);
    {
        let s = h.state_mut().settings_mut();
        assert_eq!(
            (s.shape, s.dash, s.end_cap),
            (shape, dash, cap),
            "Paintbrush: no change"
        );
    }
    assert!(
        !h.state()
            .availability(Command::CycleShape { forward: true })
    );

    h.state_mut().set_tool(ToolKind::Shapes);
    h.step();
    assert!(
        h.state()
            .availability(Command::CycleShape { forward: true })
    );
    press(&mut h, Modifiers::NONE, Key::A);
    assert_eq!(h.state_mut().settings_mut().shape, shape.cycle(true));
    press(&mut h, Modifiers::SHIFT, Key::A);
    assert_eq!(h.state_mut().settings_mut().shape, shape);
    press(&mut h, Modifiers::NONE, Key::Period);
    assert_ne!(
        h.state_mut().settings_mut().dash,
        dash,
        "Shapes have a line style"
    );
    press(&mut h, Modifiers::NONE, Key::Slash);
    assert_eq!(h.state_mut().settings_mut().end_cap, cap, "but no caps");

    h.state_mut().set_tool(ToolKind::LineCurve);
    h.step();
    press(&mut h, Modifiers::NONE, Key::Slash);
    assert_ne!(h.state_mut().settings_mut().end_cap, cap);
}

/// Effects > Repeat is available once an effect has been applied.
#[test]
fn repeat_effect_is_available_after_an_effect() {
    let mut h = harness();
    assert!(!h.state().availability(Command::RepeatEffect));
    press(
        &mut h,
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        },
        Key::I,
    );
    assert_eq!(history_names(&h).last().unwrap(), "Invert Colors");
    assert!(h.state().availability(Command::RepeatEffect));
}
