//! Phase 5: feathering, end to end through real events.

mod common;

use common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use paintnotnet::doc_dialogs::Kind;
use pnn_core::selection::{Feather, FeatherPosition, FeatherProfile};
use pnn_core::{ColorBgra, Document, Layer, Surface};

fn linear(radius: f32) -> Feather {
    Feather {
        radius,
        profile: FeatherProfile::Linear,
        position: FeatherPosition::Inside,
    }
}

#[test]
fn shift_f6_feathers_then_delete_erases_softly() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(20.0, 20.0), (60.0, 60.0)],
    );
    press(&mut h, Modifiers::SHIFT, Key::F6);
    {
        let d = h
            .state_mut()
            .doc_dialog_mut()
            .expect("Shift+F6 opens Feather Selection");
        let Kind::FeatherSelection(f) = &mut d.kind else {
            panic!("wrong dialog")
        };
        *f = linear(8.0);
    }
    h.get_by_label("OK").click();
    h.step();
    h.step();
    assert_eq!(doc(&h).selection().feather(), linear(8.0));
    assert_eq!(history_names(&h).last().unwrap(), "Feather Selection");
    press(&mut h, Modifiers::NONE, Key::Delete);
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(40, 40).a, 0, "fully erased in the middle");
    let rim = s.get(40, 21).a; // 1.5 px inside the edge: 1.5/8 erased
    assert!(rim > 200 && rim < 255, "soft edge, got alpha {rim}");
    assert_eq!(s.get(40, 19).a, 255, "outside untouched");
}

#[test]
fn toolbar_feather_applies_to_new_and_current_selection() {
    let mut h = harness();
    h.state_mut().settings_mut().feather = linear(4.0);
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (50.0, 50.0)],
    );
    assert_eq!(
        doc(&h).selection().feather(),
        linear(4.0),
        "new selections take the toolbar feather"
    );
    // Backspace fills with a soft edge.
    press(&mut h, Modifiers::NONE, Key::Backspace);
    let p = doc(&h).layer(0).surface.get(30, 11);
    assert!(p.r < 255 && p.r > 0, "partially filled rim: {p:?}");
}

#[test]
fn moving_feathered_pixels_carries_soft_alpha() {
    let mut s = Surface::new_transparent(100, 80);
    s.fill(ColorBgra::from_rgba(255, 0, 0, 255));
    let mut h = harness_with(Document::from_layers(
        100,
        80,
        vec![Layer::new("Background", s)],
    ));
    h.state_mut().settings_mut().feather = linear(8.0);
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(10.0, 10.0), (40.0, 40.0)],
    );
    press(&mut h, Modifiers::NONE, Key::M);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(25.0, 25.0), (75.0, 25.0)],
    );
    press(&mut h, Modifiers::NONE, Key::Enter);
    let s = &doc(&h).layer(0).surface;
    // The source rim was partly lifted, so a faint ring of red remains behind.
    let behind = s.get(25, 11).a;
    assert!(behind > 0 && behind < 255, "soft hole edge: {behind}");
    assert_eq!(s.get(25, 25).a, 0, "hole centre empty");
}

#[test]
fn feather_object_from_effects_menu() {
    let mut s = Surface::new_transparent(100, 80);
    for y in 20..60 {
        for x in 20..80 {
            s.set(x, y, ColorBgra::from_rgba(0, 0, 255, 255));
        }
    }
    let mut h = harness_with(Document::from_layers(
        100,
        80,
        vec![Layer::new("Layer 1", s)],
    ));
    let id = pnn_effects::EFFECTS
        .iter()
        .position(|e| e.name == "Feather Object")
        .unwrap() as u16;
    let ctx = h.ctx.clone();
    h.state_mut().execute(
        &ctx,
        paintnotnet::commands::Triggered::once(paintnotnet::commands::Command::Effect(
            pnn_effects::EffectId(id),
        )),
    );
    h.step();
    h.step();
    assert!(
        h.state_mut().doc_dialog_mut().is_some(),
        "Effects > Object > Feather Object opens its dialog"
    );
    h.get_by_label("OK").click();
    h.step();
    h.step();

    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(50, 40).a, 255);
    let rim = s.get(20, 40).a;
    assert!(rim > 0 && rim < 60, "feathered rim: {rim}");
    assert_eq!((s.get(20, 40).r, s.get(20, 40).b), (0, 255), "colour kept");
    assert_eq!(history_names(&h).last().unwrap(), "Feather Object");
}

#[test]
fn feather_undoes() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(20.0, 20.0), (60.0, 60.0)],
    );
    press(&mut h, Modifiers::SHIFT, Key::F6);
    h.get_by_label("OK").click();
    h.step();
    h.step();
    assert!(!doc(&h).selection().feather().is_none());
    press(&mut h, ctrl(), Key::Z);
    assert!(doc(&h).selection().feather().is_none());
}

#[test]
fn blur_selection_edge_fades_alpha_inside_only() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(20.0, 20.0), (60.0, 60.0)],
    );
    let id = pnn_effects::EFFECTS
        .iter()
        .position(|e| e.name == "Blur Selection Edge")
        .unwrap() as u16;
    let ctx = h.ctx.clone();
    h.state_mut().execute(
        &ctx,
        paintnotnet::commands::Triggered::once(paintnotnet::commands::Command::Effect(
            pnn_effects::EffectId(id),
        )),
    );
    h.step();
    h.step();
    {
        let d = h
            .state_mut()
            .doc_dialog_mut()
            .expect("Effects > Selection > Blur Selection Edge opens its dialog");
        let Kind::BlurSelectionEdge(f) = &mut d.kind else {
            panic!("wrong dialog")
        };
        *f = linear(8.0);
    }
    h.get_by_label("OK").click();
    h.step();
    h.step();
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(40, 40).a, 255, "centre kept");
    let rim = s.get(40, 21).a; // 1.5 px inside the edge: alpha 1.5/8
    assert!(rim > 20 && rim < 60, "faded edge, got alpha {rim}");
    assert_eq!(s.get(40, 19).a, 255, "outside untouched");
    assert_eq!(
        doc(&h).selection().feather(),
        Feather::NONE,
        "selection stays hard"
    );
    assert_eq!(history_names(&h).last().unwrap(), "Blur Selection Edge");
}
