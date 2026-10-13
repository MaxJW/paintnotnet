//! Autosave and crash recovery (a paintnotnet addition; paint.net 5 has neither).
//!
//! Every `interval`, each document with unsaved changes since its last autosave is written as
//! OpenRaster to `<state>/recovery/<pid>/<key>.ora`, beside a `<key>.txt` holding its title
//! and original path, on a worker thread. Saving or closing a document removes its files; a
//! clean exit removes the whole folder. A folder whose process isn't running any more is what
//! a crash leaves behind: at startup those images are offered back.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pnn_core::Document;

/// Default state directory: `$XDG_STATE_HOME/paintnotnet` or `~/.local/state/paintnotnet`.
pub fn default_state_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join("paintnotnet"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/paintnotnet"))
}

pub struct Autosave {
    /// `<state>/recovery`.
    root: PathBuf,
    /// This process's folder.
    dir: PathBuf,
    interval: Duration,
    last: Instant,
    /// Document revision last written per tab key.
    written: std::collections::HashMap<u64, u64>,
}

/// An image left behind by a crashed session.
#[derive(Clone, Debug, PartialEq)]
pub struct Recovered {
    pub file: PathBuf,
    pub title: String,
    pub original: Option<PathBuf>,
}

impl Autosave {
    pub fn new(state_dir: &Path, interval: Duration) -> Self {
        let root = state_dir.join("recovery");
        Self {
            dir: root.join(std::process::id().to_string()),
            root,
            interval,
            last: Instant::now(),
            written: Default::default(),
        }
    }

    /// Minutes between passes; 0 stops autosaving.
    pub fn set_interval(&mut self, minutes: u32) {
        self.interval = if minutes == 0 {
            Duration::MAX
        } else {
            Duration::from_secs(minutes as u64 * 60)
        };
    }

    /// Whether a write pass is due.
    pub fn due(&self, now: Instant) -> bool {
        self.interval != Duration::MAX && now.duration_since(self.last) >= self.interval
    }

    /// Write changed, unsaved documents if the interval has passed.
    /// `docs`: (tab key, document, unsaved?, title, path).
    pub fn tick(
        &mut self,
        now: Instant,
        docs: Vec<(u64, Document, bool, String, Option<PathBuf>)>,
    ) {
        if !self.due(now) {
            return;
        }
        self.last = now;
        for (key, doc, dirty, title, path) in docs {
            if !dirty {
                self.forget(key);
                continue;
            }
            if self.written.get(&key) == Some(&doc.revision()) {
                continue;
            }
            self.written.insert(key, doc.revision());
            let dir = self.dir.clone();
            std::thread::spawn(move || {
                if let Err(e) = write(&dir, key, &doc, &title, path.as_deref()) {
                    log::warn!("autosave failed: {e}");
                }
            });
        }
    }

    /// The tab was saved or closed.
    pub fn forget(&mut self, key: u64) {
        if self.written.remove(&key).is_some() {
            let _ = std::fs::remove_file(self.dir.join(format!("{key}.ora")));
            let _ = std::fs::remove_file(self.dir.join(format!("{key}.txt")));
        }
    }

    /// Clean exit: nothing to recover.
    pub fn finish(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }

    /// Images left by sessions whose process has gone.
    pub fn recoverable(&self) -> Vec<Recovered> {
        let mut out = Vec::new();
        let Ok(sessions) = std::fs::read_dir(&self.root) else {
            return out;
        };
        for s in sessions.flatten() {
            let name = s.file_name().to_string_lossy().into_owned();
            let alive = name.parse::<u32>().is_ok_and(|pid| {
                pid == std::process::id() || Path::new(&format!("/proc/{pid}")).exists()
            });
            if alive {
                continue;
            }
            let Ok(files) = std::fs::read_dir(s.path()) else {
                continue;
            };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) != Some("ora") {
                    continue;
                }
                let meta = std::fs::read_to_string(p.with_extension("txt")).unwrap_or_default();
                let mut lines = meta.lines();
                let title = lines.next().unwrap_or("Recovered").to_owned();
                let original = lines.next().filter(|l| !l.is_empty()).map(PathBuf::from);
                out.push(Recovered {
                    file: p,
                    title,
                    original,
                });
            }
        }
        out.sort_by(|a, b| a.file.cmp(&b.file));
        out
    }

    /// Remove the crashed sessions' folders (after recovering or discarding).
    pub fn clear_recoverable(&self) {
        let Ok(sessions) = std::fs::read_dir(&self.root) else {
            return;
        };
        for s in sessions.flatten() {
            if s.path() != self.dir {
                let _ = std::fs::remove_dir_all(s.path());
            }
        }
    }
}

fn write(
    dir: &Path,
    key: u64,
    doc: &Document,
    title: &str,
    path: Option<&Path>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let bytes = pnn_io::ora::encode(doc).map_err(std::io::Error::other)?;
    // Write then rename, so a crash mid-write leaves the previous autosave.
    let tmp = dir.join(format!("{key}.ora.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dir.join(format!("{key}.ora")))?;
    let meta = format!(
        "{title}\n{}\n",
        path.map(|p| p.display().to_string()).unwrap_or_default()
    );
    std::fs::write(dir.join(format!("{key}.txt")), meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnn_core::{ColorBgra, Layer, Surface};

    #[test]
    fn writes_forgets_and_recovers_after_a_crash() {
        let state = tempfile::tempdir().unwrap();
        let mut a = Autosave::new(state.path(), Duration::ZERO);
        let mut s = Surface::new_transparent(10, 10);
        s.set(1, 1, ColorBgra::WHITE);
        let doc = Document::from_layers(10, 10, vec![Layer::new("Background", s)]);
        a.tick(
            Instant::now(),
            vec![(7, doc.clone(), true, "Untitled".into(), None)],
        );
        let file = a.dir.join("7.ora");
        for _ in 0..200 {
            if file.exists() && a.dir.join("7.txt").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(file.exists());
        // Our own session isn't "crashed".
        assert!(a.recoverable().is_empty());
        // Pretend a dead process left it: rename the folder to an unused pid.
        let dead = a.root.join("4000000000");
        std::fs::rename(&a.dir, &dead).unwrap();
        let r = a.recoverable();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Untitled");
        let back = pnn_io::load(&r[0].file).unwrap();
        assert_eq!(back.layer(0).surface.get(1, 1), ColorBgra::WHITE);
        a.clear_recoverable();
        assert!(!dead.exists());
        // Forget removes a tab's files; finish removes the session folder.
        a.tick(Instant::now(), vec![(8, doc, true, "x".into(), None)]);
        std::thread::sleep(Duration::from_millis(100));
        a.forget(8);
        assert!(!a.dir.join("8.ora").exists());
        a.finish();
        assert!(!a.dir.exists());
    }
}
