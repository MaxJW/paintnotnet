//! Frame-timing and GPU/windowing overlay.
//!
//! Toggled with Ctrl+Alt+Shift+` (`~` on a US layout), the chord paint.net uses for its own
//! diagnostic aid.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Frame intervals kept for statistics (about 1.5 s at 165 Hz).
const HISTORY: usize = 240;

/// What we're actually running on, as opposed to what was requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    pub window_system: &'static str,
    pub adapter: String,
    pub backend: String,
    pub driver: String,
}

impl Environment {
    pub fn detect(frame: &eframe::Frame) -> Self {
        let window_system = match frame.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Wayland(_)) => "Wayland",
            Ok(RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)) => "X11",
            Ok(_) => "other",
            Err(_) => "unknown",
        };
        let (adapter, backend, driver) = match frame.wgpu_render_state() {
            Some(state) => {
                let info = state.adapter.get_info();
                (
                    info.name,
                    format!("{:?}", info.backend),
                    format!("{} {}", info.driver, info.driver_info)
                        .trim()
                        .to_owned(),
                )
            }
            None => ("none".into(), "none".into(), String::new()),
        };
        Self {
            window_system,
            adapter,
            backend,
            driver,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameStats {
    pub frames: usize,
    pub mean: Duration,
    pub p99: Duration,
    pub max: Duration,
}

impl FrameStats {
    pub fn fps(&self) -> f64 {
        1.0 / self.mean.as_secs_f64()
    }
}

#[derive(Default)]
pub struct Diagnostics {
    visible: bool,
    last_frame: Option<Instant>,
    intervals: VecDeque<Duration>,
    environment: Option<Environment>,
}

impl Diagnostics {
    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        // egui only repaints on input while the overlay is hidden, so old intervals are idle
        // gaps, not frame times.
        self.last_frame = None;
        self.intervals.clear();
    }

    pub fn environment(&self) -> Option<&Environment> {
        self.environment.as_ref()
    }

    /// Call once per frame.
    pub fn record_frame(&mut self, frame: &eframe::Frame, now: Instant) {
        if self.environment.is_none() {
            let env = Environment::detect(frame);
            log::info!(
                "running on {} via {} ({}), driver: {}",
                env.window_system,
                env.backend,
                env.adapter,
                env.driver
            );
            self.environment = Some(env);
        }
        if !self.visible {
            return;
        }
        if let Some(last) = self.last_frame {
            if self.intervals.len() == HISTORY {
                self.intervals.pop_front();
            }
            self.intervals.push_back(now - last);
        }
        self.last_frame = Some(now);
    }

    pub fn stats(&self) -> Option<FrameStats> {
        if self.intervals.is_empty() {
            return None;
        }
        let mut sorted: Vec<Duration> = self.intervals.iter().copied().collect();
        sorted.sort_unstable();
        let total: Duration = sorted.iter().sum();
        let p99_index = (sorted.len() * 99).div_ceil(100).saturating_sub(1);
        Some(FrameStats {
            frames: sorted.len(),
            mean: total / sorted.len() as u32,
            p99: sorted[p99_index],
            max: *sorted.last().expect("non-empty"),
        })
    }

    pub fn show(&self, ctx: &egui::Context) {
        if !self.visible {
            return;
        }
        // Keep frames coming so the timings measure the display, not idle time.
        ctx.request_repaint();

        egui::Area::new(egui::Id::new("diagnostics"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -8.0])
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    egui::Grid::new("diagnostics_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            if let Some(env) = &self.environment {
                                row(ui, "Window system", env.window_system);
                                row(ui, "GPU", &env.adapter);
                                row(ui, "Backend", &env.backend);
                                row(ui, "Driver", &env.driver);
                            }
                            row(ui, "Monitor", &monitor_label(ctx));
                            match self.stats() {
                                Some(s) => {
                                    row(ui, "FPS", &format!("{:.1}", s.fps()));
                                    row(ui, "Frame mean", &ms(s.mean));
                                    row(ui, "Frame p99", &ms(s.p99));
                                    row(ui, "Frame max", &ms(s.max));
                                }
                                None => row(ui, "FPS", "…"),
                            }
                        });
                });
            });
    }
}

/// The monitor the window is on, as winit reports it, e.g. `2560x1440 @1x`.
/// Per-stage timings for `PNN_PROFILE=1` smoke tests: total time and count per stage name.
static PROFILE: std::sync::Mutex<Vec<(&'static str, Duration, u32)>> =
    std::sync::Mutex::new(Vec::new());

/// Time `f` as stage `name` when `PNN_PROFILE` is set (otherwise just run it).
pub fn timed<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("PNN_PROFILE").is_some()) {
        return f();
    }
    let t = Instant::now();
    let r = f();
    let d = t.elapsed();
    if let Ok(mut p) = PROFILE.lock() {
        match p.iter_mut().find(|(n, _, _)| *n == name) {
            Some(e) => {
                e.1 += d;
                e.2 += 1;
            }
            None => p.push((name, d, 1)),
        }
    }
    r
}

/// "stage=mean ms (count)" for every timed stage, for the smoke-test summary.
pub fn profile_summary() -> String {
    PROFILE.lock().map_or_else(
        |_| String::new(),
        |p| {
            p.iter()
                .map(|(n, d, c)| format!("{n}={:.2}ms({c})", d.as_secs_f64() * 1e3 / *c as f64))
                .collect::<Vec<_>>()
                .join(" ")
        },
    )
}

pub fn monitor_label(ctx: &egui::Context) -> String {
    ctx.input(|i| {
        let viewport = i.viewport();
        match (viewport.monitor_size, viewport.native_pixels_per_point) {
            (Some(size), Some(scale)) => format!("{}x{} @{scale}x", size.x * scale, size.y * scale),
            (Some(size), None) => format!("{}x{} points", size.x, size.y),
            _ => "unknown".to_owned(),
        }
    })
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(label);
    ui.monospace(value);
    ui.end_row();
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_intervals(ms: &[u64]) -> Diagnostics {
        Diagnostics {
            intervals: ms.iter().map(|&m| Duration::from_millis(m)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn no_stats_without_frames() {
        assert!(Diagnostics::default().stats().is_none());
    }

    #[test]
    fn stats_mean_p99_max() {
        // 99 frames at 6 ms and one 30 ms hitch.
        let mut ms = vec![6; 99];
        ms.push(30);
        let s = with_intervals(&ms).stats().unwrap();
        assert_eq!(s.frames, 100);
        assert_eq!(s.max, Duration::from_millis(30));
        assert_eq!(s.p99, Duration::from_millis(6));
        assert_eq!(s.mean, Duration::from_micros(6240));
    }

    #[test]
    fn toggling_resets_history() {
        let mut d = with_intervals(&[6, 6, 6]);
        d.set_visible(true);
        assert!(d.stats().is_none());
    }
}
