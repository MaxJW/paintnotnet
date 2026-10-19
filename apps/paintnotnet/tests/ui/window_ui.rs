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

/// Dragging the title bar hands the move to the desktop, which keeps the button release:
/// afterwards the wheel over the canvas still zooms (it didn't until another click).
#[test]
fn wheel_zooms_after_moving_the_window() {
    use eframe::egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, vec2};
    let mut h = harness();
    h.step();
    // Empty title bar: just left of the command search box.
    let search = h.get_by_label_contains("Search commands").rect();
    let bar = Pos2::new(search.min.x - 30.0, search.center().y);
    h.event(Event::PointerMoved(bar));
    h.step();
    h.event(Event::PointerButton {
        pos: bar,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    let mut commands = Vec::new();
    for k in 1..=4 {
        h.event(Event::PointerMoved(bar + vec2(10.0 * k as f32, 0.0)));
        h.step();
        commands.extend(sent(&h));
    }
    assert!(
        commands.contains(&ViewportCommand::StartDrag),
        "the move went to the desktop"
    );
    // The desktop moves the window and swallows the release. eframe runs the app's
    // `raw_input_hook` before each frame (egui_kittest doesn't): the release it adds is fed
    // here as eframe would.
    let release = paintnotnet::shell::take_release(&h.ctx).expect("a release is owed");
    h.event(release);
    h.step();
    assert!(
        paintnotnet::shell::take_release(&h.ctx).is_none(),
        "only once"
    );
    // The pointer comes back over the canvas and the wheel turns.
    let canvas = h.state().canvas().rect.unwrap().center();
    h.event(Event::PointerMoved(canvas));
    h.step();
    let before = h.state().workspace().active().unwrap().view.unwrap().zoom;
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, 1.0),
        modifiers: Modifiers::NONE,
        phase: eframe::egui::TouchPhase::Move,
    });
    h.step();
    h.step();
    let after = h.state().workspace().active().unwrap().view.unwrap().zoom;
    assert!(after > before, "zoomed: {before} -> {after}");
}

/// Invert Selection keeps the mask (inverted) instead of rasterising again: it agrees with
/// rasterising the inverted outline.
#[test]
fn invert_selection_keeps_an_exact_mask() {
    use paintnotnet::commands::{Command, Triggered};
    let mut h = harness();
    let ctx = h.ctx.clone();
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.doc
        .set_selection(pnn_core::Selection::ellipse(10.3, 7.7, 71.2, 55.9));
    let _ = tab.selection_mask(true);
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::InvertSelection));
    h.step();
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    let (w, hh) = (tab.doc.width(), tab.doc.height());
    let kept = tab.selection_mask(true).unwrap().to_pixels();
    let fresh = tab.doc.selection().rasterize_hard(w, hh, true).to_pixels();
    let worst = kept
        .iter()
        .zip(&fresh)
        .map(|(a, b)| (*a as i32 - *b as i32).abs())
        .max()
        .unwrap();
    assert!(worst <= 1, "differs by up to {worst}");
    assert_eq!(kept[0], 255, "outside the ellipse is now selected");
}

/// Move Selected Pixels with the GPU drawing the drag gives the same document as the CPU
/// path, and the history step is recorded once, on release.
#[test]
fn gpu_drawn_pixel_move_matches_the_cpu() {
    use eframe::egui::PointerButton;
    gpu_and_cpu_pixel_moves_agree(600, 400, PointerButton::Primary);
    // Larger than a megapixel: the GPU path draws the result into the document in the
    // background. Right button: a rotation.
    gpu_and_cpu_pixel_moves_agree(1300, 900, PointerButton::Secondary);
}

fn gpu_and_cpu_pixel_moves_agree(w: u32, height: u32, drag_button: eframe::egui::PointerButton) {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{Event, Key, Modifiers};
    use pnn_core::{ColorBgra, Document, Layer, Selection, Surface};
    let mut s = Surface::new_transparent(w, height);
    for y in 0..height {
        for x in 0..w {
            let v = ((x * 7 + y * 13) % 251) as u8;
            s.set(x, y, ColorBgra::from_rgba(v, 255 - v, (x % 200) as u8, 255));
        }
    }
    let mut d = Document::from_layers(w, height, vec![Layer::new("Background", s)]);
    d.set_selection(Selection::ellipse(100.0, 80.0, 380.0, 300.0));
    // One harness (one layout, so the same pointer positions in the image), the same drag
    // twice: drawn by the GPU, undone, then drawn on the CPU.
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .wgpu()
        .build_eframe(|cc| paintnotnet::app::PaintNotNetApp::new(cc, None, Vec::new()));
    h.state_mut().workspace_mut().add(d, None, None);
    h.step();
    h.step();
    h.state_mut()
        .set_tool(pnn_tools::ToolKind::MoveSelectedPixels);
    for _ in 0..3 {
        h.step();
    }
    let original = h
        .state()
        .workspace()
        .active()
        .unwrap()
        .doc
        .layer(0)
        .surface
        .to_pixels();
    let drag = |h: &mut egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>, gpu: bool| {
        h.state_mut().set_cpu_floating(!gpu);
        h.step();
        let rect = h.state().canvas().rect.unwrap();
        let view = h.state().workspace().active().unwrap().view.unwrap();
        let ppp = h.ctx.pixels_per_point() as f64;
        let vp = [rect.width() as f64 * ppp, rect.height() as f64 * ppp];
        let at = |x: f64, y: f64| {
            let s = view.image_to_screen([x, y], vp);
            eframe::egui::pos2(
                rect.min.x + (s[0] / ppp) as f32,
                rect.min.y + (s[1] / ppp) as f32,
            )
        };
        let button = |pressed, pos| Event::PointerButton {
            pos,
            button: drag_button,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.event(Event::PointerMoved(at(240.0, 190.0)));
        h.step();
        h.event(button(true, at(240.0, 190.0)));
        h.step();
        for k in 1..=10 {
            h.event(Event::PointerMoved(at(
                240.0 + 17.0 * k as f64,
                190.0 - 9.0 * k as f64,
            )));
            h.step();
        }
        let on_gpu = h
            .state()
            .workspace()
            .active()
            .unwrap()
            .gpu
            .floating
            .is_some();
        h.event(button(false, at(410.0, 100.0)));
        for _ in 0..4 {
            h.step();
        }
        // A large image's result lands from the background.
        let start = std::time::Instant::now();
        while h.state().click_job_running() && start.elapsed().as_secs() < 20 {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let tab = h.state().workspace().active().unwrap();
        let last = tab.history.entries().last().map(|e| e.name.clone());
        (tab.doc.layer(0).surface.to_pixels(), last, on_gpu)
    };
    let (gpu, gpu_step, on_gpu) = drag(&mut h, true);
    // Undo the move (and the lift) back to the original pixels.
    for _ in 0..2 {
        press(&mut h, ctrl(), Key::Z);
        if h.state()
            .workspace()
            .active()
            .unwrap()
            .doc
            .layer(0)
            .surface
            .to_pixels()
            == original
        {
            break;
        }
    }
    assert_eq!(
        h.state()
            .workspace()
            .active()
            .unwrap()
            .doc
            .layer(0)
            .surface
            .to_pixels(),
        original,
        "undone"
    );
    let (cpu, cpu_step, cpu_on_gpu) = drag(&mut h, false);
    assert!(!cpu_on_gpu, "the CPU run drew on the CPU");
    if !on_gpu {
        eprintln!("no wgpu adapter: the GPU path wasn't taken");
        return;
    }
    assert_eq!(gpu_step, cpu_step, "same history step ({w}×{height})");
    let diff = cpu.iter().zip(&gpu).filter(|(a, b)| a != b).count();
    assert_eq!(diff, 0, "{diff} pixels differ ({w}×{height})");
}

/// The move tools' cursor says what a drag would do: resize on a handle, move inside the
/// selection, rotate outside (drawn, so the system cursor hides).
#[test]
fn move_tool_cursor_shows_move_resize_and_rotate() {
    use eframe::egui::{CursorIcon, Event};
    let mut h = harness();
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.doc
        .set_selection(pnn_core::Selection::rect(20.0, 20.0, 60.0, 50.0));
    h.state_mut()
        .set_tool(pnn_tools::ToolKind::MoveSelectedPixels);
    h.step();
    // Rotation only just outside a corner (10 screen pixels out); elsewhere outside moves.
    let zoom = h.state().workspace().active().unwrap().view.unwrap().zoom;
    let d = 10.0 / zoom;
    for (p, want) in [
        ((30.0, 30.0), CursorIcon::Move),
        ((20.0, 20.0), CursorIcon::ResizeNorthWest),
        ((60.0, 35.0), CursorIcon::ResizeEast),
        ((60.0 + d, 50.0 + d), CursorIcon::None),
        ((20.0 - d, 20.0 - d), CursorIcon::None),
        ((60.0 + 4.0 * d, 35.0), CursorIcon::Move),
    ] {
        let s = screen(&h, p.0, p.1);
        h.event(Event::PointerMoved(s));
        h.step();
        h.step();
        assert_eq!(h.output().platform_output.cursor_icon, want, "at {p:?}");
    }
}

/// On a large image, Flip Layer and Fill Selection work on the effect threads and land as
/// one history step each, with the same pixels as on a small image.
#[test]
fn large_layer_commands_land_from_the_background() {
    use paintnotnet::commands::{Command, Triggered};
    use pnn_core::{ColorBgra, Document, Layer, Selection, Surface};
    let (w, hh) = (1500u32, 1000u32);
    let mut s = Surface::new_transparent(w, hh);
    s.set(3, 5, ColorBgra::from_rgba(10, 20, 30, 255));
    let mut h = harness_with(Document::from_layers(
        w,
        hh,
        vec![Layer::new("Background", s)],
    ));
    let ctx = h.ctx.clone();
    let wait = |h: &mut egui_kittest::Harness<'_, paintnotnet::app::PaintNotNetApp>| {
        let start = std::time::Instant::now();
        while h.state().click_job_running() {
            assert!(start.elapsed().as_secs() < 10, "the job never landed");
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.step();
        }
    };
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::FlipLayerHorizontal));
    assert!(h.state().click_job_running(), "in the background");
    wait(&mut h);
    let d = &h.state().workspace().active().unwrap().doc;
    assert_eq!(
        d.layer(0).surface.get(w - 4, 5),
        ColorBgra::from_rgba(10, 20, 30, 255)
    );
    assert_eq!(d.layer(0).surface.get(3, 5).a, 0);
    assert_eq!(history_names(&h).last().unwrap(), "Flip Horizontal");

    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.doc
        .set_selection(Selection::rect(100.0, 100.0, 200.0, 150.0));
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::FillSelection));
    wait(&mut h);
    let primary = h.state().settings().primary;
    let d = &h.state().workspace().active().unwrap().doc;
    assert_eq!(d.layer(0).surface.get(150, 120), primary);
    assert_eq!(d.layer(0).surface.get(250, 120).a, 0, "outside untouched");
    assert_eq!(history_names(&h).last().unwrap(), "Fill Selection");
}
