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

#[test]
fn layers_history_and_layer_properties_render() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    for c in [
        Command::AddNewLayer,
        Command::AddNewLayer,
        Command::FlipImageHorizontal,
        Command::LayerProperties,
    ] {
        h.state_mut().execute(&ctx, Triggered::once(c));
        h.step();
    }
    for _ in 0..3 {
        h.step();
    }
    assert!(h.state_mut().doc_dialog_mut().is_some());
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-phase3.png")).unwrap();
    }
}

#[test]
fn marching_ants_render() {
    let mut h = harness();
    {
        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
        let sel = pnn_core::Selection::ellipse(100.0, 100.0, 500.0, 400.0).combine(
            &pnn_core::Selection::rect(400.0, 300.0, 700.0, 550.0),
            pnn_core::CombineMode::Union,
        );
        tab.doc.set_selection(sel);
    }
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-ants.png")).unwrap();
    }
}

#[test]
fn feather_preview_render() {
    let mut h = harness();
    {
        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
        let feather = pnn_core::selection::Feather {
            radius: 60.0,
            ..pnn_core::selection::Feather::NONE
        };
        let sel = pnn_core::Selection::ellipse(150.0, 100.0, 650.0, 500.0).with_feather(feather);
        tab.doc.set_selection(sel);
    }
    h.state_mut().set_feather_preview(true);
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image
            .save(format!("{dir}/shell-feather-preview.png"))
            .unwrap();
    }
}

/// Paintbrush strokes through real pointer events: a soft, half-transparent loop that
/// crosses itself (no build-up), a hatched stroke, and the brush outline at the pointer.
#[test]
fn paintbrush_render() {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
    let mut h = harness();
    h.state_mut().set_tool(pnn_tools::ToolKind::Paintbrush);
    h.step();
    let to_screen = |h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64| {
        let rect = h.state().canvas().rect.unwrap();
        let view = h.state().workspace().active().unwrap().view.unwrap();
        let s = view.image_to_screen([x, y], [rect.width() as f64, rect.height() as f64]);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    let stroke = |h: &mut Harness<'_, PaintNotNetApp>, pts: &[(f64, f64)], release: bool| {
        h.event(Event::PointerMoved(to_screen(h, pts[0].0, pts[0].1)));
        h.step();
        let press = |pressed, pos| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.event(press(true, to_screen(h, pts[0].0, pts[0].1)));
        h.step();
        for &(x, y) in &pts[1..] {
            h.event(Event::PointerMoved(to_screen(h, x, y)));
            h.step();
        }
        if release {
            let last = *pts.last().unwrap();
            h.event(press(false, to_screen(h, last.0, last.1)));
            h.step();
        }
    };
    {
        let s = h.state_mut().settings_mut();
        s.brush_width = 50.0;
        s.hardness = 0;
        s.primary = pnn_core::ColorBgra::from_rgba(200, 30, 30, 160);
    }
    let lp: Vec<(f64, f64)> = (0..=40)
        .map(|i| {
            let t = i as f64 / 40.0 * std::f64::consts::TAU * 1.25;
            (250.0 + 150.0 * t.cos(), 250.0 + 120.0 * (2.0 * t).sin())
        })
        .collect();
    stroke(&mut h, &lp, true);
    {
        let s = h.state_mut().settings_mut();
        s.brush_width = 60.0;
        s.hardness = 100;
        s.primary = pnn_core::ColorBgra::from_rgba(20, 60, 200, 255);
        s.secondary = pnn_core::ColorBgra::from_rgba(255, 220, 120, 255);
        s.fill = pnn_tools::FillStyle::Hatch(5);
    }
    stroke(
        &mut h,
        &[
            (480.0, 120.0),
            (560.0, 250.0),
            (640.0, 300.0),
            (720.0, 480.0),
        ],
        true,
    );
    h.event(Event::PointerMoved(to_screen(&h, 600.0, 520.0)));
    for _ in 0..3 {
        h.step();
    }
    let names: Vec<String> = h
        .state()
        .workspace()
        .active()
        .unwrap()
        .history
        .entries()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names.iter().filter(|n| *n == "Paintbrush").count(), 2);
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-paintbrush.png")).unwrap();
    }
}

/// A live radial gradient with its two nubs, and a Paint Bucket fill (hatched) under it.
#[test]
fn gradient_render() {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
    let mut h = harness();
    let to_screen = |h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64| {
        let rect = h.state().canvas().rect.unwrap();
        let view = h.state().workspace().active().unwrap().view.unwrap();
        let s = view.image_to_screen([x, y], [rect.width() as f64, rect.height() as f64]);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    let drag = |h: &mut Harness<'_, PaintNotNetApp>, pts: &[(f64, f64)]| {
        let press = |pressed, pos| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.event(Event::PointerMoved(to_screen(h, pts[0].0, pts[0].1)));
        h.step();
        h.event(press(true, to_screen(h, pts[0].0, pts[0].1)));
        h.step();
        for &(x, y) in &pts[1..] {
            h.event(Event::PointerMoved(to_screen(h, x, y)));
            h.step();
        }
        let last = *pts.last().unwrap();
        h.event(press(false, to_screen(h, last.0, last.1)));
        h.step();
    };
    {
        let s = h.state_mut().settings_mut();
        s.primary = pnn_core::ColorBgra::from_rgba(250, 200, 40, 255);
        s.secondary = pnn_core::ColorBgra::from_rgba(40, 60, 160, 255);
        s.gradient_type = pnn_tools::GradientType::Radial;
    }
    h.state_mut().set_tool(pnn_tools::ToolKind::Gradient);
    h.step();
    drag(&mut h, &[(400.0, 300.0), (550.0, 350.0), (650.0, 420.0)]);
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-gradient.png")).unwrap();
    }
}

/// A live spline with an arrow end cap, a round start cap and dashes, nubs and move icon.
#[test]
fn line_curve_render() {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
    let mut h = harness();
    let to_screen = |h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64| {
        let rect = h.state().canvas().rect.unwrap();
        let view = h.state().workspace().active().unwrap().view.unwrap();
        let s = view.image_to_screen([x, y], [rect.width() as f64, rect.height() as f64]);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    let drag = |h: &mut Harness<'_, PaintNotNetApp>, pts: &[(f64, f64)]| {
        let press = |pressed, pos| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.event(Event::PointerMoved(to_screen(h, pts[0].0, pts[0].1)));
        h.step();
        h.event(press(true, to_screen(h, pts[0].0, pts[0].1)));
        h.step();
        for &(x, y) in &pts[1..] {
            h.event(Event::PointerMoved(to_screen(h, x, y)));
            h.step();
        }
        let last = *pts.last().unwrap();
        h.event(press(false, to_screen(h, last.0, last.1)));
        h.step();
    };
    {
        let s = h.state_mut().settings_mut();
        s.brush_width = 8.0;
        s.start_cap = pnn_tools::LineCap::Rounded;
        s.end_cap = pnn_tools::LineCap::ArrowFilled;
        s.dash = pnn_tools::DashStyle::DashDot;
        s.primary = pnn_core::ColorBgra::from_rgba(30, 90, 200, 255);
    }
    h.state_mut().set_tool(pnn_tools::ToolKind::LineCurve);
    h.step();
    drag(&mut h, &[(150.0, 450.0), (400.0, 300.0), (650.0, 150.0)]);
    // Bend it: drag the second nub (a third of the way along) down.
    drag(&mut h, &[(316.67, 350.0), (330.0, 500.0)]);
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-line-curve.png")).unwrap();
    }
}
