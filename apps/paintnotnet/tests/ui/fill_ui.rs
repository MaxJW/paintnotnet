//! Phase 6b: Paint Bucket and Gradient, end to end through real events.

use crate::common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use paintnotnet::commands::{Command, Triggered};
use pnn_core::{ColorBgra, Document, Layer, Surface};
use pnn_tools::{GradientColorMode, ToolKind};

const RED: ColorBgra = ColorBgra::from_rgba(255, 0, 0, 255);

/// White 100×80 image with two red squares: (10..30, 10..30) and (60..80, 10..30).
fn two_squares() -> Document {
    let mut s = Surface::new_transparent(100, 80);
    for y in 0..80 {
        for x in 0..100 {
            let red = (10..30).contains(&y) && ((10..30).contains(&x) || (60..80).contains(&x));
            s.set(x, y, if red { RED } else { ColorBgra::WHITE });
        }
    }
    Document::from_layers(100, 80, vec![Layer::new("Background", s)])
}

fn click(
    h: &mut egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>,
    m: Modifiers,
    (x, y): (u32, u32),
) {
    let p = px(x, y);
    drag(h, PointerButton::Primary, m, &[p, p]);
}

#[test]
fn paint_bucket_fills_contiguous_and_shift_global() {
    let mut h = harness_with(two_squares());
    press(&mut h, Modifiers::NONE, Key::F);
    assert_eq!(h.state().tool(), ToolKind::PaintBucket);
    click(&mut h, Modifiers::NONE, (20, 20));
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(15, 15), ColorBgra::BLACK, "clicked square filled");
    assert_eq!(s.get(70, 15), RED, "other square untouched (contiguous)");
    assert_eq!(s.get(50, 50), ColorBgra::WHITE);
    assert_eq!(history_names(&h).last().unwrap(), "Paint Bucket");
    // Shift flips to global: the white background everywhere.
    h.state_mut().settings_mut().primary = ColorBgra::from_rgba(0, 0, 255, 255);
    click(&mut h, Modifiers::SHIFT, (5, 5));
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(50, 50).b, 255);
    assert_eq!(s.get(99, 79).b, 255);
    assert_eq!(s.get(70, 15), RED, "not matched");
}

#[test]
fn paint_bucket_is_limited_to_the_selection() {
    let mut h = harness_with(two_squares());
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(0.0, 0.0), (20.0, 80.0)],
    );
    press(&mut h, Modifiers::NONE, Key::F);
    click(&mut h, Modifiers::NONE, (5, 50));
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(5, 50), ColorBgra::BLACK);
    assert_eq!(s.get(19, 79), ColorBgra::BLACK);
    assert_eq!(s.get(25, 50), ColorBgra::WHITE, "outside the selection");
    assert_eq!(s.get(15, 15), RED, "different colour");
    let n = history_names(&h).len();
    click(&mut h, Modifiers::NONE, (50, 50)); // outside the selection: nothing
    assert_eq!(history_names(&h).len(), n);
    assert_eq!(doc(&h).layer(0).surface.get(50, 50), ColorBgra::WHITE);
}

fn gradient_drag(
    h: &mut egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>,
    button: PointerButton,
    from: (f64, f64),
    to: (f64, f64),
) {
    let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    drag(h, button, Modifiers::NONE, &[from, mid, to]);
}

#[test]
fn gradient_draws_stays_editable_and_commits_on_enter() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::G);
    assert_eq!(h.state().tool(), ToolKind::Gradient);
    let n = history_names(&h).len();
    gradient_drag(&mut h, PointerButton::Primary, (10.0, 40.0), (90.0, 40.0));
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(5, 10), ColorBgra::BLACK);
    assert_eq!(s.get(10, 10), ColorBgra::BLACK);
    // Half way is 127.5; the dither rounds it either way.
    assert!(matches!(s.get(50, 70).r, 127 | 128), "half way");
    assert_eq!(s.get(90, 10), ColorBgra::WHITE);
    assert_eq!(history_names(&h).len(), n, "still editable");
    // Drag the end nub to x = 50.
    gradient_drag(&mut h, PointerButton::Primary, (90.0, 40.0), (50.0, 40.0));
    assert_eq!(doc(&h).layer(0).surface.get(50, 10), ColorBgra::WHITE);
    assert!(matches!(doc(&h).layer(0).surface.get(30, 10).r, 127 | 128));
    assert_eq!(history_names(&h).len(), n, "nub edit doesn't commit");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(history_names(&h).last().unwrap(), "Gradient");
    assert_eq!(history_names(&h).len(), n + 1);
    assert!(
        matches!(doc(&h).layer(0).surface.get(30, 10).r, 127 | 128),
        "kept"
    );
}

#[test]
fn gradient_escape_commits_and_undo_takes_back_a_live_one() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::G);
    let n = history_names(&h).len();
    gradient_drag(&mut h, PointerButton::Primary, (10.0, 40.0), (90.0, 40.0));
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(doc(&h).layer(0).surface.get(10, 10), ColorBgra::BLACK);
    assert_eq!(
        history_names(&h).len(),
        n + 1,
        "Esc commits, as paint.net 5"
    );
    // A second, live gradient: Undo removes just it, leaving the committed one.
    gradient_drag(&mut h, PointerButton::Primary, (90.0, 40.0), (10.0, 40.0));
    assert_eq!(doc(&h).layer(0).surface.get(10, 10), ColorBgra::WHITE);
    let current = || h.state().workspace().active().unwrap().history.current();
    let before = current();
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Triggered::once(Command::Undo));
    h.step();
    assert_eq!(
        h.state().workspace().active().unwrap().history.current(),
        before
    );
    assert_eq!(doc(&h).layer(0).surface.get(10, 10), ColorBgra::BLACK);
    assert_eq!(history_names(&h).len(), n + 1);
}

#[test]
fn new_gradient_commits_the_previous_and_right_button_swaps() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::G);
    let n = history_names(&h).len();
    gradient_drag(&mut h, PointerButton::Primary, (10.0, 40.0), (90.0, 40.0));
    // Starting away from the nubs commits the first gradient.
    gradient_drag(&mut h, PointerButton::Secondary, (10.0, 20.0), (90.0, 20.0));
    assert_eq!(history_names(&h).len(), n + 1);
    let s = &doc(&h).layer(0).surface;
    assert_eq!(
        s.get(10, 10),
        ColorBgra::WHITE,
        "swapped: secondary at the start"
    );
    assert_eq!(s.get(90, 10), ColorBgra::BLACK);
    h.state_mut().set_tool(ToolKind::Pencil);
    h.step();
    assert_eq!(history_names(&h).len(), n + 2, "tool change commits");
}

#[test]
fn gradient_transparency_mode_fades_alpha() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::G);
    h.state_mut().settings_mut().gradient_color_mode = GradientColorMode::Transparency;
    gradient_drag(&mut h, PointerButton::Primary, (10.0, 40.0), (90.0, 40.0));
    let s = &doc(&h).layer(0).surface;
    assert_eq!(
        s.get(10, 10),
        ColorBgra::WHITE,
        "primary alpha 255 keeps it"
    );
    assert_eq!(s.get(90, 10).a, 0, "inverse of secondary alpha 255");
    assert_eq!(
        (s.get(90, 10).r, s.get(50, 10).g),
        (255, 255),
        "colour kept"
    );
}

#[test]
fn shift_snaps_the_gradient_to_15_degrees() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::G);
    // 5° off horizontal; Shift snaps it flat, so every column is one colour (give or take
    // the dither's one level).
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::SHIFT,
        &[(10.0, 40.0), (50.0, 42.0), (90.0, 47.0)],
    );
    let s = &doc(&h).layer(0).surface;
    for x in [20, 50, 80] {
        let (top, bottom) = (s.get(x, 0), s.get(x, 79));
        assert!(
            top.r.abs_diff(bottom.r) <= 1,
            "column {x}: {top:?} vs {bottom:?}"
        );
    }
    assert_ne!(s.get(20, 0), s.get(80, 0));
}

#[test]
fn toolbar_finish_commits_the_gradient() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::G);
    let n = history_names(&h).len();
    gradient_drag(&mut h, PointerButton::Primary, (10.0, 40.0), (90.0, 40.0));
    h.get_by_label("Finish").click();
    h.step();
    h.step();
    assert_eq!(history_names(&h).len(), n + 1);
    assert_eq!(history_names(&h).last().unwrap(), "Gradient");
}
