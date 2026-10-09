//! Background file work: native dialogs (via the XDG portal, so KDE's own picker), decoding
//! and encoding. Results come back as events polled once per frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use pnn_core::Document;
use pnn_io::{FileFormat, OPEN_EXTENSIONS, SaveOptions};

pub enum FileEvent {
    Opened {
        path: PathBuf,
        result: Result<Document, String>,
    },
    Saved {
        key: u64,
        path: PathBuf,
        format: FileFormat,
        revision: u64,
        result: Result<(), String>,
    },
    PickedOpen(Vec<PathBuf>),
    /// `None` = cancelled.
    PickedSave {
        key: u64,
        path: Option<PathBuf>,
    },
}

pub struct FileOps {
    tx: Sender<FileEvent>,
    rx: Receiver<FileEvent>,
    ctx: egui::Context,
    in_flight: usize,
    dialog_open: bool,
}

impl FileOps {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            ctx,
            in_flight: 0,
            dialog_open: false,
        }
    }

    /// Work is running in the background (status bar shows it).
    pub fn busy(&self) -> bool {
        self.in_flight > 0
    }

    /// A native file dialog is showing; the app behaves modally meanwhile.
    pub fn dialog_open(&self) -> bool {
        self.dialog_open
    }

    fn spawn(&mut self, name: &str, job: impl FnOnce() -> FileEvent + Send + 'static) {
        self.in_flight += 1;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let _ = tx.send(job());
                ctx.request_repaint();
            })
            .expect("spawn file thread");
    }

    pub fn open_paths(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            self.spawn("open", move || {
                let started = std::time::Instant::now();
                let result = pnn_io::load(&path).map_err(|e| e.to_string());
                if let Ok(doc) = &result {
                    log::info!(
                        "opened {} ({}x{}) in {:?}",
                        path.display(),
                        doc.width(),
                        doc.height(),
                        started.elapsed()
                    );
                }
                FileEvent::Opened { path, result }
            });
        }
    }

    pub fn pick_open(&mut self, dir: Option<PathBuf>) {
        self.dialog_open = true;
        self.spawn("open dialog", move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Open")
                .add_filter("All images", OPEN_EXTENSIONS);
            for f in FileFormat::ALL {
                dialog = dialog.add_filter(f.name(), f.extensions());
            }
            if let Some(dir) = dir {
                dialog = dialog.set_directory(dir);
            }
            FileEvent::PickedOpen(dialog.pick_files().unwrap_or_default())
        });
    }

    pub fn pick_save(&mut self, key: u64, current: Option<&Path>, title: &str) {
        self.dialog_open = true;
        let file_name = match current {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()),
            None => Some(format!("{title}.png")),
        };
        let dir = current.and_then(|p| p.parent()).map(Path::to_owned);
        self.spawn("save dialog", move || {
            let mut dialog = rfd::FileDialog::new().set_title("Save As");
            for f in FileFormat::ALL {
                dialog = dialog.add_filter(f.name(), f.extensions());
            }
            if let Some(name) = file_name {
                dialog = dialog.set_file_name(name);
            }
            if let Some(dir) = dir {
                dialog = dialog.set_directory(dir);
            }
            FileEvent::PickedSave {
                key,
                path: dialog.save_file(),
            }
        });
    }

    /// Encode and write on a background thread. `doc` is a cheap copy-on-write clone.
    pub fn save(
        &mut self,
        key: u64,
        doc: Document,
        path: PathBuf,
        format: FileFormat,
        options: SaveOptions,
    ) {
        let revision = doc.revision();
        self.spawn("save", move || {
            let result = pnn_io::save(&doc, &path, format, options).map_err(|e| e.to_string());
            FileEvent::Saved {
                key,
                path,
                format,
                revision,
                result,
            }
        });
    }

    pub fn poll(&mut self) -> Vec<FileEvent> {
        let events: Vec<FileEvent> = self.rx.try_iter().collect();
        self.in_flight -= events.len();
        if events
            .iter()
            .any(|e| matches!(e, FileEvent::PickedOpen(_) | FileEvent::PickedSave { .. }))
        {
            self.dialog_open = false;
        }
        events
    }
}

/// Resolve the format for a path chosen in Save As: by extension, or PNG with `.png` added
/// when there is none. Unknown extensions are an error, as in paint.net.
pub fn resolve_save_path(path: PathBuf) -> Result<(PathBuf, FileFormat), String> {
    if path.extension().is_none() {
        let mut path = path;
        path.set_extension("png");
        return Ok((path, FileFormat::Png));
    }
    match FileFormat::from_path(&path) {
        Some(f) => Ok((path, f)),
        None => Err(format!(
            "{}: unsupported file type. Use one of: {}.",
            path.display(),
            FileFormat::ALL
                .iter()
                .map(|f| f.extensions()[0])
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_path_resolution() {
        assert_eq!(
            resolve_save_path("a/b".into()).unwrap(),
            ("a/b.png".into(), FileFormat::Png)
        );
        assert_eq!(
            resolve_save_path("a/b.JPEG".into()).unwrap().1,
            FileFormat::Jpeg
        );
        assert!(resolve_save_path("a/b.xcf".into()).is_err());
    }
}
