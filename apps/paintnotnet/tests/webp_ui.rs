//! WebP saving through the Save Configuration dialog.

mod common;

use common::*;
use eframe::egui::Key;
use egui_kittest::kittest::Queryable;
use pnn_core::{ColorBgra, Document, Layer, Surface};
use pnn_io::FileFormat;

#[test]
fn ctrl_s_on_a_webp_asks_for_settings_then_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("picture.webp");
    let mut s = Surface::new_transparent(40, 30);
    for y in 0..30 {
        for x in 0..40 {
            s.set(
                x,
                y,
                ColorBgra::from_rgba((x * 6) as u8, (y * 8) as u8, 90, 200),
            );
        }
    }
    let doc = Document::from_layers(40, 30, vec![Layer::new("Background", s)]);
    let mut h = harness();
    h.state_mut()
        .workspace_mut()
        .add(doc.clone(), Some(path.clone()), Some(FileFormat::WebP));
    h.step();
    press(&mut h, ctrl(), Key::S);
    // pdn-webp's settings.
    for label in ["Preset:", "Quality:", "Effort:", "Lossless"] {
        h.get_by_label(label);
    }
    h.get_by_label("Lossless").click();
    h.step();
    h.get_by_label("OK").click();
    for _ in 0..200 {
        h.step();
        if path.exists()
            && h.state()
                .workspace()
                .active()
                .is_some_and(|t| !t.is_dirty())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let back = pnn_io::load(&path).expect("saved WebP loads");
    assert_eq!(
        back.layer(0).surface.to_pixels(),
        doc.layer(0).surface.to_pixels(),
        "lossless WebP round-trips exactly"
    );
}
