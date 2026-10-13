//! Headless UI tests for Phase 1: image tabs, view shortcuts and the close flow, driven
//! through real key events.

use eframe::egui::{Event, Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::dialogs::Modal;
use pnn_core::Document;

fn harness() -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.step();
    h
}

fn ctrl() -> Modifiers {
    Modifiers::CTRL | Modifiers::COMMAND
}

/// Press and release `key` with `modifiers`, stepping one frame per queued event.
fn press(h: &mut Harness<'_, PaintNotNetApp>, modifiers: Modifiers, key: Key) {
    let k = |pressed| Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers,
    };
    let events = [
        Event::ModifiersChanged(modifiers),
        k(true),
        k(false),
        Event::ModifiersChanged(Modifiers::NONE),
    ];
    let n = events.len();
    for e in events {
        h.event(e);
    }
    for _ in 0..n {
        h.step();
    }
}

fn add_docs(h: &mut Harness<'_, PaintNotNetApp>, sizes: &[(u32, u32)]) -> Vec<u64> {
    let keys = sizes
        .iter()
        .map(|&(w, h_)| {
            h.state_mut()
                .workspace_mut()
                .add(Document::new_blank(w, h_), None, None)
        })
        .collect();
    h.step();
    h.step();
    keys
}

fn active_key(h: &Harness<'_, PaintNotNetApp>) -> u64 {
    h.state().workspace().active().unwrap().key
}

fn zoom(h: &Harness<'_, PaintNotNetApp>) -> f64 {
    h.state().workspace().active().unwrap().view.unwrap().zoom
}

#[test]
fn ctrl_n_then_enter_creates_800x600_white_image() {
    let mut h = harness();
    press(&mut h, ctrl(), Key::N);
    assert!(matches!(h.state().modal(), Some(Modal::NewImage(_))));
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(h.state().modal().is_none());
    let tab = h.state().workspace().active().expect("new image");
    assert_eq!((tab.doc.width(), tab.doc.height()), (800, 600));
    assert_eq!(tab.title(), "Untitled");
}

#[test]
fn ctrl_tab_cycles_and_ctrl_shift_pgdn_moves() {
    let mut h = harness();
    let keys = add_docs(&mut h, &[(10, 10), (20, 20), (30, 30)]);
    press(&mut h, ctrl(), Key::Tab);
    assert_eq!(active_key(&h), keys[0], "wraps to the first image");
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::Tab);
    assert_eq!(active_key(&h), keys[2]);
    press(&mut h, ctrl(), Key::Num2);
    assert_eq!(active_key(&h), keys[1]);
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::PageDown);
    assert_eq!(h.state().workspace().tabs()[2].key, keys[1], "moved right");
    assert_eq!(active_key(&h), keys[1]);
}

#[test]
fn zoom_shortcuts_follow_paint_net_presets() {
    let mut h = harness();
    add_docs(&mut h, &[(4000, 3000)]);
    let fitted = zoom(&h);
    assert!(fitted < 1.0, "large image opens fitted to the window");
    press(&mut h, ctrl(), Key::Num0);
    assert_eq!(zoom(&h), 1.0);
    press(&mut h, ctrl(), Key::Equals);
    assert_eq!(zoom(&h), 2.0);
    press(&mut h, ctrl(), Key::Minus);
    press(&mut h, ctrl(), Key::Minus);
    assert_eq!(zoom(&h), 0.66);
    // Ctrl+B fits; a second Ctrl+B restores 66%.
    press(&mut h, ctrl(), Key::B);
    assert!((zoom(&h) - fitted).abs() < 1e-9);
    press(&mut h, ctrl(), Key::B);
    assert_eq!(zoom(&h), 0.66);
}

#[test]
fn home_twice_scrolls_to_top_left() {
    let mut h = harness();
    add_docs(&mut h, &[(5000, 5000)]);
    press(&mut h, ctrl(), Key::Num0);
    press(&mut h, Modifiers::SHIFT, Key::End);
    let view = h.state().workspace().active().unwrap().view.unwrap();
    assert!(
        view.center[0] > 4000.0 && view.center[1] > 4000.0,
        "Shift+End: bottom-right"
    );
    press(&mut h, Modifiers::NONE, Key::Home);
    press(&mut h, Modifiers::NONE, Key::Home);
    let view = h.state().workspace().active().unwrap().view.unwrap();
    assert!(
        view.center[0] < 1000.0 && view.center[1] < 1000.0,
        "Home ×2: top-left, got {:?}",
        view.center
    );
}

#[test]
fn closing_dirty_image_asks_and_dont_save_discards() {
    let mut h = harness();
    let keys = add_docs(&mut h, &[(10, 10), (10, 10)]);
    h.state_mut()
        .workspace_mut()
        .active_mut()
        .unwrap()
        .doc
        .invalidate_all();
    press(&mut h, ctrl(), Key::W);
    assert!(matches!(
        h.state().modal(),
        Some(Modal::UnsavedChanges { .. })
    ));
    h.get_by_label("Don't Save").click();
    h.step();
    h.step();
    assert!(h.state().modal().is_none());
    assert_eq!(h.state().workspace().tabs().len(), 1);
    assert_eq!(active_key(&h), keys[0]);
}

#[test]
fn closing_clean_image_needs_no_prompt() {
    let mut h = harness();
    add_docs(&mut h, &[(10, 10)]);
    press(&mut h, ctrl(), Key::F4);
    assert!(h.state().modal().is_none());
    assert!(h.state().workspace().tabs().is_empty());
}

#[test]
fn pixel_grid_toggles_from_view_menu() {
    let mut h = harness();
    add_docs(&mut h, &[(10, 10)]);
    h.get_by_label("View").click();
    h.step();
    h.step();
    h.get_by_label("Pixel Grid").click();
    h.step();
    h.step();
    assert!(h.state().pixel_grid());
}

#[test]
fn hovering_another_menu_title_switches_menus() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    add_docs(&mut h, &[(10, 10)]);
    h.get_by_label("File").click();
    h.step();
    h.step();
    assert!(h.query_by_label_contains("New...").is_some());
    // Only moving the pointer, no click.
    let edit = h.get_by_label("Edit").rect().center();
    h.event(eframe::egui::Event::PointerMoved(edit));
    h.step();
    h.step();
    h.step();
    assert!(
        h.query_by_label_contains("Undo").is_some(),
        "Edit menu didn't open"
    );
    assert!(
        h.query_by_label_contains("New...").is_none(),
        "File menu still open"
    );
    // With no menu open, hovering a title does nothing.
    h.key_press(eframe::egui::Key::Escape);
    h.step();
    h.step();
    let view = h.get_by_label("View").rect().center();
    h.event(eframe::egui::Event::PointerMoved(view));
    h.step();
    h.step();
    assert!(h.query_by_label("Pixel Grid").is_none());
}
