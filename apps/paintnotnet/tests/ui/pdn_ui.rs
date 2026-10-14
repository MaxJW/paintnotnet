//! paint.net images: Ctrl+S keeps the layers in a .pdn, and the file opens back as layers.

use crate::common::*;
use eframe::egui::Key;
use pnn_core::{BlendMode, ColorBgra, Document, Layer, Surface};
use pnn_io::FileFormat;

#[test]
fn ctrl_s_saves_layers_to_pdn_without_flattening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layers.pdn");
    let mut a = Surface::new_transparent(40, 30);
    a.set(3, 3, ColorBgra::from_rgba(255, 0, 0, 255));
    let mut top = Layer::new("Top", Surface::new_transparent(40, 30));
    top.blend_mode = BlendMode::Overlay;
    top.opacity = 99;
    top.visible = false;
    let doc = Document::from_layers(40, 30, vec![Layer::new("Background", a), top]);
    let mut h = harness();
    h.state_mut()
        .workspace_mut()
        .add(doc, Some(path.clone()), Some(FileFormat::Pdn));
    h.step();
    press(&mut h, ctrl(), Key::S);
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
    assert_eq!(
        h.state().workspace().active().unwrap().doc.layers().len(),
        2,
        "not flattened"
    );
    assert!(std::fs::read(&path).unwrap().starts_with(b"PDN3"));
    let back = pnn_io::load(&path).expect("PDN loads");
    assert_eq!(back.layers().len(), 2);
    let top = back.layer(1);
    assert_eq!(
        (top.name.as_str(), top.blend_mode, top.opacity, top.visible),
        ("Top", BlendMode::Overlay, 99, false)
    );
    assert_eq!(
        back.layer(0).surface.get(3, 3),
        ColorBgra::from_rgba(255, 0, 0, 255)
    );
    assert_eq!(FileFormat::from_path(&path), Some(FileFormat::Pdn));
}
