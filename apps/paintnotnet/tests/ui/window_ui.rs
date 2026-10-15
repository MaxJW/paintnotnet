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

/// Hovering the window's edges shows the resize cursors before any click.
#[test]
fn window_edges_show_resize_cursors_on_hover() {
    use eframe::egui::{CursorIcon, Event, Pos2};
    let doc = pnn_core::Document::new_blank(800, 600);
    let mut h = harness_with(doc);
    h.step();
    let r = h.ctx.content_rect();
    for (p, want) in [
        (
            Pos2::new(r.min.x + 1.0, r.center().y),
            CursorIcon::ResizeWest,
        ),
        (
            Pos2::new(r.max.x - 1.0, r.center().y),
            CursorIcon::ResizeEast,
        ),
        (
            Pos2::new(r.center().x, r.max.y - 1.0),
            CursorIcon::ResizeSouth,
        ),
        (
            Pos2::new(r.center().x, r.min.y + 1.0),
            CursorIcon::ResizeNorth,
        ),
        (
            Pos2::new(r.max.x - 2.0, r.max.y - 2.0),
            CursorIcon::ResizeSouthEast,
        ),
        (
            Pos2::new(r.min.x + 2.0, r.min.y + 2.0),
            CursorIcon::ResizeNorthWest,
        ),
    ] {
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
        h.step();
        assert_eq!(h.output().platform_output.cursor_icon, want, "at {p:?}");
    }
}

/// A plain click on another image's tab switches to it (press and release, no movement);
/// dragging a tab onto another reorders them.
#[test]
fn clicking_a_tab_switches_to_it_and_dragging_reorders() {
    use eframe::egui::{Event, Modifiers, PointerButton};
    let mut h = harness();
    for (w, name) in [(10, "a.png"), (20, "b.png")] {
        let doc = pnn_core::Document::new_blank(w, w);
        h.state_mut()
            .workspace_mut()
            .add(doc, Some(std::path::PathBuf::from(name)), None);
    }
    h.step();
    h.step();
    let active = |h: &egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>| {
        h.state().workspace().active().map(|t| t.title())
    };
    assert_eq!(active(&h).as_deref(), Some("b.png"));
    let a = h.get_by_label("a.png").rect().center();
    let press =
        |h: &mut egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>, p, pressed| {
            h.input_mut().events.push(Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        };
    h.input_mut().events.push(Event::PointerMoved(a));
    h.step();
    press(&mut h, a, true);
    press(&mut h, a, false);
    h.step();
    assert_eq!(active(&h).as_deref(), Some("a.png"), "switched");

    // Drag a onto the tab before it (Untitled): a comes first. (In the test's narrow window
    // b is scrolled out of view.)
    let u = h.get_by_label("Untitled").rect().center();
    h.input_mut().events.push(Event::PointerMoved(a));
    h.step();
    press(&mut h, a, true);
    for k in 1..=10 {
        let p = a + (u - a) * (k as f32 / 10.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
    }
    assert!(
        eframe::egui::DragAndDrop::has_any_payload(&h.ctx),
        "dragging the tab, not the window"
    );
    press(&mut h, u, false);
    h.step();
    let names: Vec<String> = h
        .state()
        .workspace()
        .tabs()
        .iter()
        .map(|t| t.title())
        .collect();
    let (ia, iu) = (
        names.iter().position(|n| n == "a.png").unwrap(),
        names.iter().position(|n| n == "Untitled").unwrap(),
    );
    assert!(ia < iu, "{names:?}");
}

/// A tab that fits between the menus and the search box is shown whole, centred.
#[test]
fn a_tab_that_fits_is_not_cut_off() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    let key = h.state().workspace().active().unwrap().key;
    h.state_mut().workspace_mut().active_mut().unwrap().path =
        Some(std::path::PathBuf::from("pexels-5965592.jpg"));
    for _ in 0..4 {
        h.step();
    }
    let r = ctx
        .read_response(eframe::egui::Id::new(("image_list_item", key)))
        .unwrap();
    eprintln!("rect {:?} interact {:?}", r.rect, r.interact_rect);
    assert_eq!(r.rect, r.interact_rect, "not clipped");
}
