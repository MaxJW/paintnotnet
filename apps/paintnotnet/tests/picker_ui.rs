//! Phase 6c: Color Picker, Recolor and Clone Stamp, end to end through real events.

mod common;

use common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use pnn_core::{ColorBgra, Document, Layer, Surface};
use pnn_tools::{ColorPickerAfterClick, ColorPickerSampling, ToolKind};

const RED: ColorBgra = ColorBgra::from_rgba(255, 0, 0, 255);
const BLUE: ColorBgra = ColorBgra::from_rgba(0, 0, 255, 255);

/// 100×80: red where x < 50, blue elsewhere; a white 20×20 square at (60..80, 10..30).
fn halves() -> Document {
    let mut s = Surface::new_transparent(100, 80);
    for y in 0..80 {
        for x in 0..100 {
            let c = if (60..80).contains(&x) && (10..30).contains(&y) {
                ColorBgra::WHITE
            } else if x < 50 {
                RED
            } else {
                BLUE
            };
            s.set(x, y, c);
        }
    }
    Document::from_layers(100, 80, vec![Layer::new("Background", s)])
}

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

fn click(h: &mut H<'_>, button: PointerButton, m: Modifiers, (x, y): (u32, u32)) {
    let p = px(x, y);
    drag(h, button, m, &[p, p]);
}

#[test]
fn color_picker_sets_primary_and_secondary() {
    let mut h = harness_with(halves());
    press(&mut h, Modifiers::NONE, Key::K);
    assert_eq!(h.state().tool(), ToolKind::ColorPicker);
    click(&mut h, PointerButton::Primary, Modifiers::NONE, (10, 10));
    click(&mut h, PointerButton::Secondary, Modifiers::NONE, (70, 20));
    let s = h.state_mut().settings_mut();
    assert_eq!((s.primary, s.secondary), (RED, ColorBgra::WHITE));
}

#[test]
fn color_picker_averages_a_square() {
    let mut h = harness_with(halves());
    press(&mut h, Modifiers::NONE, Key::K);
    h.state_mut().settings_mut().picker_sampling = ColorPickerSampling::Square3;
    // Columns 49 (red), 50 and 51 (blue).
    click(&mut h, PointerButton::Primary, Modifiers::NONE, (50, 50));
    let c = h.state_mut().settings_mut().primary;
    assert_eq!((c.r, c.g, c.b, c.a), (85, 0, 170, 255));
}

#[test]
fn ctrl_samples_the_image_and_after_click_switches_back() {
    let mut top = Layer::new("Layer 2", Surface::new_transparent(100, 80));
    top.visible = true;
    let mut d = halves();
    let base = d.layer(0).clone();
    d = Document::from_layers(100, 80, vec![base, top]);
    let mut h = harness_with(d);
    press(&mut h, Modifiers::NONE, Key::B);
    press(&mut h, Modifiers::NONE, Key::K);
    h.state_mut().settings_mut().picker_after_click = ColorPickerAfterClick::SwitchToPrevious;
    click(&mut h, PointerButton::Primary, Modifiers::NONE, (10, 10));
    assert_eq!(
        h.state_mut().settings_mut().primary.a,
        0,
        "the active (top) layer is transparent there"
    );
    assert_eq!(h.state().tool(), ToolKind::Paintbrush, "switched back");
    press(&mut h, Modifiers::NONE, Key::K);
    click(&mut h, PointerButton::Primary, Modifiers::CTRL, (10, 10));
    assert_eq!(
        h.state_mut().settings_mut().primary,
        RED,
        "Ctrl samples the image"
    );
}

#[test]
fn recolor_replaces_the_secondary_colour_with_the_primary() {
    let mut h = harness_with(halves());
    press(&mut h, Modifiers::NONE, Key::R);
    assert_eq!(h.state().tool(), ToolKind::Recolor);
    {
        let s = h.state_mut().settings_mut();
        s.primary = ColorBgra::from_rgba(0, 200, 0, 255);
        s.secondary = RED;
        s.brush_width = 10.0;
    }
    // Across red, into blue.
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(30.0, 50.0), (45.0, 50.0), (60.0, 50.0)],
    );
    let s = &doc(&h).layer(0).surface;
    // (255,0,0) + (0,200,0) − (255,0,0) = (0,200,0).
    assert_eq!(s.get(40, 50), ColorBgra::from_rgba(0, 200, 0, 255));
    assert_eq!(s.get(55, 50), BLUE, "blue is out of tolerance");
    assert_eq!(s.get(40, 60), RED, "outside the brush");
    assert_eq!(history_names(&h).last().unwrap(), "Recolor");
    // Ctrl+click picks the primary from the image, keeping the primary's alpha.
    h.state_mut().settings_mut().primary = ColorBgra::from_rgba(1, 2, 3, 100);
    click(&mut h, PointerButton::Primary, Modifiers::CTRL, (70, 20));
    assert_eq!(
        h.state_mut().settings_mut().primary,
        ColorBgra::WHITE.with_alpha(100)
    );
}

#[test]
fn clone_stamp_copies_from_the_ctrl_clicked_source() {
    let mut h = harness_with(halves());
    press(&mut h, Modifiers::NONE, Key::L);
    assert_eq!(h.state().tool(), ToolKind::CloneStamp);
    h.state_mut().settings_mut().brush_width = 6.0;
    let n = history_names(&h).len();
    // No source yet: nothing happens.
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(20.0, 60.0), (25.0, 60.0)],
    );
    assert_eq!(history_names(&h).len(), n);
    // Source: the white square's centre. Paint over the red half.
    click(&mut h, PointerButton::Primary, Modifiers::CTRL, (70, 20));
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(20.5, 60.5), (24.5, 60.5)],
    );
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(22, 60), ColorBgra::WHITE, "cloned");
    assert_eq!(s.get(22, 70), RED, "outside the brush");
    assert_eq!(history_names(&h).last().unwrap(), "Clone Stamp");
    // Aligned: the next stroke keeps the same offset (−50, +40) → (30, 70) copies (80, 30),
    // just past the square: blue.
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(30.5, 70.5), (31.5, 70.5)],
    );
    assert_eq!(doc(&h).layer(0).surface.get(30, 70), BLUE);
}
