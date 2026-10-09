//! Open documents (paint.net's image list) and per-document UI state.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use pnn_core::{Compositor, Document, History};
use pnn_io::{FileFormat, SaveOptions};
use pnn_render::View;

use crate::canvas::GpuSync;
use crate::icons::Icon;
use crate::windows::LayerThumbs;

pub struct DocumentTab {
    /// Unique for the session; keys GPU resources.
    pub key: u64,
    pub doc: Document,
    pub path: Option<PathBuf>,
    /// Format of the file on disk (for Save without Save As).
    pub format: Option<FileFormat>,
    pub save_options: SaveOptions,
    /// `doc.revision()` at the last open/save; anything newer is unsaved.
    pub saved_revision: u64,
    /// `None` until the canvas has been laid out once (needs the viewport size).
    pub view: Option<View>,
    /// View before Ctrl+B, restored by a second Ctrl+B.
    pub view_before_fit: Option<View>,
    pub compositor: Compositor,
    pub gpu: GpuSync,
    pub layer_thumbs: LayerThumbs,
    /// Undo history; the History window lists its steps.
    pub history: History<Icon>,
    pub thumbnail: Option<egui::TextureHandle>,
    pub thumbnail_revision: u64,
    pub thumbnail_updated: Option<Instant>,
}

impl DocumentTab {
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned())
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision
    }

    pub fn mark_saved(&mut self, revision: u64) {
        self.saved_revision = revision;
    }

    /// Record the current document as a history step.
    pub fn commit(&mut self, name: &str, icon: Icon) {
        self.history.push(name, icon, &self.doc);
    }

    pub fn undo(&mut self) -> bool {
        match self.history.undo() {
            Some(doc) => {
                self.doc = doc.clone();
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo() {
            Some(doc) => {
                self.doc = doc.clone();
                true
            }
            None => false,
        }
    }

    /// History window click.
    pub fn jump_to(&mut self, step: usize) {
        if let Some(doc) = self.history.jump(step) {
            self.doc = doc.clone();
        }
    }

    pub fn image_size(&self) -> [f64; 2] {
        [self.doc.width() as f64, self.doc.height() as f64]
    }
}

#[derive(Default)]
pub struct Workspace {
    tabs: Vec<DocumentTab>,
    active: Option<usize>,
    next_key: u64,
}

impl Workspace {
    pub fn tabs(&self) -> &[DocumentTab] {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut [DocumentTab] {
        &mut self.tabs
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&DocumentTab> {
        self.active.map(|i| &self.tabs[i])
    }

    pub fn active_mut(&mut self) -> Option<&mut DocumentTab> {
        self.active.map(|i| &mut self.tabs[i])
    }

    pub fn find(&self, key: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.key == key)
    }

    pub fn tab_mut(&mut self, key: u64) -> Option<&mut DocumentTab> {
        self.tabs.iter_mut().find(|t| t.key == key)
    }

    /// Add a document as a new tab at the end and make it active.
    pub fn add(&mut self, doc: Document, path: Option<PathBuf>, format: Option<FileFormat>) -> u64 {
        self.next_key += 1;
        let key = self.next_key;
        let history = if path.is_some() {
            History::new("Open Image", Icon::Open, &doc)
        } else {
            History::new("New Image", Icon::New, &doc)
        };
        self.tabs.push(DocumentTab {
            key,
            history,
            saved_revision: doc.revision(),
            doc,
            path,
            format,
            save_options: SaveOptions::default(),
            view: None,
            view_before_fit: None,
            compositor: Compositor::new(),
            gpu: GpuSync::default(),
            layer_thumbs: LayerThumbs::default(),
            thumbnail: None,
            thumbnail_revision: 0,
            thumbnail_updated: None,
        });
        self.active = Some(self.tabs.len() - 1);
        key
    }

    /// Remove a tab. The tab to its left becomes active (or the new first tab).
    pub fn remove(&mut self, key: u64) -> Option<DocumentTab> {
        let i = self.find(key)?;
        let tab = self.tabs.remove(i);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a > i => Some(a - 1),
            Some(a) if a == i => Some(i.saturating_sub(1).min(self.tabs.len() - 1)),
            other => other,
        };
        Some(tab)
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = Some(index);
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab, wrapping around.
    pub fn cycle(&mut self, forward: bool) {
        let n = self.tabs.len();
        if let Some(a) = self.active
            && n > 0
        {
            self.active = Some(if forward {
                (a + 1) % n
            } else {
                (a + n - 1) % n
            });
        }
    }

    /// Ctrl+Shift+PgUp / PgDn: move the active tab one place.
    pub fn move_active(&mut self, right: bool) {
        let Some(a) = self.active else { return };
        let b = if right { a + 1 } else { a.wrapping_sub(1) };
        if b < self.tabs.len() {
            self.tabs.swap(a, b);
            self.active = Some(b);
        }
    }

    /// Drag-and-drop reorder in the image list.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let active_key = self.active().map(|t| t.key);
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = active_key.and_then(|k| self.find(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(n: usize) -> (Workspace, Vec<u64>) {
        let mut w = Workspace::default();
        let keys = (0..n)
            .map(|_| w.add(Document::new_blank(4, 4), None, None))
            .collect();
        (w, keys)
    }

    #[test]
    fn add_activates_new_tab() {
        let (w, keys) = ws(3);
        assert_eq!(w.active().unwrap().key, keys[2]);
        assert_eq!(w.active().unwrap().title(), "Untitled");
        assert!(!w.active().unwrap().is_dirty());
    }

    #[test]
    fn cycle_wraps() {
        let (mut w, keys) = ws(3);
        w.cycle(true);
        assert_eq!(w.active().unwrap().key, keys[0]);
        w.cycle(false);
        assert_eq!(w.active().unwrap().key, keys[2]);
    }

    #[test]
    fn move_active_keeps_it_active() {
        let (mut w, keys) = ws(3);
        w.activate(0);
        w.move_active(true);
        assert_eq!(w.tabs()[1].key, keys[0]);
        assert_eq!(w.active_index(), Some(1));
        w.move_active(false);
        w.move_active(false);
        assert_eq!(w.active_index(), Some(0), "no move past the start");
    }

    #[test]
    fn remove_activates_left_neighbour() {
        let (mut w, keys) = ws(3);
        w.activate(1);
        w.remove(keys[1]);
        assert_eq!(w.active().unwrap().key, keys[0]);
        w.remove(keys[0]);
        assert_eq!(w.active().unwrap().key, keys[2]);
        w.remove(keys[2]);
        assert!(w.active().is_none());
    }

    #[test]
    fn removing_a_tab_before_active_keeps_same_document_active() {
        let (mut w, keys) = ws(3);
        w.remove(keys[0]);
        assert_eq!(w.active().unwrap().key, keys[2]);
    }

    #[test]
    fn dirty_tracking() {
        let (mut w, _) = ws(1);
        let tab = w.active_mut().unwrap();
        tab.doc.invalidate_all();
        assert!(tab.is_dirty());
        let rev = tab.doc.revision();
        tab.mark_saved(rev);
        assert!(!tab.is_dirty());
    }

    #[test]
    fn drag_reorder_keeps_active_document() {
        let (mut w, keys) = ws(3);
        w.activate(0);
        w.move_tab(0, 2);
        assert_eq!(w.tabs()[2].key, keys[0]);
        assert_eq!(w.active().unwrap().key, keys[0]);
    }
}
