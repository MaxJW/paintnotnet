//! Renders the whole shell at the size of paint.net 5's reference screenshot (1202×867) and
//! checks the layout matches paint.net's: the four utility windows parked in their corners
//! inside the workspace, not overlapping, Tools as a narrow two-column strip.
//!
//! Set `PNN_RENDER_DUMP=<dir>` to write the frame for side-by-side comparison with
//! `reference/pdn5-docs/images/mainwindow/mainwindow.png`.

use eframe::egui::vec2;
use egui_kittest::Harness;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Panel, Triggered};
use pnn_core::Document;

fn harness() -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder()
        .with_size(vec2(1202.0, 867.0))
        .wgpu()
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.state_mut()
        .workspace_mut()
        .add(Document::new_blank(800, 600), None, None);
    let ctx = h.ctx.clone();
    if h.state().diagnostics().visible() {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
    }
    for _ in 0..4 {
        h.step();
    }
    h
}

#[test]
fn utility_windows_park_like_paint_net() {
    let mut h = harness();
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-1202x867.png")).unwrap();
    }
    let canvas = h.state().canvas().rect.expect("canvas laid out");
    let panels = h.state().panels();
    let rects: Vec<_> = Panel::ALL.iter().map(|&p| (p, panels.rect(p))).collect();
    for &(p, r) in &rects {
        assert!(r.is_positive(), "{p:?} not shown");
        assert!(
            canvas.expand(1.0).contains_rect(r),
            "{p:?} {r:?} outside workspace {canvas:?}"
        );
    }
    for (i, &(a, ra)) in rects.iter().enumerate() {
        for &(b, rb) in &rects[i + 1..] {
            assert!(!ra.intersects(rb), "{a:?} overlaps {b:?}");
        }
    }
    let r = |p| panels.rect(p);
    let mid = canvas.center();
    assert!(
        r(Panel::Tools).center().x < mid.x && r(Panel::Tools).center().y < mid.y,
        "Tools top-left"
    );
    assert!(
        r(Panel::History).center().x > mid.x && r(Panel::History).center().y < mid.y,
        "History top-right"
    );
    assert!(
        r(Panel::Layers).center().x > mid.x && r(Panel::Layers).center().y > mid.y,
        "Layers bottom-right"
    );
    assert!(
        r(Panel::Colors).center().x < mid.x && r(Panel::Colors).center().y > mid.y,
        "Colors bottom-left"
    );
    assert!(
        r(Panel::Tools).width() <= 64.0,
        "Tools is a narrow strip, got {}",
        r(Panel::Tools).width()
    );
}

#[test]
fn f5_to_f8_toggle_windows_and_reset_restores() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::ToggleWindow(Panel::Layers)));
    h.step();
    assert!(!h.state().panels().visible(Panel::Layers));
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::ResetWindow(Panel::Layers)));
    h.step();
    assert!(h.state().panels().visible(Panel::Layers));
}

fn press(
    h: &mut Harness<'_, PaintNotNetApp>,
    modifiers: eframe::egui::Modifiers,
    key: eframe::egui::Key,
) {
    use eframe::egui::Event;
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
        Event::ModifiersChanged(eframe::egui::Modifiers::NONE),
    ];
    let n = events.len();
    for e in events {
        h.event(e);
    }
    for _ in 0..n + 1 {
        h.step();
    }
}

#[test]
fn alt_f_then_n_opens_new_image_dialog() {
    use eframe::egui::{Key, Modifiers};
    let mut h = harness();
    press(&mut h, Modifiers::ALT, Key::F);
    assert!(h.state().menu_open(), "Alt+F opens the File menu");
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-file-menu.png")).unwrap();
    }
    press(&mut h, Modifiers::NONE, Key::N);
    assert!(
        matches!(
            h.state().modal(),
            Some(paintnotnet::dialogs::Modal::NewImage(_))
        ),
        "N picks New... from the open File menu"
    );
    assert!(!h.state().menu_open());
}

#[test]
fn tool_letters_cycle_like_paint_net() {
    use eframe::egui::{Key, Modifiers};
    use pnn_tools::ToolKind;
    let mut h = harness();
    press(&mut h, Modifiers::NONE, Key::S);
    assert_eq!(h.state().tool(), ToolKind::RectangleSelect);
    press(&mut h, Modifiers::NONE, Key::S);
    assert_eq!(h.state().tool(), ToolKind::LassoSelect);
    press(&mut h, Modifiers::SHIFT, Key::S);
    assert_eq!(h.state().tool(), ToolKind::MagicWand);
    press(&mut h, Modifiers::NONE, Key::O);
    press(&mut h, Modifiers::NONE, Key::O);
    assert_eq!(h.state().tool(), ToolKind::Shapes);
    press(&mut h, Modifiers::NONE, Key::B);
    assert_eq!(h.state().tool(), ToolKind::Paintbrush);
    let width = h.state().settings().brush_width;
    press(&mut h, Modifiers::NONE, Key::CloseBracket);
    press(
        &mut h,
        Modifiers::CTRL | Modifiers::COMMAND,
        Key::CloseBracket,
    );
    assert_eq!(h.state().settings().brush_width, width + 6.0);
    press(&mut h, Modifiers::NONE, Key::X);
    assert_eq!(
        h.state().settings().primary,
        pnn_core::ColorBgra::WHITE,
        "X swaps colours"
    );
}

#[test]
fn dragging_a_window_near_an_edge_snaps_it() {
    use eframe::egui::{Event, Modifiers, PointerButton, pos2};
    let mut h = harness();
    let canvas = h.state().canvas().rect.unwrap();
    let tools = h.state().panels().rect(Panel::Tools);
    // Grab the Tools title bar and drop it 10 px short of the workspace's right edge,
    // vertically mid-way (away from History and Layers).
    let grab = pos2(tools.min.x + 12.0, tools.min.y + 8.0);
    let target_x = canvas.max.x - tools.width() - 10.0 + 12.0;
    let target = pos2(target_x, canvas.center().y - 100.0);
    h.event(Event::PointerMoved(grab));
    h.step();
    h.event(Event::PointerButton {
        pos: grab,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        h.event(Event::PointerMoved(grab + (target - grab) * t));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: target,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..3 {
        h.step();
    }
    let after = h.state().panels().rect(Panel::Tools);
    assert!(
        (canvas.max.x - after.max.x - 3.0).abs() < 1.5,
        "Tools snapped 3 px from the right edge; right edge at {}, workspace right {}",
        after.max.x,
        canvas.max.x
    );
}
