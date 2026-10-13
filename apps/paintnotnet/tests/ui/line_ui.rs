//! Phase 7a: Line/Curve, end to end through real events.

use crate::common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use paintnotnet::commands::{Command, Triggered};
use pnn_core::ColorBgra;
use pnn_tools::{CurveType, DashStyle, ToolKind};

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

fn line(h: &mut H<'_>, m: Modifiers, from: (f64, f64), to: (f64, f64)) {
    let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    drag(h, PointerButton::Primary, m, &[from, mid, to]);
}

fn start(h: &mut H<'_>) {
    press(h, Modifiers::NONE, Key::O);
    assert_eq!(h.state().tool(), ToolKind::LineCurve);
    let s = h.state_mut().settings_mut();
    s.brush_width = 4.0;
    s.curve_type = CurveType::Spline;
}

fn black(h: &H<'_>, x: u32, y: u32) -> bool {
    doc(h).layer(0).surface.get(x, y) == ColorBgra::BLACK
}

#[test]
fn line_is_live_until_enter() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    line(&mut h, Modifiers::NONE, (10.0, 40.0), (90.0, 40.0));
    assert!(black(&h, 50, 38) && black(&h, 50, 41));
    assert!(!black(&h, 50, 43) && !black(&h, 5, 40) && !black(&h, 95, 40));
    assert_eq!(history_names(&h).len(), n, "live");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(history_names(&h).last().unwrap(), "Line / Curve");
    assert!(black(&h, 50, 40));
}

#[test]
fn nubs_bend_the_spline_and_the_move_icon_moves_it() {
    let mut h = harness();
    start(&mut h);
    line(&mut h, Modifiers::NONE, (10.0, 40.0), (70.0, 40.0));
    // The third nub sits at x = 50; drag it up to y = 10. A spline passes through it.
    line(&mut h, Modifiers::NONE, (50.0, 40.0), (50.0, 10.0));
    assert!(black(&h, 50, 10), "curve goes through the moved nub");
    assert!(!black(&h, 50, 40));
    // Move icon: 16 screen px past the end nub at 400% = 4 image px.
    line(&mut h, Modifiers::NONE, (74.0, 44.0), (74.0, 64.0));
    assert!(black(&h, 50, 30) && !black(&h, 50, 10), "moved down 20 px");
    assert!(black(&h, 11, 60), "start nub moved too");
    // Arrow keys nudge it.
    press(&mut h, Modifiers::NONE, Key::ArrowDown);
    press(&mut h, Modifiers::CTRL, Key::ArrowDown);
    assert!(black(&h, 50, 41) && !black(&h, 50, 29), "moved 11 px more");
}

#[test]
fn shift_snaps_and_drawing_elsewhere_commits() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    line(&mut h, Modifiers::SHIFT, (10.0, 40.0), (90.0, 46.0));
    assert!(black(&h, 85, 40) && !black(&h, 85, 45), "snapped flat");
    line(&mut h, Modifiers::NONE, (10.0, 70.0), (90.0, 70.0));
    assert_eq!(
        history_names(&h).len(),
        n + 1,
        "the first line was committed"
    );
    assert!(black(&h, 50, 40) && black(&h, 50, 70));
}

#[test]
fn right_drag_rotates_about_the_centre() {
    let mut h = harness();
    start(&mut h);
    line(&mut h, Modifiers::NONE, (10.0, 40.0), (90.0, 40.0));
    // From just right of centre to just below it: +90°.
    drag(
        &mut h,
        PointerButton::Secondary,
        Modifiers::NONE,
        &[(60.0, 40.0), (57.0, 47.0), (50.0, 50.0)],
    );
    assert!(black(&h, 50, 5) && black(&h, 50, 75), "now vertical");
    assert!(!black(&h, 20, 40));
}

#[test]
fn dash_key_restyles_the_live_line_and_undo_drops_it() {
    let mut h = harness();
    start(&mut h);
    let n = history_names(&h).len();
    line(&mut h, Modifiers::NONE, (10.0, 40.0), (90.0, 40.0));
    let ctx = h.ctx.clone();
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::LineStyle));
    h.step();
    assert_eq!(h.state_mut().settings_mut().dash, DashStyle::Dash);
    // Dash = 12 px on, 4 px off at width 4, from x = 10.
    assert!(black(&h, 15, 40) && !black(&h, 24, 40) && black(&h, 30, 40));
    h.state_mut().execute(&ctx, Triggered::once(Command::Undo));
    h.step();
    assert!(!black(&h, 15, 40), "undo took back the live line");
    assert_eq!(history_names(&h).len(), n);
}
