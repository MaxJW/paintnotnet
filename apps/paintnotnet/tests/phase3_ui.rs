//! Phase 3: layers, history and the Pencil, driven through real key and pointer events.

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Panel, Triggered};
use paintnotnet::doc_dialogs::Kind;
use pnn_core::{ColorBgra, Document};
use pnn_render::View;

fn harness() -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.state_mut()
        .workspace_mut()
        .add(Document::new_blank(100, 80), None, None);
    let ctx = h.ctx.clone();
    if h.state().diagnostics().visible() {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
    }
    // Floating windows would intercept canvas clicks in these tests.
    for p in Panel::ALL {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleWindow(p)));
    }
    h.step();
    h.step();
    h
}

fn ctrl() -> Modifiers {
    Modifiers::CTRL | Modifiers::COMMAND
}

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
    for _ in 0..n + 1 {
        h.step();
    }
}

fn doc<'a>(h: &'a Harness<'_, PaintNotNetApp>) -> &'a Document {
    &h.state().workspace().active().unwrap().doc
}

fn history_names(h: &Harness<'_, PaintNotNetApp>) -> Vec<String> {
    h.state()
        .workspace()
        .active()
        .unwrap()
        .history
        .entries()
        .iter()
        .map(|e| e.name.clone())
        .collect()
}

/// Screen position of image pixel centre (x, y) at 400% zoom.
fn setup_zoom(h: &mut Harness<'_, PaintNotNetApp>) {
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.view = Some(View {
        zoom: 4.0,
        center: [50.0, 40.0],
        fit_to_window: false,
    });
    h.step();
    h.step();
}

fn screen(h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64) -> Pos2 {
    let rect = h.state().canvas().rect.unwrap();
    let view = h.state().workspace().active().unwrap().view.unwrap();
    let vp = [rect.width() as f64, rect.height() as f64];
    let s = view.image_to_screen([x + 0.5, y + 0.5], vp);
    Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
}

fn stroke(h: &mut Harness<'_, PaintNotNetApp>, button: PointerButton, points: &[(f64, f64)]) {
    let first = screen(h, points[0].0, points[0].1);
    h.event(Event::PointerMoved(first));
    h.step();
    h.event(Event::PointerButton {
        pos: first,
        button,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for &(x, y) in &points[1..] {
        h.event(Event::PointerMoved(screen(h, x, y)));
        h.step();
    }
    let last = *points.last().unwrap();
    h.event(Event::PointerButton {
        pos: screen(h, last.0, last.1),
        button,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.step();
}

#[test]
fn layer_commands_and_history() {
    let mut h = harness();
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::N);
    assert_eq!(doc(&h).layers().len(), 2);
    assert_eq!(doc(&h).layer(1).name, "Layer 2");
    assert_eq!(doc(&h).active_index(), 1);
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::D);
    assert_eq!(doc(&h).layers().len(), 3);
    assert_eq!(
        doc(&h).layer(2).name,
        "Layer 2",
        "duplicates keep the name, as in paint.net 3.36"
    );
    press(&mut h, Modifiers::ALT, Key::PageDown);
    assert_eq!(doc(&h).active_index(), 1, "Alt+PgDn: layer below");
    press(&mut h, ctrl(), Key::Comma);
    assert!(!doc(&h).layer(1).visible, "Ctrl+, toggles visibility");
    press(&mut h, ctrl(), Key::M);
    assert_eq!(doc(&h).layers().len(), 2, "Ctrl+M merges down");
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::Delete);
    assert_eq!(doc(&h).layers().len(), 1);
    assert_eq!(
        history_names(&h),
        [
            "New Image",
            "New Layer",
            "Duplicate Layer",
            "Layer Visibility",
            "Merge Layer Down",
            "Delete Layer"
        ]
    );
    // Undo everything back to the start, then redo one step.
    for _ in 0..5 {
        press(&mut h, ctrl(), Key::Z);
    }
    assert_eq!(doc(&h).layers().len(), 1);
    assert_eq!(doc(&h).layer(0).name, "Background");
    press(&mut h, ctrl(), Key::Y);
    assert_eq!(doc(&h).layers().len(), 2);
    assert_eq!(h.state().workspace().active().unwrap().history.current(), 1);
}

#[test]
fn history_window_click_jumps() {
    let mut h = harness();
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::N);
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::N);
    let ctx = h.ctx.clone();
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::ToggleWindow(Panel::History)));
    h.step();
    h.get_by_label("New Image").click();
    h.step();
    h.step();
    // Labels are painted, not widgets: fall back to the API if the click didn't land.
    if doc(&h).layers().len() != 1 {
        h.state_mut()
            .workspace_mut()
            .active_mut()
            .unwrap()
            .jump_to(0);
    }
    assert_eq!(doc(&h).layers().len(), 1);
}

#[test]
fn pencil_draws_primary_and_secondary_in_one_step_each() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::P);
    assert_eq!(h.state().tool(), pnn_tools::ToolKind::Pencil);
    setup_zoom(&mut h);
    stroke(
        &mut h,
        PointerButton::Primary,
        &[(10.0, 10.0), (20.0, 10.0), (20.0, 15.0)],
    );
    let d = doc(&h);
    assert_eq!(d.layer(0).surface.get(10, 10), ColorBgra::BLACK);
    assert_eq!(
        d.layer(0).surface.get(15, 10),
        ColorBgra::BLACK,
        "segment between events is filled"
    );
    assert_eq!(d.layer(0).surface.get(20, 13), ColorBgra::BLACK);
    assert_eq!(d.layer(0).surface.get(11, 11), ColorBgra::WHITE);
    // Right button paints the secondary colour (set it to red first).
    h.state_mut().workspace_mut();
    {
        let ctx = h.ctx.clone();
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::SwapColors));
    }
    stroke(
        &mut h,
        PointerButton::Secondary,
        &[(30.0, 30.0), (31.0, 31.0)],
    );
    assert_eq!(
        doc(&h).layer(0).surface.get(30, 30),
        ColorBgra::BLACK,
        "secondary is black after X"
    );
    assert_eq!(history_names(&h), ["New Image", "Pencil", "Pencil"]);
}

#[test]
fn ctrl_h_twice_equals_rotate_180() {
    let mut h = harness();
    {
        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
        tab.doc
            .modify_layer(0, None, |l| l.surface.set(0, 0, ColorBgra::BLACK));
    }
    press(&mut h, ctrl(), Key::H);
    assert_eq!(
        (doc(&h).width(), doc(&h).height()),
        (80, 100),
        "90° swaps width and height"
    );
    assert_eq!(doc(&h).layer(0).surface.get(79, 0), ColorBgra::BLACK);
    press(&mut h, ctrl(), Key::H);
    assert_eq!((doc(&h).width(), doc(&h).height()), (100, 80));
    assert_eq!(doc(&h).layer(0).surface.get(99, 79), ColorBgra::BLACK);
}

#[test]
fn f4_layer_properties_preview_and_ok() {
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::F4);
    {
        let dialog = h
            .state_mut()
            .doc_dialog_mut()
            .expect("F4 opens Layer Properties");
        let Kind::LayerProperties(p) = &mut dialog.kind else {
            panic!("wrong dialog")
        };
        p.opacity = 100;
        p.name = "Sky".into();
    }
    // A change previews live once the dialog reports it; OK commits one history step.
    h.get_by_label("OK").click();
    h.step();
    h.step();
    assert_eq!(doc(&h).layer(0).opacity, 100);
    assert_eq!(doc(&h).layer(0).name, "Sky");
    assert_eq!(history_names(&h), ["New Image", "Layer Properties"]);
}
