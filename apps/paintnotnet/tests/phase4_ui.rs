//! Phase 4a: selection tools and Edit menu selection commands, driven through real events.

mod common;

use common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use pnn_core::{ColorBgra, Document, Layer, Surface};
use pnn_tools::ToolKind;

/// Selected pixel count at 50% coverage, and the bounds.
fn selected(
    h: &egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>,
) -> (usize, Option<pnn_core::Rect>) {
    let d = doc(h);
    let m = d.selection().rasterize(d.width(), d.height(), false);
    (
        m.to_pixels().iter().filter(|&&v| v > 127).count(),
        d.selection().pixel_bounds(d.width(), d.height()),
    )
}

#[test]
fn rectangle_select_add_subtract_and_history() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    assert_eq!(h.state().tool(), ToolKind::RectangleSelect);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (15.0, 12.0), (20.0, 20.0)],
    );
    assert_eq!(selected(&h).0, 100, "10×10 pixels");
    // Ctrl+drag adds.
    drag(
        &mut h,
        PointerButton::Primary,
        ctrl(),
        &[(30.0, 10.0), (40.0, 20.0)],
    );
    assert_eq!(selected(&h).0, 200);
    // Alt+drag subtracts.
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::ALT,
        &[(10.0, 10.0), (15.0, 20.0)],
    );
    assert_eq!(selected(&h).0, 150);
    assert_eq!(
        history_names(&h),
        [
            "New Image",
            "Rectangle Select",
            "Rectangle Select",
            "Rectangle Select"
        ]
    );
    // A plain click deselects.
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(50.0, 50.0)],
    );
    assert!(doc(&h).selection().is_empty());
    assert_eq!(history_names(&h).last().unwrap(), "Deselect");
}

#[test]
fn right_button_chords_invert_and_intersect() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(0.0, 0.0), (20.0, 10.0)],
    );
    drag(
        &mut h,
        PointerButton::Secondary,
        ctrl(),
        &[(10.0, 0.0), (30.0, 10.0)],
    );
    assert_eq!(selected(&h).0, 200, "xor: 100 + 100");
    drag(
        &mut h,
        PointerButton::Secondary,
        Modifiers::ALT,
        &[(0.0, 0.0), (15.0, 10.0)],
    );
    assert_eq!(selected(&h).0, 100, "intersect keeps 0..10");
}

#[test]
fn ellipse_with_shift_is_a_circle_and_lasso_selects_triangle() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    press(&mut h, Modifiers::NONE, Key::S);
    press(&mut h, Modifiers::NONE, Key::S);
    assert_eq!(h.state().tool(), ToolKind::EllipseSelect);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::SHIFT,
        &[(10.0, 10.0), (30.0, 20.0), (50.0, 30.0)],
    );
    let (_, b) = selected(&h);
    let b = b.unwrap();
    assert_eq!(
        (b.width, b.height),
        (40, 40),
        "Shift constrains to a circle"
    );
    // Lasso (S ×2 within a second of a fresh S).
    press(&mut h, Modifiers::NONE, Key::B);
    press(&mut h, Modifiers::NONE, Key::S);
    press(&mut h, Modifiers::NONE, Key::S);
    assert_eq!(h.state().tool(), ToolKind::LassoSelect);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(0.0, 0.0), (40.0, 0.0), (0.0, 40.0)],
    );
    let (count, _) = selected(&h);
    assert!(
        (780..=820).contains(&count),
        "triangle area ~800, got {count}"
    );
}

#[test]
fn magic_wand_contiguous_and_shift_global() {
    let mut s = Surface::new_transparent(30, 10);
    s.fill(ColorBgra::WHITE);
    for y in 0..10 {
        s.set(10, y, ColorBgra::BLACK);
    }
    let mut h = harness_with(Document::from_layers(
        30,
        10,
        vec![Layer::new("Background", s)],
    ));
    press(&mut h, Modifiers::SHIFT, Key::S);
    assert_eq!(h.state().tool(), ToolKind::MagicWand);
    drag(&mut h, PointerButton::Primary, Modifiers::NONE, &[px(2, 2)]);
    assert_eq!(selected(&h).0, 100, "left white block only");
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::SHIFT,
        &[px(2, 2)],
    );
    assert_eq!(selected(&h).0, 290, "Shift: every white pixel");
    assert_eq!(history_names(&h).last().unwrap(), "Magic Wand");
}

#[test]
fn erase_fill_invert_select_all_crop() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (20.0, 20.0)],
    );
    press(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(
        doc(&h).layer(0).surface.get(15, 15),
        ColorBgra::BLACK,
        "Backspace fills with primary"
    );
    assert_eq!(doc(&h).layer(0).surface.get(25, 15), ColorBgra::WHITE);
    press(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(
        doc(&h).layer(0).surface.get(15, 15),
        ColorBgra::TRANSPARENT,
        "Delete erases"
    );
    press(&mut h, ctrl(), Key::I);
    assert_eq!(selected(&h).0, 100 * 80 - 100, "Ctrl+I inverts");
    press(&mut h, ctrl(), Key::A);
    assert_eq!(selected(&h).0, 100 * 80);
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(doc(&h).selection().is_empty(), "Enter deselects");
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(5.0, 5.0), (25.0, 15.0)],
    );
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::X);
    assert_eq!(
        (doc(&h).width(), doc(&h).height()),
        (20, 10),
        "crop to selection"
    );
    assert_eq!(
        doc(&h).layer(0).surface.get(10, 7),
        ColorBgra::TRANSPARENT,
        "erased area kept"
    );
    press(&mut h, ctrl(), Key::Z);
    assert_eq!((doc(&h).width(), doc(&h).height()), (100, 80), "undo crop");
}

#[test]
fn pencil_is_clipped_to_the_selection() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 0.0), (20.0, 80.0)],
    );
    press(&mut h, Modifiers::NONE, Key::P);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[px(0, 40), px(30, 40)],
    );
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(5, 40), ColorBgra::WHITE, "outside the selection");
    assert_eq!(s.get(15, 40), ColorBgra::BLACK, "inside");
    assert_eq!(s.get(25, 40), ColorBgra::WHITE);
}
