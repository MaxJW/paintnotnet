//! Phase 19: the lag audit. Drives the app through its tools, selections, effects,
//! adjustments, dialogs, layer and image commands, undo/redo, clipboard and saving on a large
//! image, timing every frame on the UI thread.
//!
//! Opt in with an image (not committed), in a release build:
//! `PNN_PERF_IMAGE=photo.jpg cargo test --release -p paintnotnet --test ui perf_audit -- --ignored --nocapture`
//! `PNN_PERF_ONLY=<a,b,…>` runs only the actions whose name contains one of them;
//! `PNN_PERF_FRAMES=<name>` prints every frame's time for actions whose name contains it.
//! Each action prints its frames, the slowest frame, the UI thread's total and the wall time
//! (which includes waiting for background work). "LAG" marks a frame over 100 ms; "slow"
//! one over 16 ms.

use crate::common::ctrl;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Panel, Triggered};
use pnn_tools::ToolKind;
use std::time::{Duration, Instant};

struct Audit {
    h: Harness<'static, PaintNotNetApp>,
    frames: Vec<Duration>,
    rows: Vec<(String, usize, Duration, Duration, Duration)>,
    only: Option<String>,
}

impl Audit {
    fn step(&mut self) {
        let t = Instant::now();
        self.h.step();
        self.frames.push(t.elapsed());
    }

    fn event(&mut self, e: Event) {
        self.h.event(e);
        self.step();
    }

    /// Run `f` as one timed action.
    fn action(&mut self, name: &str, f: impl FnOnce(&mut Audit)) {
        if self
            .only
            .as_ref()
            .is_some_and(|o| !o.split(',').any(|part| name.contains(part)))
        {
            return;
        }
        self.frames.clear();
        let t = Instant::now();
        f(self);
        let wall = t.elapsed();
        let worst = self.frames.iter().max().copied().unwrap_or_default();
        let total: Duration = self.frames.iter().sum();
        let flag = if worst > Duration::from_millis(100) {
            "LAG"
        } else if worst > Duration::from_millis(16) {
            "slow"
        } else {
            ""
        };
        println!(
            "{name:<40} frames {:>4}  worst {:>8.1} ms  ui {:>8.1} ms  wall {:>8.1} ms  {flag}",
            self.frames.len(),
            worst.as_secs_f64() * 1e3,
            total.as_secs_f64() * 1e3,
            wall.as_secs_f64() * 1e3
        );
        if std::env::var("PNN_PERF_FRAMES").is_ok_and(|f| name.contains(f.as_str())) {
            let ms: Vec<String> = self
                .frames
                .iter()
                .map(|d| format!("{:.1}", d.as_secs_f64() * 1e3))
                .collect();
            println!("    frames (ms): {}", ms.join(" "));
        }
        self.rows
            .push((name.to_owned(), self.frames.len(), worst, total, wall));
    }

    /// Step until `done`, letting background work run between frames (at most `limit`).
    fn settle(
        &mut self,
        limit: Duration,
        mut done: impl FnMut(&mut Harness<'static, PaintNotNetApp>) -> bool,
    ) {
        let start = Instant::now();
        loop {
            self.step();
            if done(&mut self.h) || start.elapsed() > limit {
                break;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    /// A few frames, for animations and repaints to run out.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    fn command(&mut self, c: Command) {
        // Commands wait while background work runs (they'd be refused).
        self.settle(Duration::from_secs(20), |h| {
            !h.state().click_job_running() && !h.state().doc_job_running()
        });
        let ctx = self.h.ctx.clone();
        let t = Instant::now();
        self.h.state_mut().execute(&ctx, Triggered::once(c));
        // The command itself runs outside a frame here; count it as one.
        self.frames.push(t.elapsed());
        self.frames(3);
        // Image commands on a large image finish in the background; the next action waits.
        self.settle(Duration::from_secs(20), |h| {
            !h.state().click_job_running() && !h.state().doc_job_running()
        });
    }

    fn key(&mut self, modifiers: Modifiers, key: Key) {
        self.h.event(Event::ModifiersChanged(modifiers));
        for pressed in [true, false] {
            self.h.event(Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            });
        }
        self.h.event(Event::ModifiersChanged(Modifiers::NONE));
        for _ in 0..5 {
            self.step();
        }
    }

    fn screen(&self, p: (f64, f64)) -> Pos2 {
        let rect = self.h.state().canvas().rect.unwrap();
        let view = self.h.state().workspace().active().unwrap().view.unwrap();
        let ppp = self.h.ctx.pixels_per_point() as f64;
        let vp = [rect.width() as f64 * ppp, rect.height() as f64 * ppp];
        let s = view.image_to_screen([p.0, p.1], vp);
        Pos2::new(
            rect.min.x + (s[0] / ppp) as f32,
            rect.min.y + (s[1] / ppp) as f32,
        )
    }

    /// Press, move through `points` (image px, a frame each), release.
    fn drag(&mut self, button: PointerButton, modifiers: Modifiers, points: &[(f64, f64)]) {
        let first = self.screen(points[0]);
        self.h.event(Event::ModifiersChanged(modifiers));
        self.event(Event::PointerMoved(first));
        self.event(Event::PointerButton {
            pos: first,
            button,
            pressed: true,
            modifiers,
        });
        for &p in &points[1..] {
            let s = self.screen(p);
            self.event(Event::PointerMoved(s));
        }
        let last = self.screen(*points.last().unwrap());
        self.event(Event::PointerButton {
            pos: last,
            button,
            pressed: false,
            modifiers,
        });
        self.h.event(Event::ModifiersChanged(Modifiers::NONE));
        self.frames(3);
    }

    fn click(&mut self, p: (f64, f64)) {
        self.drag(PointerButton::Primary, Modifiers::NONE, &[p]);
        // A wand or bucket click on a large image finishes in the background.
        self.settle(Duration::from_secs(10), |h| !h.state().click_job_running());
    }

    fn size(&self) -> (f64, f64) {
        let d = &self.h.state().workspace().active().unwrap().doc;
        (d.width() as f64, d.height() as f64)
    }

    /// A stroke across the middle of the image, `n` moves.
    fn stroke(&self, n: usize) -> Vec<(f64, f64)> {
        let (w, h) = self.size();
        (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                (w * (0.2 + 0.6 * t), h * (0.5 + 0.2 * (t * 6.0).sin()))
            })
            .collect()
    }

    fn undo_redo(&mut self, name: &str) {
        self.action(&format!("{name}: undo"), |a| a.key(ctrl(), Key::Z));
        self.action(&format!("{name}: redo"), |a| a.key(ctrl(), Key::Y));
        self.action(&format!("{name}: undo again"), |a| a.key(ctrl(), Key::Z));
    }

    fn effect_done(h: &mut Harness<'static, PaintNotNetApp>) -> bool {
        h.state_mut().effect_mut().is_none_or(|e| e.preview_done()) && !h.state().doc_job_running()
    }

    fn ok(&mut self) {
        if let Some(b) = self.h.query_by_label("OK") {
            b.click();
        }
        self.frames(4);
    }
}

#[test]
#[ignore = "needs PNN_PERF_IMAGE and a release build"]
fn perf_audit() {
    let Some(path) = std::env::var_os("PNN_PERF_IMAGE") else {
        return;
    };
    let t = Instant::now();
    let doc = pnn_io::load(std::path::Path::new(&path)).expect("image loads");
    println!(
        "load {}×{}: {:.0} ms",
        doc.width(),
        doc.height(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let h = Harness::builder()
        .with_size(vec2(1600.0, 1000.0))
        .wgpu()
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    let mut a = Audit {
        h,
        frames: Vec::new(),
        rows: Vec::new(),
        only: std::env::var("PNN_PERF_ONLY").ok(),
    };
    // Not the user's desktop clipboard: the audit copies the whole image.
    a.h.state_mut()
        .set_clipboard(Box::new(paintnotnet::clipboard::MemoryClipboard::default()));
    let ctx = a.h.ctx.clone();
    if a.h.state().diagnostics().visible() {
        a.h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
    }
    let only = a.only.take();
    a.action("open: first frames (upload)", |a| {
        a.h.state_mut().workspace_mut().add(doc, None, None);
        a.frames(10);
    });
    a.only = only;

    // View.
    a.action("view: zoom in ×3", |a| {
        for _ in 0..3 {
            a.key(ctrl(), Key::Equals);
        }
    });
    a.action("view: pan (middle drag)", |a| {
        let (w, h) = a.size();
        let pts: Vec<_> = (0..20)
            .map(|i| (w * 0.5 + i as f64 * 5.0, h * 0.5))
            .collect();
        a.drag(PointerButton::Middle, Modifiers::NONE, &pts);
    });
    a.action("view: zoom out ×3", |a| {
        for _ in 0..3 {
            a.key(ctrl(), Key::Minus);
        }
    });
    a.action("view: fit to window", |a| a.key(ctrl(), Key::B));
    a.action("view: toggle panels", |a| {
        for p in Panel::ALL {
            a.command(Command::ToggleWindow(p));
        }
        for p in Panel::ALL {
            a.command(Command::ToggleWindow(p));
        }
    });
    a.action("idle (30 frames)", |a| a.frames(30));

    // Painting tools: a stroke, then undo/redo.
    for tool in [
        ToolKind::Paintbrush,
        ToolKind::Pencil,
        ToolKind::Eraser,
        ToolKind::Recolor,
    ] {
        a.h.state_mut().set_tool(tool);
        let name = format!("{tool:?} stroke");
        a.action(&name, |a| {
            let s = a.stroke(40);
            a.drag(PointerButton::Primary, Modifiers::NONE, &s);
        });
        a.undo_redo(&name);
    }
    a.h.state_mut().set_tool(ToolKind::CloneStamp);
    a.action("CloneStamp source + stroke", |a| {
        let (w, h) = a.size();
        a.drag(PointerButton::Primary, ctrl(), &[(w * 0.3, h * 0.3)]);
        let s = a.stroke(40);
        a.drag(PointerButton::Primary, Modifiers::NONE, &s);
    });
    a.undo_redo("CloneStamp");
    a.h.state_mut().set_tool(ToolKind::PaintBucket);
    a.action("PaintBucket click", |a| {
        let (w, h) = a.size();
        a.click((w * 0.5, h * 0.5));
    });
    a.undo_redo("PaintBucket");
    a.h.state_mut().set_tool(ToolKind::Gradient);
    a.action("Gradient drag + finish", |a| {
        let (w, h) = a.size();
        let pts: Vec<_> = (0..20)
            .map(|i| (w * (0.2 + 0.03 * i as f64), h * (0.3 + 0.02 * i as f64)))
            .collect();
        a.drag(PointerButton::Primary, Modifiers::NONE, &pts);
        a.command(Command::Finish);
    });
    a.undo_redo("Gradient");
    for tool in [ToolKind::LineCurve, ToolKind::Shapes] {
        a.h.state_mut().set_tool(tool);
        let name = format!("{tool:?} drag + finish");
        a.action(&name, |a| {
            let (w, h) = a.size();
            let pts: Vec<_> = (0..20)
                .map(|i| (w * (0.3 + 0.02 * i as f64), h * (0.3 + 0.02 * i as f64)))
                .collect();
            a.drag(PointerButton::Primary, Modifiers::NONE, &pts);
            a.command(Command::Finish);
        });
        a.undo_redo(&name);
    }
    a.h.state_mut().set_tool(ToolKind::Text);
    a.action("Text: click, type, finish", |a| {
        let (w, h) = a.size();
        a.click((w * 0.4, h * 0.4));
        for c in "Hello paintnotnet".chars() {
            a.event(Event::Text(c.to_string()));
        }
        a.command(Command::Finish);
    });
    a.undo_redo("Text");
    a.h.state_mut().set_tool(ToolKind::ColorPicker);
    a.action("ColorPicker click", |a| {
        let (w, h) = a.size();
        a.click((w * 0.6, h * 0.6));
    });

    // Selections.
    for tool in [
        ToolKind::RectangleSelect,
        ToolKind::EllipseSelect,
        ToolKind::LassoSelect,
    ] {
        a.h.state_mut().set_tool(tool);
        let name = format!("{tool:?} drag");
        a.action(&name, |a| {
            let s = a.stroke(30);
            a.drag(PointerButton::Primary, Modifiers::NONE, &s);
        });
    }
    a.h.state_mut().set_tool(ToolKind::MagicWand);
    a.action("MagicWand click (photo: complex)", |a| {
        let (w, h) = a.size();
        a.click((w * 0.5, h * 0.5));
    });
    a.action("idle with a complex selection (30 frames)", |a| {
        a.frames(30)
    });
    a.action("Feather Selection 10 px (complex): open + OK", |a| {
        a.command(Command::FeatherSelection);
        if let Some(d) = a.h.state_mut().doc_dialog_mut()
            && let paintnotnet::doc_dialogs::Kind::FeatherSelection(f) = &mut d.kind
        {
            f.radius = 10.0;
        }
        a.frames(5);
        a.ok();
        a.frames(5);
    });
    a.undo_redo("Feather Selection (complex)");
    a.h.state_mut().set_tool(ToolKind::MoveSelection);
    a.action("MoveSelection drag (complex)", |a| {
        let (w, h) = a.size();
        let pts: Vec<_> = (0..15)
            .map(|i| (w * 0.5 + i as f64 * 8.0, h * 0.5))
            .collect();
        a.drag(PointerButton::Primary, Modifiers::NONE, &pts);
    });
    a.h.state_mut().set_tool(ToolKind::MoveSelectedPixels);
    a.action("MoveSelectedPixels drag (complex)", |a| {
        let (w, h) = a.size();
        let pts: Vec<_> = (0..15)
            .map(|i| (w * 0.5 + i as f64 * 8.0, h * 0.5))
            .collect();
        a.drag(PointerButton::Primary, Modifiers::NONE, &pts);
        a.command(Command::Finish);
    });
    a.undo_redo("MoveSelectedPixels");
    a.action("MoveSelectedPixels rotate (complex)", |a| {
        let (w, h) = a.size();
        // From outside the selection's bounds: a rotation.
        let pts: Vec<_> = (0..12)
            .map(|i| (w * 0.02 + i as f64 * 4.0, h * 0.02 + i as f64 * 9.0))
            .collect();
        a.drag(PointerButton::Primary, Modifiers::NONE, &pts);
        a.command(Command::Finish);
    });
    a.undo_redo("MoveSelectedPixels rotate");
    a.action("Invert Selection", |a| a.command(Command::InvertSelection));
    a.action("Erase Selection", |a| a.command(Command::EraseSelection));
    a.undo_redo("Erase Selection");
    a.action("Select All", |a| a.command(Command::SelectAll));
    a.action("Fill Selection", |a| a.command(Command::FillSelection));
    a.undo_redo("Fill Selection");
    a.action("Deselect", |a| a.command(Command::Deselect));
    a.h.state_mut().set_tool(ToolKind::EllipseSelect);
    a.action("ellipse for effects", |a| {
        let (w, h) = a.size();
        a.drag(
            PointerButton::Primary,
            Modifiers::NONE,
            &[
                (w * 0.25, h * 0.25),
                (w * 0.5, h * 0.5),
                (w * 0.75, h * 0.75),
            ],
        );
    });
    a.action("Deselect", |a| a.command(Command::Deselect));

    // Clipboard (a selection first).
    a.action("Select All + Copy", |a| {
        a.command(Command::SelectAll);
        a.command(Command::Copy);
        a.frames(5);
    });
    a.action("Paste into new layer", |a| {
        a.command(Command::PasteIntoNewLayer);
        a.frames(5);
        a.command(Command::Finish);
    });
    a.undo_redo("Paste into new layer");
    a.action("Deselect", |a| a.command(Command::Deselect));

    // Layers.
    for (name, c) in [
        ("Add New Layer", Command::AddNewLayer),
        ("Duplicate Layer", Command::DuplicateLayer),
        ("Flip Layer Horizontal", Command::FlipLayerHorizontal),
        ("Rotate Layer 180", Command::RotateLayer180),
        ("Move Layer Down", Command::MoveLayerDown),
        ("Merge Layer Down", Command::MergeLayerDown),
        ("Toggle Layer Visibility", Command::ToggleLayerVisibility),
        (
            "Toggle Layer Visibility back",
            Command::ToggleLayerVisibility,
        ),
    ] {
        a.action(name, |a| a.command(c));
    }
    a.action("Layer Properties: open + OK", |a| {
        a.command(Command::LayerProperties);
        a.ok();
    });

    // Image.
    for (name, c) in [
        ("Flip Image Horizontal", Command::FlipImageHorizontal),
        ("Rotate Image 90°", Command::RotateImage90Clockwise),
        (
            "Rotate Image 90° back",
            Command::RotateImage90CounterClockwise,
        ),
        ("Flatten", Command::Flatten),
    ] {
        a.action(name, |a| a.command(c));
    }
    a.undo_redo("Flatten");
    // Document dialogs with settings that change the image.
    for (name, c) in [
        ("Resize 50%: open + OK", Command::Resize),
        ("Canvas Size 120%: open + OK", Command::CanvasSize),
        ("Rotate/Zoom 30°: open + OK", Command::RotateZoom),
    ] {
        a.action(name, |a| {
            a.command(c);
            if let Some(d) = a.h.state_mut().doc_dialog_mut() {
                use paintnotnet::doc_dialogs::Kind;
                match &mut d.kind {
                    Kind::Resize(r) => r.fields.percent = 50,
                    Kind::CanvasSize(cs) => cs.fields.percent = 120,
                    Kind::RotateZoom(rz) => rz.angle = 30.0,
                    _ => {}
                }
            }
            a.frames(3);
            a.settle(Duration::from_secs(30), |h| !h.state().doc_job_running());
            a.ok();
            a.settle(Duration::from_secs(30), |h| {
                h.state_mut().doc_dialog_mut().is_none() && !h.state().doc_job_running()
            });
        });
        a.undo_redo(name);
    }

    // Adjustments and effects: open, preview to the end, OK, undo.
    let mut targets: Vec<(String, Command)> = pnn_effects::Adjustment::ALL
        .iter()
        .map(|&adj| {
            (
                format!("Adjustment {}", adj.name()),
                Command::Adjustment(adj),
            )
        })
        .collect();
    for (i, e) in pnn_effects::EFFECTS.iter().enumerate() {
        let c = Command::Effect(pnn_effects::EffectId(i as u16));
        if c.implemented() && e.name != "Remove Background" {
            targets.push((format!("Effect {}", e.name), c));
        }
    }
    for (name, c) in targets {
        a.action(&format!("{name}: open + preview"), |a| {
            a.command(c);
            a.settle(Duration::from_secs(60), Audit::effect_done);
        });
        a.action(&format!("{name}: OK"), |a| {
            if a.h.state_mut().effect_mut().is_some() || a.h.state_mut().doc_dialog_mut().is_some()
            {
                a.ok();
            }
            a.frames(3);
        });
        a.action(&format!("{name}: undo"), |a| a.key(ctrl(), Key::Z));
    }

    // Saving (in the background) in each format.
    let dir = tempfile::tempdir().unwrap();
    for ext in ["png", "jpg", "webp", "ora", "pdn", "tiff"] {
        let path = dir.path().join(format!("audit.{ext}"));
        a.action(&format!("Save .{ext}"), |a| {
            let tab = a.h.state_mut().workspace_mut().active_mut().unwrap();
            tab.path = Some(path.clone());
            tab.format = pnn_io::FileFormat::from_path(&path);
            a.key(ctrl(), Key::S);
            a.frames(3);
            for label in ["Flatten", "OK", "Save"] {
                if let Some(b) = a.h.query_by_label(label) {
                    b.click();
                    a.frames(3);
                }
            }
            a.settle(Duration::from_secs(60), |h| {
                h.state()
                    .workspace()
                    .active()
                    .is_some_and(|t| !t.is_dirty())
            });
        });
        // Dirty again for the next save.
        a.action("(paint a dot)", |a| {
            a.h.state_mut().set_tool(ToolKind::Pencil);
            let (w, h) = a.size();
            a.click((w * 0.1, h * 0.1));
        });
    }

    println!("\nSlowest frames:");
    let mut rows = a.rows.clone();
    rows.sort_by_key(|r| std::cmp::Reverse(r.2));
    for (name, frames, worst, ui, wall) in rows.iter().take(25) {
        println!(
            "  {:>8.1} ms  {name} ({frames} frames, ui {:.0} ms, wall {:.0} ms)",
            worst.as_secs_f64() * 1e3,
            ui.as_secs_f64() * 1e3,
            wall.as_secs_f64() * 1e3
        );
    }
}
