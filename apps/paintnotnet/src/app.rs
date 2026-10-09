use std::time::{Duration, Instant};

use eframe::egui;

use crate::diagnostics::Diagnostics;

/// `--smoke-test N`: show diagnostics, run for N seconds, print a summary line, then quit.
pub struct SmokeTest {
    duration: Duration,
    started: Option<Instant>,
}

impl SmokeTest {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            started: None,
        }
    }
}

pub struct PaintNotNetApp {
    diagnostics: Diagnostics,
    smoke: Option<SmokeTest>,
}

impl PaintNotNetApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, smoke: Option<SmokeTest>) -> Self {
        let mut diagnostics = Diagnostics::default();
        diagnostics.set_visible(cfg!(debug_assertions) || smoke.is_some());
        Self { diagnostics, smoke }
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| diagnostics_chord_pressed(&i.events)) {
            self.diagnostics.set_visible(!self.diagnostics.visible());
        }
    }

    fn run_smoke_test(&mut self, ctx: &egui::Context, now: Instant) {
        let Some(smoke) = &mut self.smoke else { return };
        let started = *smoke.started.get_or_insert(now);
        if now - started < smoke.duration {
            return;
        }
        let env = self.diagnostics.environment();
        let stats = self.diagnostics.stats();
        println!(
            "smoke-test: window_system={} monitor={:?} backend={} adapter={:?} frames={} fps={:.1} mean_ms={:.2} p99_ms={:.2} max_ms={:.2}",
            env.map_or("unknown", |e| e.window_system),
            crate::diagnostics::monitor_label(ctx),
            env.map_or("unknown", |e| e.backend.as_str()),
            env.map_or("unknown", |e| e.adapter.as_str()),
            stats.map_or(0, |s| s.frames),
            stats.map_or(0.0, |s| s.fps()),
            stats.map_or(0.0, |s| s.mean.as_secs_f64() * 1000.0),
            stats.map_or(0.0, |s| s.p99.as_secs_f64() * 1000.0),
            stats.map_or(0.0, |s| s.max.as_secs_f64() * 1000.0),
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        self.smoke = None;
    }
}

impl eframe::App for PaintNotNetApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        self.diagnostics.record_frame(frame, now);

        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.label("paintnotnet: Phase 0 groundwork\nCtrl+Alt+Shift+` toggles diagnostics");
            });
        });

        self.diagnostics.show(&ctx);
        self.run_smoke_test(&ctx, now);
    }
}

/// Ctrl+Alt+Shift+` / ~. Matched on the physical key too, because with Shift held a US
/// layout reports the logical key as `~`, which egui has no `Key` for.
fn diagnostics_chord_pressed(events: &[egui::Event]) -> bool {
    let chord = egui::Modifiers::CTRL | egui::Modifiers::ALT | egui::Modifiers::SHIFT;
    events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. }
                if modifiers.matches_exact(chord)
                    && (*key == egui::Key::Backtick || *physical_key == Some(egui::Key::Backtick))
        )
    })
}
