//! Phase 6: painting tools, end to end through real events.

use crate::common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use paintnotnet::commands::{Command, Triggered};
use pnn_core::ColorBgra;
use pnn_tools::{ToolBlend, ToolKind};

fn line(
    h: &mut egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>,
    button: PointerButton,
) {
    drag(
        h,
        button,
        Modifiers::NONE,
        &[(10.0, 40.0), (35.0, 40.0), (60.0, 40.0)],
    );
}

#[test]
fn paintbrush_paints_a_soft_edged_stroke_in_one_step() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::B);
    assert_eq!(h.state().tool(), ToolKind::Paintbrush);
    h.state_mut().settings_mut().brush_width = 8.0;
    line(&mut h, PointerButton::Primary);
    let s = &doc(&h).layer(0).surface;
    for x in [12, 30, 47, 58] {
        assert_eq!(s.get(x, 40), ColorBgra::BLACK, "stroke core at x={x}");
    }
    let edge = s.get(30, 43); // centre 3.5 px from the line: in the 75% hardness ramp
    assert!(edge.r > 0 && edge.r < 255, "soft edge, got {edge:?}");
    assert_eq!(s.get(30, 47), ColorBgra::WHITE, "outside the brush");
    assert_eq!(s.get(70, 40), ColorBgra::WHITE, "past the end");
    assert_eq!(history_names(&h).last().unwrap(), "Paintbrush");
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Triggered::once(Command::Undo));
    h.step();
    assert_eq!(
        doc(&h).layer(0).surface.get(30, 40),
        ColorBgra::WHITE,
        "undone"
    );
}

#[test]
fn right_button_paints_secondary_and_strokes_do_not_build_up() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::B);
    {
        let s = h.state_mut().settings_mut();
        s.brush_width = 6.0;
        s.secondary = ColorBgra::from_rgba(255, 0, 0, 128);
    }
    // Back and forth over the same pixels in one stroke.
    drag(
        &mut h,
        PointerButton::Secondary,
        Modifiers::NONE,
        &[(10.0, 40.0), (60.0, 40.0), (10.0, 40.0), (60.0, 40.0)],
    );
    let p = doc(&h).layer(0).surface.get(30, 40);
    // Half-transparent red over white once: (255, 127, 127).
    assert_eq!((p.r, p.a), (255, 255));
    assert!((126..=128).contains(&p.g), "blended once, got {p:?}");
}

#[test]
fn paintbrush_respects_the_selection() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(20.0, 20.0), (40.0, 60.0)],
    );
    press(&mut h, Modifiers::NONE, Key::B);
    h.state_mut().settings_mut().brush_width = 6.0;
    line(&mut h, PointerButton::Primary);
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(30, 40), ColorBgra::BLACK, "inside the selection");
    assert_eq!(s.get(15, 40), ColorBgra::WHITE, "left of the selection");
    assert_eq!(s.get(50, 40), ColorBgra::WHITE, "right of the selection");
}

#[test]
fn eraser_clears_alpha_and_keeps_colour() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::E);
    assert_eq!(h.state().tool(), ToolKind::Eraser);
    h.state_mut().settings_mut().brush_width = 6.0;
    line(&mut h, PointerButton::Secondary);
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(30, 40).a, 0, "erased");
    let edge = s.get(30, 42); // 2.5 px out: in the ramp
    assert!(edge.a > 0 && edge.a < 255, "soft edge, got {edge:?}");
    assert_eq!((edge.r, edge.g, edge.b), (255, 255, 255), "colour kept");
    assert_eq!(s.get(30, 47).a, 255, "outside the brush");
    assert_eq!(history_names(&h).last().unwrap(), "Eraser");
}

#[test]
fn overwrite_mode_replaces_pixels() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::B);
    {
        let s = h.state_mut().settings_mut();
        s.brush_width = 6.0;
        s.primary = ColorBgra::from_rgba(0, 0, 255, 100);
        s.blend = ToolBlend::Overwrite;
    }
    line(&mut h, PointerButton::Primary);
    assert_eq!(
        doc(&h).layer(0).surface.get(30, 40),
        ColorBgra::from_rgba(0, 0, 255, 100),
        "the pixel is replaced, not blended"
    );
}

#[test]
fn switching_tools_mid_stroke_commits_it() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::B);
    h.state_mut().settings_mut().brush_width = 4.0;
    // Press and move without releasing, then switch tools.
    h.event(eframe::egui::Event::PointerMoved(screen(&h, 10.0, 40.0)));
    h.step();
    h.event(eframe::egui::Event::PointerButton {
        pos: screen(&h, 10.0, 40.0),
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.event(eframe::egui::Event::PointerMoved(screen(&h, 50.0, 40.0)));
    h.step();
    h.state_mut().set_tool(ToolKind::Pencil);
    h.step();
    assert_eq!(history_names(&h).last().unwrap(), "Paintbrush");
    assert_eq!(doc(&h).layer(0).surface.get(30, 40), ColorBgra::BLACK);
}

/// Alt + right-drag resizes the brush instead of painting: 100 pt to the right doubles it,
/// 40 pt up adds 20% hardness.
#[test]
fn alt_right_drag_resizes_the_brush_without_painting() {
    let mut h = harness();
    h.state_mut().set_tool(ToolKind::Paintbrush);
    h.step();
    let (width, hardness) = (
        h.state().settings().brush_width,
        h.state().settings().hardness,
    );
    let history = history_names(&h);
    // The harness shows the image at 400%: 25 image px right = 100 pt, 10 up = 40 pt.
    drag(
        &mut h,
        PointerButton::Secondary,
        Modifiers::ALT,
        &[(50.0, 40.0), (60.0, 36.0), (75.0, 30.0)],
    );
    let s = h.state().settings();
    assert!(
        (s.brush_width - width * 2.0).abs() < 0.051,
        "{width} → {}",
        s.brush_width
    );
    assert_eq!(s.hardness, hardness + 20);
    assert_eq!(history_names(&h), history, "nothing painted");
}

/// The Paintbrush's Fill is in the options row (it was behind "…", where opening the dropdown
/// closed the menu around it): picking a pattern sticks.
#[test]
fn paintbrush_fill_picks_a_pattern() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    h.state_mut().set_tool(pnn_tools::ToolKind::Paintbrush);
    h.step();
    h.step();
    h.get_by_value("Solid Color").click();
    h.step();
    h.step();
    h.get_by_label("Forward Diagonal").click();
    h.step();
    h.step();
    assert_eq!(h.state().settings().fill, pnn_tools::FillStyle::Hatch(2));
}
