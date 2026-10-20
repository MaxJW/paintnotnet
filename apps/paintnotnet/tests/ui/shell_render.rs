//! Renders the whole shell at the size of paint.net 5's reference screenshot (1202×867) and
//! checks design A's docked layout: the tool rail top-left, Colors, Layers and History
//! stacked in the right dock, the canvas fitted between them, nothing overlapping.
//!
//! Set `PNN_RENDER_DUMP=<dir>` to write the frame for side-by-side comparison with
//! `reference/pdn5-docs/images/mainwindow/mainwindow.png`.

use eframe::egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
fn docked_shell_matches_design_a() {
    let _gpu = crate::common::gpu_lock();
    let mut h = harness();
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-1202x867.png")).unwrap();
    }
    let canvas = h.state().canvas().rect.expect("canvas laid out");
    let panels = h.state().panels();
    let rects: Vec<_> = Panel::ALL.iter().map(|&p| (p, panels.rect(p))).collect();
    let screen = eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, vec2(1202.0, 867.0));
    for &(p, r) in &rects {
        assert!(r.is_positive(), "{p:?} not shown");
        assert!(screen.contains_rect(r), "{p:?} {r:?} off screen");
        assert!(
            !r.intersects(canvas),
            "{p:?} {r:?} covers the free canvas {canvas:?}"
        );
    }
    for (i, &(a, ra)) in rects.iter().enumerate() {
        for &(b, rb) in &rects[i + 1..] {
            assert!(!ra.intersects(rb), "{a:?} overlaps {b:?}");
        }
    }
    let r = |p| panels.rect(p);
    let tools = r(Panel::Tools);
    assert!(tools.max.x < canvas.min.x, "rail left of the canvas");
    assert!(tools.width() <= 64.0, "one column, got {}", tools.width());
    let (colors, layers, history) = (r(Panel::Colors), r(Panel::Layers), r(Panel::History));
    assert!(colors.min.x > canvas.max.x, "dock right of the canvas");
    assert!(
        colors.max.y < layers.min.y && layers.max.y < history.min.y,
        "stacked in order"
    );
    assert_eq!(colors.x_range(), history.x_range(), "one column");
    // 305 px before the roomier spacing (2026-10-15), which costs ~26 px of height here, and
    // 38 px less since the command bar under the menus (2026-10-20).
    assert!(
        (history.max.y - layers.min.y) > 235.0,
        "Layers and History fill the dock: {}",
        history.max.y - layers.min.y
    );
    // Zoomed in, the (white) image runs on under the panels, which show it as glass that
    // stays dark enough for their secondary text.
    let ctx = h.ctx.clone();
    for _ in 0..3 {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ZoomIn));
    }
    for _ in 0..3 {
        h.step();
    }
    let image = h.render().expect("render");
    let empty_history = h.state().panels().rect(Panel::History).center_bottom() - vec2(0.0, 20.0);
    let glass = image.get_pixel(empty_history.x as u32, empty_history.y as u32);
    let weak = paintnotnet::theme::Palette::dark().weak_text;
    let ratio = contrast(
        [glass[0], glass[1], glass[2]],
        [weak.r(), weak.g(), weak.b()],
    );
    assert!(
        ratio >= 4.5,
        "glass {glass:?} over white: weak text contrast {ratio:.2}"
    );
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP") {
        image.save(format!("{dir}/shell-zoomed.png")).unwrap();
        h.ctx.set_theme(eframe::egui::ThemePreference::Light);
        for _ in 0..3 {
            h.step();
        }
        if let Ok(image) = h.render() {
            image.save(format!("{dir}/shell-light.png")).unwrap();
        }
    }
}

/// WCAG contrast ratio of two sRGB colours.
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let lum = |c: [u8; 3]| {
        let l = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn dock_resizes_collapses_and_remembers() {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{Event, Modifiers, PointerButton};
    let mut h = harness();
    let layers = h.state().panels().rect(Panel::Layers);
    // Drag the dock's left edge 60 px to the left.
    let grab = eframe::egui::pos2(layers.min.x - 2.0, layers.center().y);
    h.event(Event::PointerMoved(grab));
    h.step();
    h.event(Event::PointerButton {
        pos: grab,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=6 {
        h.event(Event::PointerMoved(grab - vec2(10.0 * i as f32, 0.0)));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: grab - vec2(60.0, 0.0),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..3 {
        h.step();
    }
    let wider = h.state().panels().rect(Panel::Layers);
    assert!(
        (wider.width() - layers.width() - 60.0).abs() < 1.5,
        "dock widened by the drag: {} → {}",
        layers.width(),
        wider.width()
    );
    // Click the History title to collapse it to its title row.
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Button, "History")
        .click();
    for _ in 0..3 {
        h.step();
    }
    let history = h.state().panels().rect(Panel::History);
    assert_eq!(history.height(), paintnotnet::panels::HEADER);
    let dock = &h.state().config().dock;
    assert!(
        (dock.width - wider.width()).abs() < 0.5,
        "width saved: {}",
        dock.width
    );
    assert_eq!(dock.collapsed, vec!["History".to_owned()]);
}

#[test]
fn f5_to_f8_toggle_windows_and_reset_restores() {
    let _gpu = crate::common::gpu_lock();
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
    let _gpu = crate::common::gpu_lock();
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
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-new-image.png")).unwrap();
    }
    assert!(!h.state().menu_open());
}

/// File > Open Recent with thumbnails of the files opened (paint.net 3.36 shows each beside
/// its name).
#[test]
fn open_recent_render() {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{Key, Modifiers};
    use pnn_core::{ColorBgra, Layer, Surface};
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    let ctx = h.ctx.clone();
    let files = [
        (
            "sunset.png",
            400,
            240,
            ColorBgra::from_rgba(230, 120, 40, 255),
        ),
        (
            "portrait.png",
            200,
            320,
            ColorBgra::from_rgba(40, 110, 200, 255),
        ),
        ("logo.png", 120, 120, ColorBgra::from_rgba(40, 170, 90, 128)),
    ];
    for (name, w, hgt, c) in files {
        let mut s = Surface::new_transparent(w, hgt);
        s.fill(c);
        let path = dir.path().join(name);
        pnn_io::save(
            &Document::from_layers(w, hgt, vec![Layer::new("Background", s)]),
            &path,
            pnn_io::FileFormat::Png,
            Default::default(),
        )
        .unwrap();
        h.state_mut().note_recent(&path);
        let tabs = h.state().workspace().tabs().len();
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::OpenRecent(0)));
        for _ in 0..200 {
            h.step();
            if h.state().workspace().tabs().len() > tabs {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    for _ in 0..10 {
        h.step();
    }
    let recent = h.state_mut().recent_menu_files(&ctx);
    assert!(
        recent.iter().all(|f| f.thumb.is_some()),
        "every file has one"
    );
    press(&mut h, Modifiers::ALT, Key::F);
    h.get_by_label_contains("Recent").hover();
    for _ in 0..6 {
        h.step();
    }
    h.get_by_label("1 logo.png");
    h.get_by_label("3 sunset.png");
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-open-recent.png")).unwrap();
    }
}

#[test]
fn tool_letters_cycle_like_paint_net() {
    let _gpu = crate::common::gpu_lock();
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
fn layers_history_and_layer_properties_render() {
    let _gpu = crate::common::gpu_lock();
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
    let _gpu = crate::common::gpu_lock();
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
    let _gpu = crate::common::gpu_lock();
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
    let _gpu = crate::common::gpu_lock();
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
    let _gpu = crate::common::gpu_lock();
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
    let _gpu = crate::common::gpu_lock();
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

/// All 29 shapes, filled with outline, in a grid; the last one still live with its handles.
#[test]
fn shapes_render() {
    let _gpu = crate::common::gpu_lock();
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
    h.state_mut().set_tool(pnn_tools::ToolKind::Shapes);
    {
        let s = h.state_mut().settings_mut();
        s.shape_draw = pnn_tools::ShapeDrawType::Both;
        s.brush_width = 3.0;
        s.primary = pnn_core::ColorBgra::from_rgba(20, 40, 110, 255);
        s.secondary = pnn_core::ColorBgra::from_rgba(250, 190, 60, 255);
    }
    for (i, shape) in pnn_tools::ShapeType::ALL.iter().enumerate() {
        h.state_mut().settings_mut().shape = *shape;
        let (col, row) = ((i % 6) as f64, (i / 6) as f64);
        let (x, y) = (20.0 + col * 130.0, 15.0 + row * 117.0);
        drag(
            &mut h,
            &[(x, y), (x + 50.0, y + 40.0), (x + 100.0, y + 90.0)],
        );
    }
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-shapes.png")).unwrap();
    }
}

/// Live text: two lines, bold italic underlined, centred, with the caret and move nub.
#[test]
fn text_render() {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
    let mut h = harness();
    let to_screen = |h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64| {
        let rect = h.state().canvas().rect.unwrap();
        let view = h.state().workspace().active().unwrap().view.unwrap();
        let s = view.image_to_screen([x, y], [rect.width() as f64, rect.height() as f64]);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    h.state_mut().set_tool(pnn_tools::ToolKind::Text);
    {
        let s = h.state_mut().settings_mut();
        s.font_family = "DejaVu Serif".into();
        s.font_size = 36.0;
        s.bold = true;
        s.italic = true;
        s.underline = true;
        s.text_align = pnn_tools::TextAlignment::Center;
        s.primary = pnn_core::ColorBgra::from_rgba(150, 30, 60, 255);
    }
    h.step();
    let p = to_screen(&h, 400.0, 200.0);
    h.event(Event::PointerMoved(p));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.event(Event::Text("paint.net on Linux".into()));
    h.step();
    h.event(Event::Key {
        key: eframe::egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.event(Event::Text("Grüße, ünïcode ✓".into()));
    for _ in 0..4 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-text.png")).unwrap();
    }
}

/// An adjustment dialog (Hue / Saturation) over a live preview.
#[test]
fn adjustment_dialog_render() {
    let _gpu = crate::common::gpu_lock();
    let mut h = harness();
    {
        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
        let (w, hh) = (tab.doc.width(), tab.doc.height());
        let a = tab.doc.active_index();
        tab.doc.modify_layer(a, None, |l| {
            for y in 0..hh {
                for x in 0..w {
                    let c = pnn_core::ColorBgra::from_rgba(
                        (x * 255 / w) as u8,
                        (y * 255 / hh) as u8,
                        160,
                        255,
                    );
                    l.surface.set(x, y, c);
                }
            }
        });
    }
    let ctx = h.ctx.clone();
    h.state_mut().execute(
        &ctx,
        Triggered::once(Command::Adjustment(pnn_effects::Adjustment::HueSaturation)),
    );
    {
        let e = h.state_mut().effect_mut().unwrap();
        e.set_values(
            &ctx,
            vec![
                pnn_effects::framework::Value::Int(120),
                pnn_effects::framework::Value::Int(100),
                pnn_effects::framework::Value::Int(0),
            ],
        );
    }
    for _ in 0..6 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-adjustment.png")).unwrap();
    }
}

/// Curves (RGB, red lifted) and Levels dialogs over a live preview.
#[test]
fn curves_and_levels_render() {
    let _gpu = crate::common::gpu_lock();
    use paintnotnet::effect_runner::Custom;
    use pnn_effects::Adjustment;
    use pnn_effects::curves::{Curves, Points};
    for (adj, file) in [
        (Adjustment::Curves, "curves"),
        (Adjustment::Levels, "levels"),
    ] {
        let mut h = harness();
        {
            let tab = h.state_mut().workspace_mut().active_mut().unwrap();
            let (w, hh) = (tab.doc.width(), tab.doc.height());
            let a = tab.doc.active_index();
            tab.doc.modify_layer(a, None, |l| {
                for y in 0..hh {
                    for x in 0..w {
                        let v = ((x + y) * 255 / (w + hh)) as u8;
                        let c = pnn_core::ColorBgra::from_rgba(v, v / 2 + 60, 200 - v / 2, 255);
                        l.surface.set(x, y, c);
                    }
                }
            });
        }
        let ctx = h.ctx.clone();
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::Adjustment(adj)));
        if adj == Adjustment::Curves {
            let mut c = Curves {
                luminosity: false,
                mask: [true, false, false],
                ..Curves::default()
            };
            c.rgb[0] = Points::from([(0, 0), (90, 170), (255, 255)]);
            h.state_mut()
                .effect_mut()
                .unwrap()
                .set_custom(&ctx, Custom::Curves(c));
        }
        for _ in 0..6 {
            h.step();
        }
        if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
            && let Ok(image) = h.render()
        {
            image.save(format!("{dir}/shell-{file}.png")).unwrap();
        }
    }
}

/// Settings › Keyboard and the command palette.
#[test]
fn settings_and_palette_render() {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{Event, Key, Modifiers};
    let mut h = harness();
    let ctx = h.ctx.clone();
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::Settings));
    h.step();
    h.step();
    h.get_by_label("Keyboard").click();
    for _ in 0..3 {
        h.step();
    }
    h.event(Event::Text("blur".into()));
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-settings.png")).unwrap();
    }
    let mut h = harness();
    let ctx = h.ctx.clone();
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::CommandPalette));
    h.step();
    h.event(Event::Text("sha".into()));
    for _ in 0..3 {
        h.step();
    }
    h.event(Event::Key {
        key: Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..3 {
        h.step();
    }
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-palette.png")).unwrap();
    }
}

fn effect_named(name: &str, category: pnn_effects::Category) -> Command {
    let (id, _) = pnn_effects::effects_in(category)
        .find(|(_, e)| e.name == name)
        .unwrap();
    Command::Effect(id)
}

/// The built-in Object and Selection effects' dialogs: Bevel Object (angle dial, colour
/// section opened) over a red object, and Outline Selection (colour well, dropdowns) dashed
/// around an elliptical selection.
#[test]
fn object_and_selection_dialogs_render() {
    let _gpu = crate::common::gpu_lock();
    let mut h = harness();
    {
        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
        let a = tab.doc.active_index();
        tab.doc.modify_layer(a, None, |l| {
            l.surface = pnn_core::Surface::new_transparent(800, 600);
            for y in 150..450 {
                for x in 200..600 {
                    let (dx, dy) = (x as f32 - 400.0, y as f32 - 300.0);
                    if (dx / 200.0).powi(2) + (dy / 150.0).powi(2) <= 1.0 {
                        l.surface
                            .set(x, y, pnn_core::ColorBgra::from_rgba(210, 40, 40, 255));
                    }
                }
            }
        });
    }
    let ctx = h.ctx.clone();
    h.state_mut().execute(
        &ctx,
        Triggered::once(effect_named("Bevel Object", pnn_effects::Category::Object)),
    );
    h.step();
    // "Colors" is also the Colors panel's title: open the dialog's section header, which
    // is drawn after the dock.
    h.get_all_by_role_and_label(eframe::egui::accesskit::Role::Button, "Colors")
        .last()
        .expect("the dialog's Colors section")
        .click();
    for _ in 0..8 {
        h.step();
    }
    let dump = std::env::var("PNN_RENDER_DUMP").ok();
    if let Some(dir) = &dump
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-bevel.png")).unwrap();
    }
    h.get_by_label("OK").click();
    for _ in 0..4 {
        h.step();
    }
    {
        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
        tab.doc
            .set_selection(pnn_core::Selection::ellipse(150.0, 120.0, 650.0, 480.0));
    }
    h.state_mut().execute(
        &ctx,
        Triggered::once(effect_named(
            "Outline Selection",
            pnn_effects::Category::Selection,
        )),
    );
    h.step();
    {
        use pnn_effects::framework::Value;
        let e = h.state_mut().effect_mut().unwrap();
        e.set_values(
            &ctx,
            vec![
                Value::Int(8),
                Value::Color(pnn_core::ColorBgra::from_rgba(30, 90, 220, 255)),
                Value::Choice(1),
                Value::Choice(1),
                Value::Choice(1),
                Value::Bool(true),
            ],
        );
    }
    for _ in 0..8 {
        h.step();
    }
    if let Some(dir) = &dump
        && let Ok(image) = h.render()
    {
        image
            .save(format!("{dir}/shell-outline-selection.png"))
            .unwrap();
    }
}

/// Most tools draw their own cursor over the canvas (crosshair and the tool's icon) and hide
/// the system one; Pan keeps the system's grab hand.
#[test]
fn tools_draw_their_cursor_over_the_canvas() {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{CursorIcon, Event};
    let mut h = harness();
    let canvas = h.state().canvas().rect.unwrap();
    h.event(Event::PointerMoved(canvas.center()));
    h.step();
    h.step();
    assert_eq!(
        h.output().platform_output.cursor_icon,
        CursorIcon::None,
        "paintbrush hides the system cursor"
    );
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-cursor.png")).unwrap();
    }
    // Alt + right-drag shows the resize HUD at the press point and hides the cursor.
    use eframe::egui::{Modifiers, PointerButton};
    let at = canvas.center();
    h.event(Event::ModifiersChanged(Modifiers::ALT));
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::ALT,
    });
    h.step();
    h.event(Event::PointerMoved(at + vec2(330.0, -20.0)));
    h.step();
    h.step();
    assert_eq!(h.output().platform_output.cursor_icon, CursorIcon::None);
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-resize-hud.png")).unwrap();
    }
    h.event(Event::PointerButton {
        pos: at + vec2(330.0, -20.0),
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::ALT,
    });
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();

    h.state_mut().set_tool(pnn_tools::ToolKind::Pan);
    h.step();
    h.step();
    assert_eq!(h.output().platform_output.cursor_icon, CursorIcon::Grab);
}

/// Collapsing a section glides over `motion::NORMAL` instead of jumping; dragging the dock's
/// edge follows the pointer at once.
#[test]
fn collapsing_glides_but_dragging_does_not() {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{Event, Modifiers, PointerButton};
    use paintnotnet::panels::HEADER;
    let mut h = Harness::builder()
        .with_size(vec2(1202.0, 867.0))
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.state_mut()
        .workspace_mut()
        .add(Document::new_blank(800, 600), None, None);
    for _ in 0..4 {
        h.step();
    }
    let open = h.state().panels().rect(Panel::History).height();
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Button, "History")
        .click();
    for _ in 0..4 {
        h.step();
    }
    let midway = h.state().panels().rect(Panel::History).height();
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP")
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-collapsing.png")).unwrap();
    }
    assert!(
        midway > HEADER + 1.0 && midway < open - 1.0,
        "part way after 3 frames: {open} → {midway}"
    );
    for _ in 0..20 {
        h.step();
    }
    assert_eq!(h.state().panels().rect(Panel::History).height(), HEADER);

    let layers = h.state().panels().rect(Panel::Layers);
    let grab = eframe::egui::pos2(layers.min.x - 2.0, layers.center().y);
    h.event(Event::PointerMoved(grab));
    h.step();
    h.event(Event::PointerButton {
        pos: grab,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.event(Event::PointerMoved(grab - vec2(40.0, 0.0)));
    h.step();
    h.step();
    let wider = h.state().panels().rect(Panel::Layers);
    assert!(
        (wider.width() - layers.width() - 40.0).abs() < 1.5,
        "width follows the drag at once: {} → {}",
        layers.width(),
        wider.width()
    );
}

/// The move tools' nubs and pivot before a move, then after the pivot is dragged to the
/// top-left nub and the pixels turned from just outside the bottom-right corner.
#[test]
fn move_pivot_render() {
    let _gpu = crate::common::gpu_lock();
    use crate::common::drag;
    use eframe::egui::{Modifiers, PointerButton};
    let mut h = harness();
    {
        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
        let mut s = pnn_core::Surface::new_transparent(800, 600);
        for y in 150..400 {
            for x in 200..500 {
                let c = ((x / 25 + y / 25) % 2) as u8 * 200;
                s.set(x, y, pnn_core::ColorBgra::from_rgba(230, c, 255 - c, 255));
            }
        }
        tab.doc.modify_layer(0, None, |l| l.surface = s);
        tab.doc
            .set_selection(pnn_core::Selection::rect(200.0, 150.0, 500.0, 400.0));
    }
    h.state_mut()
        .set_tool(pnn_tools::ToolKind::MoveSelectedPixels);
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(h.state().canvas().move_pivot, Some([350.0, 275.0]));
    let dir = std::env::var("PNN_RENDER_DUMP").ok();
    if let Some(dir) = &dir
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-move-frame.png")).unwrap();
    }
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(350.0, 275.0), (260.0, 200.0), (201.0, 151.0)],
    );
    assert_eq!(h.state().canvas().move_pivot, Some([200.0, 150.0]));
    let zoom = h.state().workspace().active().unwrap().view.unwrap().zoom;
    let d = 10.0 / zoom;
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::SHIFT,
        &[(500.0 + d, 400.0 + d), (470.0, 440.0), (420.0, 480.0)],
    );
    for _ in 0..3 {
        h.step();
    }
    if let Some(dir) = &dir
        && let Ok(image) = h.render()
    {
        image.save(format!("{dir}/shell-move-rotated.png")).unwrap();
    }
}

/// Every window and dialog, dark and light, in a large and a small window, for review
/// (`PNN_RENDER_DUMP=<dir>` writes `dlg-<name>-<size>-<theme>.png`; nothing without it).
#[test]
fn dialog_gallery() {
    let _gpu = crate::common::gpu_lock();
    use eframe::egui::{Key, Modifiers, ThemePreference};
    use pnn_effects::Adjustment;
    let Ok(dir) = std::env::var("PNN_RENDER_DUMP") else {
        return;
    };
    let effect = |name: &str| {
        let i = pnn_effects::EFFECTS
            .iter()
            .position(|e| e.name == name)
            .unwrap_or_else(|| panic!("no effect {name}"));
        Command::Effect(pnn_effects::EffectId(i as u16))
    };
    let shots: Vec<(&str, Option<Command>)> = vec![
        ("new-image", Some(Command::NewImage)),
        ("resize", Some(Command::Resize)),
        ("canvas-size", Some(Command::CanvasSize)),
        ("rotate-zoom", Some(Command::RotateZoom)),
        ("layer-properties", Some(Command::LayerProperties)),
        ("feather-selection", Some(Command::FeatherSelection)),
        ("feather-object", Some(effect("Feather Object"))),
        ("gaussian-blur", Some(effect("Gaussian Blur"))),
        ("drop-shadow", Some(effect("Drop Shadow"))),
        ("levels", Some(Command::Adjustment(Adjustment::Levels))),
        ("curves", Some(Command::Adjustment(Adjustment::Curves))),
        (
            "hue-saturation",
            Some(Command::Adjustment(Adjustment::HueSaturation)),
        ),
        ("remove-background", Some(effect("Remove Background"))),
        ("settings", Some(Command::Settings)),
        ("about", Some(Command::About)),
        ("save-jpeg", None),
        ("save-webp", None),
        ("flatten-prompt", None),
        ("unsaved-close", None),
        ("paste-larger", None),
    ];
    use paintnotnet::commands::MenuId;
    let shots: Vec<(&str, Option<Command>)> = shots
        .into_iter()
        .chain([
            ("menu-file", Some(Command::OpenMenu(MenuId::File))),
            ("menu-edit", Some(Command::OpenMenu(MenuId::Edit))),
            ("menu-view", Some(Command::OpenMenu(MenuId::View))),
            ("menu-image", Some(Command::OpenMenu(MenuId::Image))),
            ("menu-layers", Some(Command::OpenMenu(MenuId::Layers))),
            (
                "menu-adjustments",
                Some(Command::OpenMenu(MenuId::Adjustments)),
            ),
            ("menu-effects", Some(Command::OpenMenu(MenuId::Effects))),
            ("menu-help", Some(Command::OpenMenu(MenuId::Help))),
        ])
        .collect();
    let tmp = tempfile::tempdir().unwrap();
    for (size, w, hh) in [("large", 1202.0, 867.0), ("small", 860.0, 600.0)] {
        for (theme, pref) in [
            ("dark", ThemePreference::Dark),
            ("light", ThemePreference::Light),
        ] {
            for (name, command) in &shots {
                let mut h = Harness::builder()
                    .with_size(vec2(w, hh))
                    .wgpu()
                    .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
                h.ctx.set_theme(pref);
                h.state_mut()
                    .workspace_mut()
                    .add(Document::new_blank(800, 600), None, None);
                let tab = h.state_mut().workspace_mut().active_mut().unwrap();
                tab.doc
                    .set_selection(pnn_core::Selection::ellipse(200.0, 150.0, 600.0, 450.0));
                h.step();
                h.step();
                let ctx = h.ctx.clone();
                if h.state().diagnostics().visible() {
                    h.state_mut()
                        .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
                }
                let ctrl = Modifiers {
                    ctrl: true,
                    command: true,
                    ..Modifiers::NONE
                };
                match (command, *name) {
                    (Some(c), _) => h.state_mut().execute(&ctx, Triggered::once(*c)),
                    (None, "save-jpeg" | "save-webp" | "flatten-prompt") => {
                        let ext = match *name {
                            "save-webp" => "webp",
                            "save-jpeg" => "jpg",
                            _ => "png",
                        };
                        if *name == "flatten-prompt" {
                            h.state_mut()
                                .execute(&ctx, Triggered::once(Command::AddNewLayer));
                        }
                        let path = tmp.path().join(format!("gallery.{ext}"));
                        let tab = h.state_mut().workspace_mut().active_mut().unwrap();
                        tab.format = pnn_io::FileFormat::from_path(&path);
                        tab.path = Some(path);
                        h.state_mut()
                            .execute(&ctx, Triggered::once(Command::AddNewLayer));
                        if *name != "flatten-prompt" {
                            h.state_mut()
                                .execute(&ctx, Triggered::once(Command::Flatten));
                        }
                        press(&mut h, ctrl, Key::S);
                    }
                    (None, "paste-larger") => {
                        h.state_mut().set_clipboard(Box::new(
                            paintnotnet::clipboard::MemoryClipboard {
                                image: Some(paintnotnet::clipboard::ClipImage {
                                    width: 1920,
                                    height: 1080,
                                    rgba: vec![128; 1920 * 1080 * 4],
                                }),
                                text: None,
                            },
                        ));
                        h.state_mut().execute(&ctx, Triggered::once(Command::Paste));
                    }
                    _ => {
                        h.state_mut()
                            .execute(&ctx, Triggered::once(Command::AddNewLayer));
                        h.state_mut().execute(&ctx, Triggered::once(Command::Close));
                    }
                }
                for _ in 0..6 {
                    h.step();
                }
                if let Ok(image) = h.render() {
                    image
                        .save(format!("{dir}/dlg-{name}-{size}-{theme}.png"))
                        .unwrap();
                }
            }
        }
    }
}
