//! Phase 9: Effects menu items through the effect framework.

use crate::common::*;
use eframe::egui::Key;
use egui_kittest::kittest::Queryable;
use paintnotnet::commands::{Command, Triggered};
use pnn_core::{ColorBgra, Document, Layer, Surface};

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

fn effect(name: &str) -> Command {
    let id = pnn_effects::EFFECTS
        .iter()
        .position(|e| e.name == name)
        .unwrap_or_else(|| panic!("{name} in the catalog"));
    Command::Effect(pnn_effects::EffectId(id as u16))
}

fn run(h: &mut H<'_>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Triggered::once(c));
    for _ in 0..6 {
        h.step();
    }
}

/// Left half black, right half white.
fn halves() -> Document {
    let mut s = Surface::new_transparent(100, 80);
    for y in 0..80 {
        for x in 0..100 {
            let v = if x < 50 { 0 } else { 255 };
            s.set(x, y, ColorBgra::from_rgba(v, v, v, 255));
        }
    }
    Document::from_layers(100, 80, vec![Layer::new("Background", s)])
}

#[test]
fn gaussian_blur_from_the_effects_menu() {
    let mut h = harness_with(halves());
    let c = effect("Gaussian Blur");
    assert!(h.state().command_state().enabled(c), "menu item enabled");
    run(&mut h, c);
    assert!(h.state_mut().effect_mut().is_some(), "dialog open");
    // Default radius 2 previews at once: the edge softens.
    let edge = doc(&h).layer(0).surface.get(49, 40).r;
    assert!(edge > 0 && edge < 128, "previewed: {edge}");
    h.get_by_label("OK").click();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(history_names(&h).last().unwrap(), "Gaussian Blur");
    // Ctrl+F repeats it: the edge softens further.
    press(&mut h, ctrl(), Key::F);
    let again = doc(&h).layer(0).surface.get(48, 40).r;
    assert!(again > 0, "repeated: {again}");
    assert_eq!(history_names(&h).last().unwrap(), "Gaussian Blur");
}

#[test]
fn every_implemented_effect_opens_previews_and_cancels() {
    let names: Vec<&str> = pnn_effects::EFFECTS
        .iter()
        .enumerate()
        .filter(|(i, _)| pnn_effects::effect_def(pnn_effects::EffectId(*i as u16)).is_some())
        .map(|(_, e)| e.name)
        .collect();
    assert!(names.len() >= 28, "{names:?}");
    for name in names {
        let mut h = harness_with(halves());
        let n = history_names(&h).len();
        run(&mut h, effect(name));
        assert!(h.state_mut().effect_mut().is_some(), "{name} dialog");
        h.get_by_label("Cancel").click();
        for _ in 0..3 {
            h.step();
        }
        assert_eq!(
            doc(&h).layer(0).surface.to_pixels(),
            halves().layer(0).surface.to_pixels(),
            "{name}: cancel restores"
        );
        assert_eq!(history_names(&h).len(), n);
    }
}
