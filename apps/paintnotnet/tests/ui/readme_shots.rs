//! The README's screenshots, rendered from the real app with artwork made in it (no outside
//! images). Opt in, with an absolute path (tests run in the crate's folder):
//! `PNN_README_SHOTS=$PWD/docs/images cargo test -p paintnotnet --test ui readme_shots`, then
//! `optipng -o2 docs/images/*.png`.

use eframe::egui::{Event, Key, Modifiers, ThemePreference, vec2};
use egui_kittest::Harness;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Triggered};
use pnn_core::{BlendMode, ColorBgra, Document, Layer, Selection, Surface};

const W: u32 = 1600;
const H: u32 = 1000;

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn rgb(c: [f32; 3], a: u8) -> ColorBgra {
    ColorBgra::from_rgba(c[0] as u8, c[1] as u8, c[2] as u8, a)
}

/// A dusk landscape in four layers: sky, sun, far and near hills.
fn scene() -> Document {
    let mut sky = Surface::new_transparent(W, H);
    let stops = [
        (0.0, [22.0, 26.0, 70.0]),
        (0.45, [120.0, 64.0, 150.0]),
        (0.72, [246.0, 128.0, 112.0]),
        (1.0, [255.0, 196.0, 130.0]),
    ];
    for y in 0..H {
        let t = y as f32 / H as f32;
        let i = stops.iter().rposition(|s| s.0 <= t).unwrap_or(0).min(2);
        let (a, b) = (stops[i], stops[i + 1]);
        let c = rgb(mix(a.1, b.1, (t - a.0) / (b.0 - a.0)), 255);
        for x in 0..W {
            sky.set(x, y, c);
        }
    }
    let mut sun = Surface::new_transparent(W, H);
    let (cx, cy, r) = (1080.0f32, 600.0f32, 110.0f32);
    for y in 300..900 {
        for x in 700..1460 {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let a = if d <= r {
                255.0
            } else {
                (1.0 - (d - r) / 260.0).max(0.0).powi(2) * 170.0
            };
            if a > 0.5 {
                let c = if d <= r {
                    [255.0, 238.0, 196.0]
                } else {
                    [255.0, 190.0, 140.0]
                };
                sun.set(x, y, rgb(c, a as u8));
            }
        }
    }
    let hills = |base: f32, amp: [f32; 2], freq: [f32; 2], top: [f32; 3], bottom: [f32; 3]| {
        let mut s = Surface::new_transparent(W, H);
        for x in 0..W {
            let xf = x as f32;
            let ridge = base + amp[0] * (xf * freq[0]).sin() + amp[1] * (xf * freq[1] + 1.3).sin();
            for y in (ridge.max(0.0) as u32)..H {
                let t = (y as f32 - ridge) / (H as f32 - ridge).max(1.0);
                // A soft top edge.
                let a = ((y as f32 - ridge) * 1.5).clamp(0.0, 1.0) * 255.0;
                s.set(x, y, rgb(mix(top, bottom, t), a as u8));
            }
        }
        s
    };
    let far = hills(
        660.0,
        [46.0, 22.0],
        [0.0042, 0.011],
        [128.0, 70.0, 128.0],
        [70.0, 40.0, 92.0],
    );
    let near = hills(
        780.0,
        [58.0, 20.0],
        [0.0031, 0.0093],
        [52.0, 30.0, 70.0],
        [18.0, 12.0, 32.0],
    );
    let mut doc = Document::from_layers(
        W,
        H,
        vec![
            Layer::new("Sky", sky),
            Layer::new("Sun", sun),
            Layer::new("Far hills", far),
            Layer::new("Near hills", near),
        ],
    );
    doc.set_active_index(3);
    doc
}

fn harness(theme: ThemePreference) -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder()
        .with_size(vec2(1440.0, 900.0))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.ctx.set_theme(theme);
    h.state_mut().workspace_mut().add(scene(), None, None);
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.path = Some("dusk.pdn".into());
    tab.format = Some(pnn_io::FileFormat::Pdn);
    let ctx = h.ctx.clone();
    if h.state().diagnostics().visible() {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
    }
    for _ in 0..3 {
        h.step();
    }
    h
}

fn run(h: &mut Harness<'static, PaintNotNetApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Triggered::once(c));
    h.step();
}

/// Wait for background work (effects, previews) to finish.
fn settle(h: &mut Harness<'static, PaintNotNetApp>) {
    for _ in 0..400 {
        h.step();
        let busy = h.state().click_job_running()
            || h.state().doc_job_running()
            || h.state_mut()
                .effect_mut()
                .is_some_and(|e| !e.preview_done());
        if !busy {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    for _ in 0..4 {
        h.step();
    }
}

/// A Clouds layer (the real effect) screened over the sky, as a user would make it.
fn add_clouds(h: &mut Harness<'static, PaintNotNetApp>) {
    let ctx = h.ctx.clone();
    // Above the sky: select it, then add the layer.
    h.state_mut()
        .workspace_mut()
        .active_mut()
        .unwrap()
        .doc
        .set_active_index(0);
    run(h, Command::AddNewLayer);
    let clouds = pnn_effects::effects_in(pnn_effects::Category::Render)
        .find(|(_, e)| e.name == "Clouds")
        .map(|(id, _)| Command::Effect(id))
        .unwrap();
    run(h, clouds);
    if let Some(e) = h.state_mut().effect_mut() {
        use pnn_effects::framework::Value;
        e.set_values(
            &ctx,
            vec![
                Value::Int(420),
                Value::Float(0.58),
                Value::Choice(0),
                Value::Int(11),
            ],
        );
    }
    settle(h);
    h.event(Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    settle(h);
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    let i = tab.doc.active_index();
    tab.doc.modify_layer(i, None, |l| {
        l.name = "Clouds".into();
        l.blend_mode = BlendMode::Screen;
        l.opacity = 110;
    });
    tab.commit(
        "Layer Properties",
        paintnotnet::icons::Icon::LayerProperties,
    );
    tab.doc.set_active_index(4);
    settle(h);
}

fn save(h: &mut Harness<'static, PaintNotNetApp>, dir: &str, name: &str) {
    settle(h);
    let image = h.render().expect("render");
    image.save(format!("{dir}/{name}.png")).unwrap();
}

#[test]
fn readme_shots() {
    let _gpu = crate::common::gpu_lock();
    let Ok(dir) = std::env::var("PNN_README_SHOTS") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();

    // The main window: layers, history and a selection around the sun.
    let mut h = harness(ThemePreference::Dark);
    add_clouds(&mut h);
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.doc
        .set_selection(Selection::ellipse(940.0, 460.0, 1220.0, 740.0));
    tab.commit("Ellipse Select", paintnotnet::icons::Icon::EllipseSelect);
    h.state_mut().set_tool(pnn_tools::ToolKind::EllipseSelect);
    save(&mut h, &dir, "main-window");

    // An adjustment with its live preview, in the light theme.
    let mut h = harness(ThemePreference::Light);
    add_clouds(&mut h);
    h.state_mut()
        .workspace_mut()
        .active_mut()
        .unwrap()
        .doc
        .set_active_index(0);
    run(
        &mut h,
        Command::Adjustment(pnn_effects::Adjustment::HueSaturation),
    );
    {
        let ctx = h.ctx.clone();
        use pnn_effects::framework::Value;
        if let Some(e) = h.state_mut().effect_mut() {
            e.set_values(&ctx, vec![Value::Int(-38), Value::Int(125), Value::Int(0)]);
        }
    }
    save(&mut h, &dir, "adjustment");

    // The command palette.
    let mut h = harness(ThemePreference::Dark);
    add_clouds(&mut h);
    run(&mut h, Command::CommandPalette);
    h.event(Event::Text("blur".into()));
    save(&mut h, &dir, "command-palette");

    // The Effects menu.
    let mut h = harness(ThemePreference::Dark);
    add_clouds(&mut h);
    run(
        &mut h,
        Command::OpenMenu(paintnotnet::commands::MenuId::Effects),
    );
    save(&mut h, &dir, "effects-menu");
}
