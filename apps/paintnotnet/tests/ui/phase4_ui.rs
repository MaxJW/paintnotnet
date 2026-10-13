//! Phase 4a: selection tools and Edit menu selection commands, driven through real events.

use crate::common::*;
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

/// 100×80 white image with a 10×10 red block at (10,10).
fn red_block() -> Document {
    let mut s = Surface::new_transparent(100, 80);
    s.fill(ColorBgra::WHITE);
    for y in 10..20 {
        for x in 10..20 {
            s.set(x, y, ColorBgra::from_rgba(255, 0, 0, 255));
        }
    }
    Document::from_layers(100, 80, vec![Layer::new("Background", s)])
}

const RED: ColorBgra = ColorBgra::from_rgba(255, 0, 0, 255);

#[test]
fn move_selected_pixels_drag_and_finish() {
    let mut h = harness_with(red_block());
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (20.0, 20.0)],
    );
    press(&mut h, Modifiers::NONE, Key::M);
    assert_eq!(h.state().tool(), ToolKind::MoveSelectedPixels);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(15.0, 15.0), (25.0, 15.0), (45.0, 15.0)],
    );
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(45, 15), RED, "moved 30 px right");
    assert_eq!(s.get(15, 15), ColorBgra::TRANSPARENT, "hole left behind");
    let b = doc(&h).selection().pixel_bounds(100, 80).unwrap();
    assert_eq!((b.x, b.y), (40, 10), "selection moved with the pixels");
    press(&mut h, Modifiers::NONE, Key::Enter);
    let names = history_names(&h);
    assert_eq!(&names[names.len() - 2..], ["Move Pixels", "Finish Pixels"]);
}

#[test]
fn ctrl_drag_moves_a_copy_and_arrows_nudge() {
    let mut h = harness_with(red_block());
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (20.0, 20.0)],
    );
    press(&mut h, Modifiers::NONE, Key::M);
    drag(
        &mut h,
        PointerButton::Primary,
        ctrl(),
        &[(15.0, 15.0), (35.0, 15.0)],
    );
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(15, 15), RED, "original stays");
    assert_eq!(s.get(35, 15), RED, "copy moved");
    press(&mut h, Modifiers::NONE, Key::ArrowDown);
    press(&mut h, ctrl(), Key::ArrowRight);
    let b = doc(&h).selection().pixel_bounds(100, 80).unwrap();
    assert_eq!((b.x, b.y), (40, 11), "1 px down, 10 px right");
    assert_eq!(doc(&h).layer(0).surface.get(49, 20), RED);
}

#[test]
fn move_selection_moves_only_the_outline() {
    let mut h = harness_with(red_block());
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (20.0, 20.0)],
    );
    press(&mut h, Modifiers::SHIFT, Key::M);
    assert_eq!(h.state().tool(), ToolKind::MoveSelection);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(15.0, 15.0), (35.0, 35.0)],
    );
    assert_eq!(
        doc(&h).layer(0).surface.get(15, 15),
        RED,
        "pixels untouched"
    );
    let b = doc(&h).selection().pixel_bounds(100, 80).unwrap();
    assert_eq!((b.x, b.y), (30, 30));
    assert_eq!(history_names(&h).last().unwrap(), "Move Selection");
}

#[test]
fn nub_scales_and_right_drag_rotates() {
    let mut h = harness_with(red_block());
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (20.0, 20.0)],
    );
    press(&mut h, Modifiers::SHIFT, Key::M);
    // Bottom-right nub at (20,20) dragged to (30,30): selection doubles.
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(20.0, 20.0), (25.0, 25.0), (30.0, 30.0)],
    );
    let b = doc(&h).selection().pixel_bounds(100, 80).unwrap();
    assert_eq!((b.x, b.y, b.width, b.height), (10, 10, 20, 20));
    assert_eq!(history_names(&h).last().unwrap(), "Scale Selection");
    // Right-drag half a turn about the centre (20,20) with Shift snapping.
    drag(
        &mut h,
        PointerButton::Secondary,
        Modifiers::SHIFT,
        &[(40.0, 20.0), (20.0, 41.0), (0.0, 21.0)],
    );
    let b = doc(&h).selection().pixel_bounds(100, 80).unwrap();
    assert_eq!(
        (b.x, b.y, b.width, b.height),
        (10, 10, 20, 20),
        "180° keeps the square in place"
    );
    assert_eq!(history_names(&h).last().unwrap(), "Rotate Selection");
}

#[test]
fn move_with_nothing_selected_moves_the_layer() {
    let mut h = harness_with(red_block());
    press(&mut h, Modifiers::NONE, Key::M);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(50.0, 50.0), (55.0, 50.0)],
    );
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(20, 15), RED, "block shifted 5 px");
    assert_eq!(s.get(2, 2), ColorBgra::TRANSPARENT, "left edge uncovered");
}
