//! paintnotnet's own window buttons (the desktop's title bar is off by default).

use crate::common::*;
use eframe::egui::ViewportCommand;
use egui_kittest::kittest::Queryable;

fn sent(h: &egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>) -> Vec<ViewportCommand> {
    h.output()
        .viewport_output
        .values()
        .flat_map(|v| v.commands.clone())
        .collect()
}

#[test]
fn window_buttons_minimize_maximize_and_close() {
    let mut h = harness();
    h.step();
    for (name, want) in [
        ("Minimize", ViewportCommand::Minimized(true)),
        ("Maximize", ViewportCommand::Maximized(true)),
        ("Close", ViewportCommand::Close),
    ] {
        h.get_by_label(name).click();
        h.step();
        assert!(sent(&h).contains(&want), "{name}: {:?}", sent(&h));
    }
}
