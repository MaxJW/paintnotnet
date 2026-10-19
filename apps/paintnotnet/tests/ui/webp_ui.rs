//! WebP saving through the Save Configuration dialog.

use crate::common::*;
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
    for label in ["Preset", "Quality", "Effort", "Lossless"] {
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

/// One save options dialog for every type: its File type dropdown changes the format (and
/// the extension), the settings follow the type, and the flatten question comes after.
#[test]
fn save_options_file_type_switches_the_format() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("picture.jpg");
    let mut s = Surface::new_transparent(40, 30);
    s.fill(ColorBgra::from_rgba(30, 140, 200, 255));
    let doc = Document::from_layers(
        40,
        30,
        vec![
            Layer::new("Background", s),
            Layer::new("Layer 2", Surface::new_transparent(40, 30)),
        ],
    );
    let mut h = harness();
    h.state_mut()
        .workspace_mut()
        .add(doc, Some(jpg.clone()), Some(FileFormat::Jpeg));
    h.step();
    press(&mut h, ctrl(), Key::S);
    h.get_by_label("Quality");
    // The dropdown's selected type is its accessible value.
    h.get_by(|n| n.value().as_deref() == Some("JPEG (.jpg)"))
        .click();
    h.step();
    h.get_by_label("PNG (.png)").click();
    h.step();
    h.step();
    assert!(h.query_by_label("Quality").is_none(), "PNG has no quality");
    h.get_by_label("OK").click();
    // Two layers, a flat format: asked now, after the type (a new window takes a frame to
    // size itself before it takes clicks).
    for _ in 0..4 {
        h.step();
    }
    h.get_by_label("Flatten").click();
    let png = dir.path().join("picture.png");
    for _ in 0..200 {
        h.step();
        if png.exists()
            && h.state()
                .workspace()
                .active()
                .is_some_and(|t| !t.is_dirty())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(png.exists() && !jpg.exists(), "saved as PNG");
    let tab = h.state().workspace().active().unwrap();
    assert_eq!(tab.path.as_deref(), Some(png.as_path()));
    assert_eq!(tab.format, Some(FileFormat::Png));
    assert_eq!(
        pnn_io::load(&png).unwrap().layer(0).surface.get(5, 5),
        ColorBgra::from_rgba(30, 140, 200, 255)
    );
}
