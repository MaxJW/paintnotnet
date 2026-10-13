//! OpenRaster: layered save without flattening, and opening it back.

mod common;

use common::*;
use eframe::egui::Key;
use pnn_core::{BlendMode, ColorBgra, Document, Layer, Surface};
use pnn_io::FileFormat;

#[test]
fn ctrl_s_saves_layers_to_ora_without_flattening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layers.ora");
    let mut a = Surface::new_transparent(40, 30);
    a.set(3, 3, ColorBgra::from_rgba(255, 0, 0, 255));
    let mut top = Layer::new("Top", Surface::new_transparent(40, 30));
    top.blend_mode = BlendMode::Screen;
    let doc = Document::from_layers(40, 30, vec![Layer::new("Background", a), top]);
    let mut h = harness();
    h.state_mut()
        .workspace_mut()
        .add(doc, Some(path.clone()), Some(FileFormat::Ora));
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
    let back = pnn_io::load(&path).expect("ORA loads");
    assert_eq!(back.layers().len(), 2);
    assert_eq!(back.layer(1).blend_mode, BlendMode::Screen);
    assert_eq!(
        back.layer(0).surface.get(3, 3),
        ColorBgra::from_rgba(255, 0, 0, 255)
    );
}
