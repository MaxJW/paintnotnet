//! Helpers shared by the UI tests: drive the real app with key and pointer events.
#![allow(dead_code)]

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Panel, Triggered};
use pnn_core::Document;
use pnn_render::View;

/// App with one document open, diagnostics and floating windows hidden (so clicks land on
/// the canvas), zoomed to 400% centred on the image.
pub fn harness_with(doc: Document) -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    let (w, hgt) = (doc.width() as f64, doc.height() as f64);
    h.state_mut().workspace_mut().add(doc, None, None);
    let ctx = h.ctx.clone();
    if h.state().diagnostics().visible() {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
    }
    for p in Panel::ALL {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleWindow(p)));
    }
    h.step();
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.view = Some(View {
        zoom: 4.0,
        center: [w / 2.0, hgt / 2.0],
        fit_to_window: false,
    });
    h.step();
    h.step();
    h
}

pub fn harness() -> Harness<'static, PaintNotNetApp> {
    harness_with(Document::new_blank(100, 80))
}

pub fn ctrl() -> Modifiers {
    Modifiers::CTRL | Modifiers::COMMAND
}

pub fn press(h: &mut Harness<'_, PaintNotNetApp>, modifiers: Modifiers, key: Key) {
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

pub fn doc<'a>(h: &'a Harness<'_, PaintNotNetApp>) -> &'a Document {
    &h.state().workspace().active().unwrap().doc
}

pub fn history_names(h: &Harness<'_, PaintNotNetApp>) -> Vec<String> {
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

/// Screen position of image point (x, y) (pixel corners at integers).
pub fn screen(h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64) -> Pos2 {
    let rect = h.state().canvas().rect.unwrap();
    let view = h.state().workspace().active().unwrap().view.unwrap();
    let vp = [rect.width() as f64, rect.height() as f64];
    let s = view.image_to_screen([x, y], vp);
    Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
}

/// Press at the first point, move through the rest, release at the last, holding
/// `modifiers` throughout. Points are image coordinates.
pub fn drag(
    h: &mut Harness<'_, PaintNotNetApp>,
    button: PointerButton,
    modifiers: Modifiers,
    points: &[(f64, f64)],
) {
    h.event(Event::ModifiersChanged(modifiers));
    let first = screen(h, points[0].0, points[0].1);
    h.event(Event::PointerMoved(first));
    h.step();
    h.event(Event::PointerButton {
        pos: first,
        button,
        pressed: true,
        modifiers,
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
        modifiers,
    });
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
    h.step();
}

/// Centre of image pixel (x, y).
pub fn px(x: u32, y: u32) -> (f64, f64) {
    (x as f64 + 0.5, y as f64 + 0.5)
}
