//! A log of what happens in a session, for finding lag and hangs after the fact: the usual
//! log lines also go to `~/.local/state/paintnotnet/paintnotnet.log` (the previous session's
//! is kept as `paintnotnet.log.1`), plus commands, tool presses and releases, slow frames
//! with what was going on, background jobs, memory every few seconds, and a watchdog that
//! notes when the UI thread has been stuck in one frame for over a second.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where the log goes: `$XDG_STATE_HOME/paintnotnet`, else `~/.local/state/paintnotnet`.
pub fn log_dir() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(state.join("paintnotnet"))
}

/// Log lines to standard error and, when it can be opened, the session log file.
struct Tee {
    file: Option<std::fs::File>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(f) = &mut self.file {
            let _ = f.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(f) = &mut self.file {
            f.flush()?;
        }
        Ok(())
    }
}

/// Set up logging. `to_file`: also write the session log (not for the AI helper process).
pub fn init(to_file: bool) {
    let mut b =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(
            "info,wgpu_core=warn,wgpu_hal=warn,naga=warn,zbus=warn,tracing=warn",
        ));
    let file = to_file.then(log_dir).flatten().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("paintnotnet.log");
        let _ = std::fs::rename(&path, dir.join("paintnotnet.log.1"));
        std::fs::File::create(path).ok()
    });
    b.target(env_logger::Target::Pipe(Box::new(Tee { file })));
    b.init();
}

/// Resident memory of this process in MB (from /proc).
pub fn resident_mb() -> u64 {
    let pages = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0);
    pages * 4096 / 1_000_000
}

/// Watches the UI thread: frames that take long, and frames that don't end at all.
pub struct Watchdog {
    epoch: Instant,
    /// Milliseconds since `epoch` when the current frame started (0 = not in a frame).
    frame_started: Arc<AtomicU64>,
    activity: Arc<Mutex<String>>,
    frame_start: Instant,
    last_memory: Instant,
}

impl Watchdog {
    pub fn start() -> Self {
        let epoch = Instant::now();
        let frame_started = Arc::new(AtomicU64::new(0));
        let activity = Arc::new(Mutex::new(String::new()));
        let (started, act) = (frame_started.clone(), activity.clone());
        let _ = std::thread::Builder::new()
            .name("watchdog".into())
            .spawn(move || {
                let mut reported = 0u64;
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let s = started.load(Ordering::Relaxed);
                    if s == 0 {
                        continue;
                    }
                    let now = epoch.elapsed().as_millis() as u64;
                    let stuck = now.saturating_sub(s);
                    // Once per second of being stuck in the same frame.
                    if stuck >= 1000 && stuck / 1000 > reported {
                        reported = stuck / 1000;
                        let what = act.lock().map(|a| a.clone()).unwrap_or_default();
                        log::warn!(
                            "watchdog: UI thread stuck in one frame for {:.1} s ({what}); memory {} MB",
                            stuck as f64 / 1000.0,
                            resident_mb()
                        );
                    } else if stuck < 1000 {
                        reported = 0;
                    }
                }
            });
        Self {
            epoch,
            frame_started,
            activity,
            frame_start: Instant::now(),
            last_memory: Instant::now(),
        }
    }

    /// A frame starts doing `activity` (what the app is busy with, for the log).
    pub fn frame_begin(&mut self, activity: String) {
        self.frame_start = Instant::now();
        let ms = self.epoch.elapsed().as_millis() as u64;
        self.frame_started.store(ms.max(1), Ordering::Relaxed);
        if let Ok(mut a) = self.activity.lock() {
            *a = activity;
        }
    }

    /// The frame ended: log it if it was slow, and memory now and then.
    pub fn frame_end(&mut self) {
        self.frame_started.store(0, Ordering::Relaxed);
        let took = self.frame_start.elapsed();
        if took >= Duration::from_millis(50) {
            let what = self.activity.lock().map(|a| a.clone()).unwrap_or_default();
            log::warn!("slow frame: {:.0} ms ({what})", took.as_secs_f64() * 1e3);
        }
        if self.last_memory.elapsed() >= Duration::from_secs(10) {
            self.last_memory = Instant::now();
            log::info!("memory {} MB", resident_mb());
        }
    }
}
