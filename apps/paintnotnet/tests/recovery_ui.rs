//! Autosave and crash recovery.

mod common;

use common::*;
use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use pnn_core::{ColorBgra, Document, Layer, Surface};

#[test]
fn autosave_writes_unsaved_changes() {
    let state = tempfile::tempdir().unwrap();
    let mut h = harness();
    h.state_mut()
        .enable_autosave(state.path().to_owned(), std::time::Duration::ZERO);
    // Make the document dirty (Invert Colors).
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::I);
    let session = state
        .path()
        .join("recovery")
        .join(std::process::id().to_string());
    let mut found = false;
    for _ in 0..200 {
        h.step();
        found = std::fs::read_dir(&session).is_ok_and(|d| {
            d.flatten()
                .any(|f| f.path().extension().and_then(|e| e.to_str()) == Some("ora"))
        });
        if found {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(found, "an .ora autosave appeared in {}", session.display());
}

#[test]
fn a_crashed_sessions_images_are_offered_back() {
    let state = tempfile::tempdir().unwrap();
    let dead = state.path().join("recovery").join("4000000000");
    std::fs::create_dir_all(&dead).unwrap();
    let mut s = Surface::new_transparent(20, 10);
    s.set(2, 2, ColorBgra::from_rgba(0, 255, 0, 255));
    let doc = Document::from_layers(20, 10, vec![Layer::new("Background", s)]);
    std::fs::write(dead.join("3.ora"), pnn_io::ora::encode(&doc).unwrap()).unwrap();
    std::fs::write(
        dead.join("3.txt"),
        "holiday.png\n/home/someone/holiday.png\n",
    )
    .unwrap();

    let mut h = harness();
    let before = h.state().workspace().tabs().len();
    h.state_mut()
        .enable_autosave(state.path().to_owned(), std::time::Duration::from_secs(60));
    h.step();
    h.step();
    h.get_by_label("Recover images");
    h.get_by_label("Recover").click();
    h.step();
    h.step();
    let ws = h.state().workspace();
    assert_eq!(ws.tabs().len(), before + 1);
    let tab = ws.active().unwrap();
    assert_eq!(tab.title(), "holiday.png");
    assert!(tab.is_dirty(), "recovered work is unsaved");
    assert_eq!(
        tab.doc.layer(0).surface.get(2, 2),
        ColorBgra::from_rgba(0, 255, 0, 255)
    );
    assert!(!dead.exists(), "recovery files cleared");
}
